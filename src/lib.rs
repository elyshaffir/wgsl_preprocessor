use std::{any, borrow, collections::HashMap, fmt::Display, mem, path};

const INSTRUCTION_PREFIX: &str = "//!";
const INCLUDE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "include");
const DEFINE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "define");

/// Type for data types that would be later put in a vertex buffer.
pub trait VertexBufferData
where
	Self: Sized,
{
	/// Generate the attributes of the [`wgpu::VertexBufferLayout`] that will be created for the implementic struct.
	///
	/// Example:
	///
	/// ```ignore
	/// impl VertexBufferData for [f32; 4] {
	/// 	fn buffer_attributes<'a>() -> &'a [wgpu::VertexAttribute] {
	/// 		&wgpu::vertex_attr_array![0 => Float32x4]
	/// 	}
	/// }
	/// ```
	fn buffer_attributes<'a>() -> &'a [wgpu::VertexAttribute];

	/// Generate a [`wgpu::VertexBufferLayout`] for the implementing struct.
	///
	/// Example:
	///
	/// ```ignore
	/// let vertex_state = wgpu::VertexState {
	/// 	module: ...,
	/// 	entry_point: "...",
	/// 	buffers: &[<[f32; 4]>::describe()],
	/// }
	/// ```
	fn describe<'a>() -> wgpu::VertexBufferLayout<'a> {
		wgpu::VertexBufferLayout {
			array_stride: mem::size_of::<Self>() as wgpu::BufferAddress,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: Self::buffer_attributes(),
		}
	}
}

/// Type for data types that can be defined in WGSL.
pub trait WGSLType {
	/// The name of the type in WGSL syntax.
	const TYPE_NAME: &'static str;

	/// Generates the string representation of the data type in WGSL syntax.
	///
	/// Example:
	///
	/// ```ignore
	/// struct Struct {
	/// 	pub data: [f32; 4],
	/// }
	///
	/// impl WGSLType for Struct {
	/// 	const TYPE_NAME: &'static str = "Struct";
	/// 	fn string_definition(&self) -> String {
	/// 		format!(
	/// 			"vec4<f32>({})",
	/// 			format!("{:?}", self.data).replace(&['[', ']'], "")
	/// 		)
	/// 	}
	/// }
	/// ```
	fn string_definition(&self) -> String;
}

/// Wraps shader code, changes it and builds it into a [`wgpu::ShaderModuleDescriptor`].
pub struct ShaderBuilder {
	source_path: String,
	code: String,
}

impl ShaderBuilder {
	/// Creates a new [`ShaderBuilder`].
	///
	/// # Arguments
	/// - `source_path` - Path to the root WGSL module.
	///		All includes will be relative to the parent directory of the root WGSL module.
	/// 	Code is generated recursively with attention to `include` statements like C's #include statement.
	pub fn new(source_path: &str) -> Result<Self, ex::io::Error> {
		let module_path = path::Path::new(&source_path);
		let code = Self::load_shader_module(
			module_path.parent().unwrap_or(path::Path::new("./")),
			module_path,
		)?;
		Ok(Self {
			source_path: source_path.to_string(),
			code,
		})
	}

	/// Performs the WGSL's parallel to C's `#define` statement.
	///
	/// # Arguments
	/// - `name` - Name of the constant; the string to replace in the code.
	/// - `value` - Value of the constant.
	pub fn put_constant<T: Display>(&mut self, name: &str, value: T) -> &mut Self {
		// TODO change to WGSLType and implement for primitive types and vectors.
		let type_name = any::type_name::<T>();
		self.code = self.code.replace(name, &format!("{type_name}({value})"));
		self
	}

	/// Calls [`ShaderBuilder::put_constant`] for every (key, value) pair in a given [`HashMap`].
	pub fn put_constant_map<T: Display + Copy>(
		&mut self,
		constant_map: &HashMap<&str, T>,
	) -> &mut Self {
		constant_map.iter().for_each(|(name, &value)| {
			self.put_constant(name, value);
		});
		self
	}

	/// Defines a constant array of elements.
	///
	/// # Arguments
	/// - `name` - Name of the array in the WGSL source.
	/// - `array` - Vector of [`WGSLType`] whose elements will be the elements in the array.
	pub fn put_array_definition<T: WGSLType>(&mut self, name: &str, array: &Vec<&T>) -> &mut Self {
		let type_name = T::TYPE_NAME;
		let array_length = array.len();
		let mut string_definition = String::new();

		string_definition.push_str(&format!(
			"var<private> {name}: array<{type_name}, {array_length}> = array<{type_name}, {array_length}>("
		));

		for struct_value in array.iter() {
			let struct_string = struct_value.string_definition();
			string_definition.push_str(&format!("{type_name}({struct_string}),"));
		}

		string_definition.push_str(");");

		self.code = self
			.code
			.replace(&format!("{DEFINE_INSTRUCTION} {name}"), &string_definition);
		self
	}

	/// Builds a [`wgpu::ShaderModuleDescriptor`] from the shader.
	/// The `label` member of the built [`wgpu::ShaderModuleDescriptor`] is the name of the shader file without the postfix.
	pub fn build(&self) -> wgpu::ShaderModuleDescriptor {
		wgpu::ShaderModuleDescriptor {
			label: Some(
				&self
					.source_path
					.rsplit(['/', '.'])
					.nth(1)
					.unwrap_or(&self.source_path),
			),
			source: wgpu::ShaderSource::Wgsl(borrow::Cow::Borrowed(&self.code)),
		}
	}

	fn load_shader_module(
		base_path: &path::Path,
		module_path: &path::Path,
	) -> Result<String, ex::io::Error> {
		let module_source = ex::fs::read_to_string(module_path)?;
		let mut module_string = String::new();
		for line in module_source.lines() {
			if line.starts_with(INCLUDE_INSTRUCTION) {
				for include in line.split_whitespace().skip(1) {
					module_string.push_str(&Self::load_shader_module(
						base_path,
						&path::Path::new(include),
					)?);
				}
			} else {
				module_string.push_str(line);
			}
		}
		Ok(module_string)
	}
}

#[cfg(test)]
mod tests {
	use crate::{ShaderBuilder, WGSLType};
	use std::{collections::HashMap, io};

	#[test]
	fn nonexistent() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/nonexistent.wgsl")
				.err()
				.unwrap()
				.kind(),
			io::ErrorKind::NotFound
		);
	}

	#[test]
	fn standard_include() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/includer.wgsl")
				.unwrap()
				.code,
			ShaderBuilder::new("test_shaders/included.wgsl")
				.unwrap()
				.code
		);
	}

	#[test]
	fn missing_include() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/missing_include.wgsl")
				.err()
				.unwrap()
				.kind(),
			io::ErrorKind::NotFound
		);
	}

	#[test]
	fn nested_include() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/nested_include.wgsl")
				.unwrap()
				.code,
			ShaderBuilder::new("test_shaders/includer.wgsl")
				.unwrap()
				.code
		)
	}

	#[test]
	fn multiple_includes() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/multiple_includes.wgsl")
				.unwrap()
				.code,
			format!(
				"{}{}",
				ShaderBuilder::new("test_shaders/included.wgsl")
					.unwrap()
					.code,
				ShaderBuilder::new("test_shaders/included2.wgsl")
					.unwrap()
					.code
			)
		)
	}

	#[test]
	fn multiple_inline_includes() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/multiple_inline_includes.wgsl")
				.unwrap()
				.code,
			ShaderBuilder::new("test_shaders/multiple_includes.wgsl")
				.unwrap()
				.code
		)
	}

	#[test]
	fn put_constant() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/set_constants.wgsl")
				.unwrap()
				.put_constant("ONE", 1u32)
				.put_constant("TWO", 2u32)
				.code,
			ShaderBuilder::new("test_shaders/set_constants_processed.wgsl")
				.unwrap()
				.code
		)
	}

	#[test]
	fn put_constant_map() {
		let mut constants = HashMap::new();
		constants.insert("ONE", 1u32);
		constants.insert("TWO", 2u32);
		assert_eq!(
			ShaderBuilder::new("test_shaders/set_constants.wgsl")
				.unwrap()
				.put_constant_map(&constants)
				.code,
			ShaderBuilder::new("test_shaders/set_constants_processed.wgsl")
				.unwrap()
				.code
		)
	}

	#[test]
	fn load_proper_label() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/included.wgsl")
				.unwrap()
				.build()
				.label
				.unwrap(),
			"included"
		);
	}

	#[test]
	fn put_array_definition_structs() {
		struct Struct {
			pub data: [f32; 4],
		}
		impl WGSLType for Struct {
			const TYPE_NAME: &'static str = "Struct";

			fn string_definition(&self) -> String {
				format!(
					"vec4<f32>({})",
					format!("{:?}", self.data).replace(&['[', ']'], "")
				)
			}
		}
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_structs.wgsl")
				.unwrap()
				.put_array_definition(
					"STRUCT_ARRAY",
					&vec![
						&Struct {
							data: [1.0, 2.0, 3.0, 4.0]
						},
						&Struct {
							data: [1.5, 2.1, 3.7, 4.9]
						}
					]
				)
				.code,
			ShaderBuilder::new("test_shaders/put_array_definition_structs_processed.wgsl")
				.unwrap()
				.code
		)
	}

	#[test]
	fn put_array_definition_vectors() {
		impl WGSLType for [f32; 4] {
			const TYPE_NAME: &'static str = "vec4<f32>";

			fn string_definition(&self) -> String {
				format!("{:?}", self).replace(&['[', ']'], "")
			}
		}
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_vectors.wgsl")
				.unwrap()
				.put_array_definition(
					"VECTOR_ARRAY",
					&vec![&[1.0, 2.0, 3.0, 4.0], &[1.5, 2.1, 3.7, 4.9]]
				)
				.code,
			ShaderBuilder::new("test_shaders/put_array_definition_vectors_processed.wgsl")
				.unwrap()
				.code
		)
	}
}
