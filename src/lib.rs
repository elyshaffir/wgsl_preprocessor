use std::{any, borrow, collections::HashMap, fmt::Display, fs, io, io::Read, mem, path};
// todo documentation for public interface.
pub trait BufferData {
	// todo add vertex to the name
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

fn load_shader_module(module_path: &str) -> String {
	let module_path = path::PathBuf::from(module_path);
	if !module_path.is_file() {
		// todo convert to Result<...> and don't panic.
		panic!("Shader not found: {:?}", module_path);
	}

	let mut module_source = String::new();
	io::BufReader::new(fs::File::open(&module_path).unwrap()) // todo error handling, never unwrap().
		.read_to_string(&mut module_source)
		.unwrap();
	let mut module_string = String::new();

	let first_line = module_source.lines().next().unwrap(); // todo include should be possible everywhere and not just in the first line.
	if first_line.starts_with("//!include") {
		// todo proper string constant for every macro ("include", "define") and extract the "//!" prefix.
		for include in first_line.split_whitespace().skip(1) {
			module_string.push_str(&*load_shader_module(include));
		}
	}

	module_string.push_str(&module_source);
	module_string
}

pub trait WGSLData {
	// todo rename to WGSLType or something.
	fn string_definition(&self) -> String;
}

pub struct Shader {
	name: String, // todo rename
	code: String, // todo rename?
}

impl Shader {
	pub fn new(name: String) -> Self {
		let code = load_shader_module(&name);
		Self { name, code }
	}

	pub fn define_str(&mut self, name: &str, value: &str) -> &mut Self {
		// todo change to generic define (not just strings). Also, see define_many to see if this method is deprecated.
		self.code = self.code.replace(name, value);
		self
	}

	pub fn define_struct_array<T: WGSLData>(&mut self, name: &str, structs: &Vec<&T>) -> &mut Self {
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

		self.define_once(name, &string_definition)
	}

	pub fn define_many<T: Display>(&mut self, defines: &HashMap<&String, T>) -> &mut Self {
		// todo rename to define and have as only way to define constants (ie. deprecate define_str)
		let type_name = any::type_name::<T>();
		for (name, value) in defines.iter() {
			self.define_str(name, &format!("{type_name}({value})"));
		}
		self
	}

	pub fn load(&self) -> wgpu::ShaderModuleDescriptor {
		// todo rename to build() or some other builder pattern name
		wgpu::ShaderModuleDescriptor {
			label: Some(&self.name),
			source: wgpu::ShaderSource::Wgsl(borrow::Cow::Borrowed(&self.code)),
		}
	}

	fn define_once(&mut self, name: &str, value: &str) -> &mut Self {
		self.define_str(&format!("//!define {name}"), value) // todo see //!include and follow suite.
	}
}
