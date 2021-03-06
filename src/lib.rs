use std::{any, borrow, collections::HashMap, mem, path};

const INSTRUCTION_PREFIX: &str = "//!";
const INCLUDE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "include");
const DEFINE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "define");

// todo use crate type_info to implement WGSLType for structs automatically (attribute). fields need to implement WGSLType recursively.

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
/// [`WGSLType`] is already implemented for some primitive types.
pub trait WGSLType {
	/// Whether to cast the type when used inside an array, like in [`ShaderBuilder::put_array_definition`].
	/// Shouldn't be modified, used only in the implementations of `u32`, `i32`, `f32`.
	const NO_CAST_ARRAY: bool = false;

	/// Returns the name of the type in WGSL syntax.
	fn type_name() -> String;

	/// Returns a string that defines the type in WGSL syntax.
	fn string_definition(&self) -> String;
}

#[duplicate::duplicate_item(wgsl_type; [u32]; [i32]; [f32])]
impl WGSLType for wgsl_type {
	const NO_CAST_ARRAY: bool = true;

	fn type_name() -> String {
		any::type_name::<wgsl_type>().to_string()
	}

	fn string_definition(&self) -> String {
		format!("{}({self})", Self::type_name())
	}
}

#[cfg(all(feature = "cgmath_vectors", feature = "array_vectors"))]
compile_error!(
	"Feature \"cgmath_vectors\" and feature \"array_vectors\" cannot be enabled at the same time."
);

#[cfg(feature = "array_vectors")]
#[duplicate::duplicate_item(wgsl_type; [[u32; 2]]; [[i32; 2]]; [[f32; 2]]; [[u32; 3]]; [[i32; 3]]; [[f32; 3]]; [[u32; 4]]; [[i32; 4]]; [[f32; 4]])]
impl WGSLType for wgsl_type {
	fn type_name() -> String {
		format!(
			"vec{}<{}>",
			std::mem::size_of::<wgsl_type>() / 4,
			any::type_name::<wgsl_type>()
				.split(['[', ';'])
				.nth(1)
				.unwrap()
		)
	}

	fn string_definition(&self) -> String {
		format!("{}({:?})", Self::type_name(), self).replace(&['[', ']'], "")
	}
}

#[cfg(feature = "cgmath_vectors")]
#[duplicate::duplicate_item(wgsl_type; [cgmath::Vector2<u32>]; [cgmath::Vector2<i32>]; [cgmath::Vector2<f32>]; [cgmath::Vector3<u32>]; [cgmath::Vector3<i32>]; [cgmath::Vector3<f32>]; [cgmath::Vector4<u32>]; [cgmath::Vector4<i32>]; [cgmath::Vector4<f32>])]
impl WGSLType for wgsl_type {
	fn type_name() -> String {
		format!(
			"vec{}<{}>",
			std::mem::size_of::<wgsl_type>() / 4,
			any::type_name::<wgsl_type>()
				.split(['<', '>'])
				.nth(1)
				.unwrap()
		)
	}

	fn string_definition(&self) -> String {
		println!("{}", format!("{:?}", self));
		format!(
			"{}({})",
			Self::type_name(),
			format!("{:?}", self)
				.replace("Vector", "")
				.split(&['[', ']'])
				.nth(1)
				.unwrap()
		)
	}
}

impl WGSLType for bool {
	fn type_name() -> String {
		"bool".to_string()
	}

	fn string_definition(&self) -> String {
		self.to_string()
	}
}

/// Wraps shader code, changes it and builds it into a [`wgpu::ShaderModuleDescriptor`].
pub struct ShaderBuilder {
	/// String with the current WGSL source.
	pub source_string: String,
	source_path: String,
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
		let source_string = Self::load_shader_module(
			module_path.parent().unwrap_or(path::Path::new("./")),
			module_path,
		)?;
		Ok(Self {
			source_string,
			source_path: source_path.to_string(),
		})
	}

	/// Performs the WGSL's parallel to C's `#define` statement.
	///
	/// # Arguments
	/// - `name` - Name of the constant; the string to replace in the code.
	/// - `value` - Value of the constant.
	pub fn put_constant(&mut self, name: &str, value: impl WGSLType) -> &mut Self {
		self.source_string = self.source_string.replace(name, &value.string_definition());
		self
	}

	/// Calls [`ShaderBuilder::put_constant`] for every (key, value) pair in a given [`HashMap`].
	pub fn put_constant_map(
		&mut self,
		constant_map: &HashMap<&str, impl WGSLType + Copy>,
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
	pub fn put_array_definition<'a, T: 'a + WGSLType>(
		&'a mut self,
		name: &str,
		array: &Vec<&T>,
	) -> &'a mut Self {
		let type_name = T::type_name();
		let array_length = array.len();
		let mut string_definition = String::new();

		string_definition.push_str(&format!(
			"var<private> {name}: array<{type_name}, {array_length}> = array<{type_name}, {array_length}>("
		));

		for struct_value in array.iter() {
			let mut struct_definition = struct_value.string_definition();
			if T::NO_CAST_ARRAY {
				struct_definition = struct_definition
					.rsplit(&['(', ')'])
					.nth(1)
					.unwrap_or(&string_definition)
					.to_string()
			}
			string_definition.push_str(&struct_definition);
			string_definition.push(',');
		}

		string_definition.push_str(");");

		self.source_string = self
			.source_string
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
			source: wgpu::ShaderSource::Wgsl(borrow::Cow::Borrowed(&self.source_string)),
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
				.source_string,
			ShaderBuilder::new("test_shaders/included.wgsl")
				.unwrap()
				.source_string
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
				.source_string,
			ShaderBuilder::new("test_shaders/includer.wgsl")
				.unwrap()
				.source_string
		)
	}

	#[test]
	fn multiple_includes() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/multiple_includes.wgsl")
				.unwrap()
				.source_string,
			format!(
				"{}{}",
				ShaderBuilder::new("test_shaders/included.wgsl")
					.unwrap()
					.source_string,
				ShaderBuilder::new("test_shaders/included2.wgsl")
					.unwrap()
					.source_string
			)
		)
	}

	#[test]
	fn multiple_inline_includes() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/multiple_inline_includes.wgsl")
				.unwrap()
				.source_string,
			ShaderBuilder::new("test_shaders/multiple_includes.wgsl")
				.unwrap()
				.source_string
		)
	}

	#[test]
	fn put_constant() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/set_constants.wgsl")
				.unwrap()
				.put_constant("ONE", 1u32)
				.put_constant("TWO", 2u32)
				.source_string,
			ShaderBuilder::new("test_shaders/set_constants_processed.wgsl")
				.unwrap()
				.source_string
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
				.source_string,
			ShaderBuilder::new("test_shaders/set_constants_processed.wgsl")
				.unwrap()
				.source_string
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
	fn put_array_definition_bools() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_bools.wgsl")
				.unwrap()
				.put_array_definition("BOOL_ARRAY", &vec![&true, &false])
				.source_string,
			ShaderBuilder::new("test_shaders/put_array_definition_bools_processed.wgsl")
				.unwrap()
				.source_string
		)
	}

	#[test]
	fn put_array_definition_scalar() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_scalars.wgsl")
				.unwrap()
				.put_array_definition("SCALAR_ARRAY", &vec![&1, &0])
				.source_string,
			ShaderBuilder::new("test_shaders/put_array_definition_scalars_processed.wgsl")
				.unwrap()
				.source_string
		)
	}

	#[test]
	fn put_array_definition_structs() {
		struct Struct {
			pub data: [f32; 4],
		}
		impl WGSLType for Struct {
			fn type_name() -> String {
				"Struct".to_string()
			}

			fn string_definition(&self) -> String {
				format!("{}(vec4<f32>({:?}))", Self::type_name(), self.data)
					.replace(&['[', ']'], "")
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
				.source_string,
			ShaderBuilder::new("test_shaders/put_array_definition_structs_processed.wgsl")
				.unwrap()
				.source_string
		)
	}

	#[cfg(feature = "array_vectors")]
	#[test]
	fn put_array_definition_array_vectors() {
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

	#[cfg(feature = "cgmath_vectors")]
	#[test]
	fn put_array_definition_cgmath_vectors() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_vectors.wgsl")
				.unwrap()
				.put_array_definition(
					"VECTOR_ARRAY",
					&vec![
						&cgmath::Vector4::<f32>::new(1.0, 2.0, 3.0, 4.0),
						&cgmath::Vector4::<f32>::new(1.5, 2.1, 3.7, 4.9)
					]
				)
				.source_string,
			ShaderBuilder::new("test_shaders/put_array_definition_vectors_processed.wgsl")
				.unwrap()
				.source_string
		)
	}
}
