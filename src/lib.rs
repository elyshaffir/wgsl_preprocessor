use std::{any, borrow, collections::HashMap, fmt::Display, mem, path};

const INSTRUCTION_PREFIX: &str = "//!";
const INCLUDE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "include");
const DEFINE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "define");

// todo documentation for public interface.
pub trait VertexBufferData {
	type DataType;

	fn buffer_attributes<'a>() -> &'a [wgpu::VertexAttribute];

	fn describe<'a>() -> wgpu::VertexBufferLayout<'a> {
		wgpu::VertexBufferLayout {
			array_stride: mem::size_of::<Self::DataType>() as wgpu::BufferAddress,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: Self::buffer_attributes(),
		}
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
				module_string.push_str(&load_shader_module(base_path, &path::Path::new(include))?);
			}
		} else {
			module_string.push_str(line);
		}
	}
	Ok(module_string)
}

pub trait WGSLType {
	fn string_definition(&self) -> String;
}

pub struct Shader {
	source_path: String,
	code: String,
}

impl Shader {
	pub fn new(source_path: &str) -> Result<Self, ex::io::Error> {
		let module_path = path::Path::new(&source_path);
		let code = load_shader_module(module_path.parent().unwrap(), module_path)?; // todo document the unwrap
		Ok(Self {
			source_path: source_path.to_string(),
			code,
		})
	}

	pub fn apply_constant<T: Display>(&mut self, name: &str, value: T) -> &mut Self {
		let type_name = any::type_name::<T>();
		self.code = self.code.replace(name, &format!("{type_name}({value})"));
		self
	}

	pub fn apply_constant_map<T: Display + Copy>(
		&mut self,
		constant_map: &HashMap<&str, T>,
	) -> &mut Self {
		constant_map.iter().for_each(|(name, &value)| {
			self.apply_constant(name, value);
		});
		self
	}

	pub fn define_struct_array<T: WGSLType>(&mut self, name: &str, structs: &Vec<&T>) -> &mut Self {
		// todo rename to define_constant_array or something
		let type_name = any::type_name::<T>().split("::").last().unwrap(); // todo allow type name to be optionally specified in WGSLData.
		let array_length = structs.len();
		let mut string_definition = String::new();

		string_definition.push_str(&format!(
			"var<private> {name}: array<{type_name}, {array_length}> = array<{type_name}, {array_length}>("
		));

		for struct_value in structs.iter() {
			let struct_string = struct_value.string_definition();
			string_definition.push_str(&format!("{type_name}({struct_string}),"));
		}

		string_definition.push_str(");");

		self.code = self
			.code
			.replace(&format!("{DEFINE_INSTRUCTION} {name}"), &string_definition);
		self
	}

	pub fn build(&self) -> wgpu::ShaderModuleDescriptor {
		wgpu::ShaderModuleDescriptor {
			label: Some(&self.source_path.rsplit(['/', '.']).nth(1).unwrap()),
			source: wgpu::ShaderSource::Wgsl(borrow::Cow::Borrowed(&self.code)),
		}
	}
}

#[cfg(test)]
mod tests {
	use std::{collections::HashMap, io};

	use crate::Shader;

	#[test]
	fn nonexistent() {
		assert_eq!(
			Shader::new("test_shaders/nonexistent.wgsl")
				.err()
				.unwrap()
				.kind(),
			io::ErrorKind::NotFound
		);
	}

	#[test]
	fn standard_include() {
		assert_eq!(
			Shader::new("test_shaders/includer.wgsl").unwrap().code,
			Shader::new("test_shaders/included.wgsl").unwrap().code
		);
	}

	#[test]
	fn missing_include() {
		assert_eq!(
			Shader::new("test_shaders/missing_include.wgsl")
				.err()
				.unwrap()
				.kind(),
			io::ErrorKind::NotFound
		);
	}

	#[test]
	fn nested_include() {
		assert_eq!(
			Shader::new("test_shaders/nested_include.wgsl")
				.unwrap()
				.code,
			Shader::new("test_shaders/includer.wgsl").unwrap().code
		)
	}

	#[test]
	fn multiple_includes() {
		assert_eq!(
			Shader::new("test_shaders/multiple_includes.wgsl")
				.unwrap()
				.code,
			format!(
				"{}{}",
				Shader::new("test_shaders/included.wgsl").unwrap().code,
				Shader::new("test_shaders/included2.wgsl").unwrap().code
			)
		)
	}

	#[test]
	fn multiple_inline_includes() {
		assert_eq!(
			Shader::new("test_shaders/multiple_inline_includes.wgsl")
				.unwrap()
				.code,
			Shader::new("test_shaders/multiple_includes.wgsl")
				.unwrap()
				.code
		)
	}

	#[test]
	fn set_constant() {
		assert_eq!(
			Shader::new("test_shaders/set_constants.wgsl")
				.unwrap()
				.apply_constant("ONE", 1u32)
				.apply_constant("TWO", 2u32)
				.code,
			Shader::new("test_shaders/set_constants_processed.wgsl")
				.unwrap()
				.code
		)
	}

	#[test]
	fn apply_constant_map() {
		let mut constants = HashMap::new();
		constants.insert("ONE", 1u32);
		constants.insert("TWO", 2u32);
		assert_eq!(
			Shader::new("test_shaders/set_constants.wgsl")
				.unwrap()
				.apply_constant_map(&constants)
				.code,
			Shader::new("test_shaders/set_constants_processed.wgsl")
				.unwrap()
				.code
		)
	}

	#[test]
	fn load_proper_label() {
		assert_eq!(
			Shader::new("test_shaders/included.wgsl")
				.unwrap()
				.build()
				.label
				.unwrap(),
			"included"
		);
	}
}
