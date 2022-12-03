/*!
This crate provides a library for performing similar actions to what is expected from a preprocessor in WGSL.
Since WGSL [will not have a preprocessor](https://github.com/gpuweb/gpuweb/issues/568) at least for version 1.0,
this crate provides solutions to some common problems like including shader files and defining constants from
Rust code.

# Example: Include Multiple Shader Files

Here are the contents of the three shader files in this example (there are blank lines at the end of each included file):
`test_shaders/main.wgsl`:
```wgsl
//!include test_shaders/included.wgsl test_shaders/included2.wgsl
```
`test_shaders/included.wgsl`:
```wgsl
struct TestStruct {
	test_data: vec4<f32>;
};

```
`test_shaders/included2.wgsl`:
```wgsl
struct AnotherTestStruct {
	another_test_data: vec3<u32>;
};

```
With these `include` statements, `main.wgsl`, becomes:
```wgsl
struct TestStruct {
	test_data: vec4<f32>;
};
struct AnotherTestStruct {
	another_test_data: vec3<u32>;
};

```
It is important to note that `test_shaders/main.wgsl` could also contain:
```wgsl
//!include test_shaders/included.wgsl
//!include test_shaders/included2.wgsl
```
The result would be the same.

# Example: Define Macros

Non-function-like macro definitions are supported, for example:
```wgsl
//!define u3 vec3<u32>
@compute
@workgroup_size(64)
fn main(@builtin(global_invocation_id) id: u3) {
	// ...
}
```
With this `define` statement, the source becomes:
```wgsl
@compute
@workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
	// ...
}
```
Multi-line macros are not yet supported.

# Example: Defining a Constant Struct Array

Let's say some color constants are calculated before shader compile time and should be injected into the
code for performance reasons.
`main.wgsl` would contain:
```wgsl
struct Struct {
	data: vec4<f32>,
}
//!define STRUCT_ARRAY
```
In the Rust code, `Struct` is defined, and given an implementation of [`WGSLType`] it can be translated to
a WGSL struct with a single `vec4<f32>` member named `data`.
The Rust code building and compiling the shaders will contain:
```
use wgsl_preprocessor::WGSLType;

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
```
After building and compiling `main.wgsl` with the following array definition:
```no_run
use wgsl_preprocessor::ShaderBuilder;

# use wgsl_preprocessor::WGSLType;
# struct Struct {
# 	pub data: [f32; 4],
# }
# impl WGSLType for Struct {
# 	fn type_name() -> String {
# 		"Struct".to_string()
# 	}
#
# 	fn string_definition(&self) -> String {
# 		format!("{}(vec4<f32>({:?}))", Self::type_name(), self.data)
# 			.replace(&['[', ']'], "")
# 	}
# }
ShaderBuilder::new("main.wgsl", None)
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
	.build();
```
The compiled contents would be identical to:
```wgsl
var<private> STRUCT_ARRAY: array<Struct, 2> = array<Struct, 2>(Struct(vec4<f32>(1.0, 2.0, 3.0, 4.0)),Struct(vec4<f32>(1.5, 2.1, 3.7, 4.9)),);
```

#example: Parsing ifdef, else and ifndef
In this example we have a condition specifying wether we want our shader to work
with 2 dimentional vectors or 3 dimensional vectors.
```wgsl
//!ifdef THREE_DIMENTIONAL
struct Struct { data: vec3<f32>, }
//!else
struct Struct { data: vec2<f32>, }
//!endif

//!ifndef THREE_DIMENTIONAL
struct OtherStruct { data: vec2<f32>, }
//!else
struct OtherStruct { data: vec3<f32>, }
//!endif
```
In the Rust code we then pass the `parse_defs` function ana array containing all
of the vaild definitions (this is designed for compatability with the bitflags crate).

```no_run
use wgsl_preprocessor::ShaderBuilder;

ShaderBuilder::new("main.wgsl", Some(&["THREE_DIMENTIONAL".into()]))
	.unwrap()
	.build();
```
The output will then be identical to:
```wgsl
struct Struct { data: vec3<f32>, }

struct OtherStruct { data: vec3<f32>, }
```


# Crate features

### Inserting Arrays of Suitable Lengths as Vectors

By default, none of the following features are enabled.
* **array_vectors** -
  When enabled, implementations of [`WGSLType`] are compiled for all array types of suitable lengths and scalar types.
  This feature forces the translation of (for example) `[f32; 4]` to the WGSL type `vec4<f32>` in methods like [`ShaderBuilder::put_array_definition`].
* **cgmath_vectors** -
  This feature is similar to **array_vectors** but with [`cgmath`] vector objects like [`cgmath::Vector3<u32>`]
  which would be translated to `vec3<u32>`.
*/
use std::{any, borrow, collections::HashMap, path, vec};

use wgpu::BindGroupLayoutDescriptor;

const INSTRUCTION_PREFIX: &str = "//!";
const INCLUDE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "include");
const DEFINE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "define");
const IFDEF_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "ifdef");
const IFNDEF_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "ifndef");
const ELSE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "else");
const ENDIF_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "endif");
lazy_static::lazy_static! {
	static ref MACRO_REGEX: regex::Regex = regex::Regex::new(&format!(r"{DEFINE_INSTRUCTION} (\S+) (.+)")).unwrap();
}

/// Type for data types that can be defined in WGSL.
/// [`WGSLType`] is already implemented for some primitive types.
pub trait WGSLType {
	/// Returns the name of the type in WGSL syntax.
	fn type_name() -> String;

	/// Returns a string that creates an instance of the type in WGSL syntax.
	fn string_definition(&self) -> String;
}

impl WGSLType for u32 {
	fn type_name() -> String {
		any::type_name::<u32>().to_string()
	}

	fn string_definition(&self) -> String {
		format!("{self}u")
	}
}

#[duplicate::duplicate_item(wgsl_type; [i32]; [f32])]
impl WGSLType for wgsl_type {
	fn type_name() -> String {
		any::type_name::<wgsl_type>().to_string()
	}

	fn string_definition(&self) -> String {
		format!("{self}")
	}
}

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
	/// It is marked public for debugging purposes.
	pub source_string: String,
	source_path: String,
}

impl ShaderBuilder {
	/// Creates a new [`ShaderBuilder`].
	///
	/// # Arguments
	/// - `source_path` - Path to the root WGSL module.
	///		All includes will be relative to the parent directory of the root WGSL module.
	/// 	Code is generated recursively with attention to `include` and `define` statements.
	/// 	See "Examples" for more details on include and macro functionality.
	/// - `defines` - list of strings which indicate the valid defines in wgsl
	pub fn new(source_path: &str, defines: Option<&[String]>) -> Result<Self, ex::io::Error> {
		let module_path = path::Path::new(&source_path);
		let source_string = Self::load_shader_module(
			module_path.parent().unwrap_or(path::Path::new("./")),
			module_path,
			defines,
		)?
		.0;
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

		for value in array.iter() {
			string_definition.push_str(&value.string_definition());
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

	pub fn bind_groups_from_layouts(&mut self, layouts: &[(BindGroupLayoutDescriptor, Vec<(String, String)>)]) -> &mut Self {
		for (i, layout) in layouts.iter().enumerate() {
			self.bind_group_from_layout(i as u32, &layout.0, &layout.1);
		}

		self
	}

	pub fn bind_group_from_layout(&mut self, bind_group_id: u32, layout: &BindGroupLayoutDescriptor, names_types: &Vec<(String, String)>) -> &mut Self {

		let mut bind_group_strings: Vec<String> = vec![];

		for (i, entry) in layout.entries.iter().enumerate() {
			let bin = entry.binding;
			let arr = entry.count.is_some();

			let ty = match entry.ty {
				wgpu::BindingType::Buffer { 
					ty, 
					has_dynamic_offset: _,
					min_binding_size: _
				} => {
					let ty = match ty {
						wgpu::BufferBindingType::Uniform => "<uniform>",
						wgpu::BufferBindingType::Storage { read_only } => {
							if read_only {
								"<storage, read>"
							}
							else {
								"<storage, read_write>"
							}
						}
					};


					(ty, names_types[i].1.clone())
				}

				wgpu::BindingType::Sampler(_) => {
					("", "sampler".into())
				}

				wgpu::BindingType::StorageTexture { 
					access, 
					format, 
					view_dimension: _
				} => {
					let format = get_texture_format(format);

					let access = match access {
						wgpu::StorageTextureAccess::ReadOnly => "read",
						wgpu::StorageTextureAccess::WriteOnly => "write",
						wgpu::StorageTextureAccess::ReadWrite => "read_write",
					};


					("", format!("texture_storage_2d<{format}, {access}>"))
				}

				wgpu::BindingType::Texture { 
					sample_type, 
					view_dimension: _, 
					multisampled: _ 
				} => {
					use wgpu::TextureSampleType::*;
					let ty = match sample_type {
						Float {
							filterable: _
						} => "texture_2d<f32>",
						Depth => "texture_depth_2d",
						Sint => "texture_2d<i32>",
						Uint => "texture_2d<u32>",
					};

					("", ty.into())
				}
			};

			if arr {
				todo!("array support")
			}
			bind_group_strings.push(format!("@group({bind_group_id}) @binding({bin})\n"));
			bind_group_strings.push(
				["var", ty.0, " ", names_types[i].0.as_str(), ": ", ty.1.as_str(), ";\n"].concat()
			);
		}
		let group = bind_group_strings.concat();

		self.source_string = [group, self.source_string.clone()].concat();
		self
	}

	/// parses 
	/// 
	/// `//!ifdef SOME_DEFINE`
	/// `//!ifndef SOME_DEFINE`
	/// `//!else`
	/// `//!endif`
	///
	/// similar to glsl
	///
	/// # Arguments
	/// - `defines` - list of strings which equal the defines in wgsl
	fn parse_defines(source_string: &mut String, defines: &[String]) {
		let lines = source_string.lines();
		let mut fin: Vec<String> = vec![];

		let mut add = true;

		for line in lines {
			let line = line.trim();

			if line.starts_with(ELSE_INSTRUCTION) {
				add = !add;
				continue;
			} else if line.starts_with(ENDIF_INSTRUCTION) {
				add = true;
				continue;
			}

			if line.starts_with(IFDEF_INSTRUCTION) {
				let def = defines.contains(&line.split(' ').nth(1).unwrap().into());

				add = def;
				continue;
			} else if line.starts_with(IFNDEF_INSTRUCTION) {
				let def = !defines.contains(&line.split(' ').nth(1).unwrap().into());

				add = def;
				continue;
			}

			if add {
				fin.push(line.to_owned() + if line == "\n" || line == "" { "" } else { "\n" });
			}
		}

		*source_string = fin.concat();
	}

	fn load_shader_module(
		base_path: &path::Path,
		module_path: &path::Path,
		defines: Option<&[String]>,
	) -> Result<(String, HashMap<String, String>), ex::io::Error> {
		let mut module_source = ex::fs::read_to_string(module_path)?;
		if let Some(defines) = defines {
			Self::parse_defines(&mut module_source, defines);
		}
		let mut module_string = String::new();
		let mut definitions: HashMap<String, String> = HashMap::new();
		for line in module_source.lines() {
			if line.starts_with(INCLUDE_INSTRUCTION) {
				for include in line.split_whitespace().skip(1) {
					let (included_module_string, included_definitions) =
						Self::load_shader_module(base_path, &path::Path::new(include), defines)?;
					module_string.push_str(&included_module_string);
					definitions.extend(included_definitions);
				}
			} else if let Some(captures) = MACRO_REGEX.captures(line) {
				definitions.insert(captures[1].to_string(), captures[2].to_string());
			} else {
				module_string.push_str(line);
				module_string.push('\n');
			}
		}
		definitions.iter().for_each(|(name, value)| {
			module_string = module_string.replace(name, value);
		});
		Ok((module_string, definitions))
	}
}

fn get_texture_format(format: wgpu::TextureFormat) -> &'static str {
	use wgpu::TextureFormat::*;
	match format  {
		R8Unorm | R8Snorm | Rg8Snorm | Rg8Unorm |
		Rgba8Unorm | Rgba8UnormSrgb | Bgra8Unorm |
		Bgra8UnormSrgb | Etc2Rgb8Unorm | Etc2Rgb8UnormSrgb |
		Etc2Rgba8Unorm | Etc2Rgba8UnormSrgb | Rgba8Snorm |
		R16Float | Rg16Float | Rgba16Float | R32Float |
		Rg32Float | Rgba32Float | Depth32Float | Depth32FloatStencil8 => {
			"f32"
		} 

		R8Uint | Rg8Uint | Rgba8Uint | R16Uint | Rg16Uint |
		Rgba16Uint | R32Uint | Rg32Uint | Rgba32Uint => {
			"u32"
		}

		R8Sint | Rg8Sint | Rgba8Sint | R16Sint | Rg16Sint |
		Rgba16Sint | R32Sint | Rg32Sint | Rgba32Sint => {
			"i32"
		}

		_ => panic!("not supported")
	}
} 

#[cfg(test)]
mod tests {
	use crate::{ShaderBuilder, WGSLType};
	use std::{collections::HashMap, io};

	#[test]
	fn bind_group_gen() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/bind_group_gen.wgsl", None)
				.unwrap()
				.bind_group_from_layout(0, &wgpu::BindGroupLayoutDescriptor {label: Some("layout_desc"), entries: &[wgpu::BindGroupLayoutEntry {binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering) , count: None}, 
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::VERTEX,
					ty: wgpu::BindingType::Buffer { 
						ty: wgpu::BufferBindingType::Uniform, 
						has_dynamic_offset: false, 
						min_binding_size: None },
					count: None,
				}]}, &vec![("some_sampler".into(), "sampler".into()), ("some_val".into(), "vec3<f32>".into())])
				.source_string,
			ShaderBuilder::new("test_shaders/bind_group_gen_result.wgsl", None)
				.unwrap()
				.source_string
		)
	}

  #[test]
	fn conditional_compilation() {
		assert_eq!(
			ShaderBuilder::new(
				"test_shaders/conditional_compilation.wgsl",
				Some(&["THREE_DIMENTIONAL".into()])
			)
			.unwrap()
			.source_string,
			ShaderBuilder::new("test_shaders/conditional_compilation_processed.wgsl", None)
				.unwrap()
				.source_string
		)
	}

	#[test]
	fn nonexistent() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/nonexistent.wgsl", None)
				.err()
				.unwrap()
				.kind(),
			io::ErrorKind::NotFound
		);
	}

	#[test]
	fn standard_include() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/includer.wgsl", None)
				.unwrap()
				.source_string,
			ShaderBuilder::new("test_shaders/included.wgsl", None)
				.unwrap()
				.source_string
		);
	}

	#[test]
	fn missing_include() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/missing_include.wgsl", None)
				.err()
				.unwrap()
				.kind(),
			io::ErrorKind::NotFound
		);
	}

	#[test]
	fn nested_include() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/nested_include.wgsl", None)
				.unwrap()
				.source_string,
			ShaderBuilder::new("test_shaders/includer.wgsl", None)
				.unwrap()
				.source_string
		)
	}

	#[test]
	fn multiple_includes() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/multiple_includes.wgsl", None)
				.unwrap()
				.source_string,
			format!(
				"{}{}",
				ShaderBuilder::new("test_shaders/included.wgsl", None)
					.unwrap()
					.source_string,
				ShaderBuilder::new("test_shaders/included2.wgsl", None)
					.unwrap()
					.source_string
			)
		)
	}

	#[test]
	fn multiple_inline_includes() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/multiple_inline_includes.wgsl", None)
				.unwrap()
				.source_string,
			ShaderBuilder::new("test_shaders/multiple_includes.wgsl", None)
				.unwrap()
				.source_string
		)
	}

	#[test]
	fn define() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/definer.wgsl", None)
				.unwrap()
				.source_string,
			ShaderBuilder::new("test_shaders/defined.wgsl", None)
				.unwrap()
				.source_string
		)
	}

	#[test]
	fn include_define() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/included_define.wgsl", None)
				.unwrap()
				.source_string,
			ShaderBuilder::new("test_shaders/included_define_processed.wgsl", None)
				.unwrap()
				.source_string,
		)
	}

	#[test]
	fn define_with_spaces() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/define_with_spaces.wgsl", None)
				.unwrap()
				.source_string,
			ShaderBuilder::new("test_shaders/define_with_spaces_processed.wgsl", None)
				.unwrap()
				.source_string,
		)
	}

	#[test]
	fn put_constant() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/set_constants.wgsl", None)
				.unwrap()
				.put_constant("ONE", 1u32)
				.put_constant("TWO", 2u32)
				.source_string,
			ShaderBuilder::new("test_shaders/set_constants_processed.wgsl", None)
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
			ShaderBuilder::new("test_shaders/set_constants.wgsl", None)
				.unwrap()
				.put_constant_map(&constants)
				.source_string,
			ShaderBuilder::new("test_shaders/set_constants_processed.wgsl", None)
				.unwrap()
				.source_string
		)
	}

	#[test]
	fn load_proper_label() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/included.wgsl", None)
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
			ShaderBuilder::new("test_shaders/put_array_definition_bools.wgsl", None)
				.unwrap()
				.put_array_definition("BOOL_ARRAY", &vec![&true, &false])
				.source_string,
			ShaderBuilder::new(
				"test_shaders/put_array_definition_bools_processed.wgsl",
				None
			)
			.unwrap()
			.source_string
		)
	}

	#[test]
	fn put_array_definition_scalar() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_scalars.wgsl", None)
				.unwrap()
				.put_array_definition("SCALAR_ARRAY", &vec![&1, &0])
				.source_string,
			ShaderBuilder::new(
				"test_shaders/put_array_definition_scalars_processed.wgsl",
				None
			)
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
			ShaderBuilder::new("test_shaders/put_array_definition_structs.wgsl", None)
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
			ShaderBuilder::new(
				"test_shaders/put_array_definition_structs_processed.wgsl",
				None
			)
			.unwrap()
			.source_string
		)
	}

	#[cfg(feature = "array_vectors")]
	#[test]
	fn put_array_definition_array_vectors() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_vectors.wgsl", None)
				.unwrap()
				.put_array_definition(
					"VECTOR_ARRAY",
					&vec![&[1.0, 2.0, 3.0, 4.0], &[1.5, 2.1, 3.7, 4.9]]
				)
				.source_string,
			ShaderBuilder::new(
				"test_shaders/put_array_definition_vectors_processed.wgsl",
				None
			)
			.unwrap()
			.source_string
		)
	}

	#[cfg(feature = "cgmath_vectors")]
	#[test]
	fn put_array_definition_cgmath_vectors() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_vectors.wgsl", None)
				.unwrap()
				.put_array_definition(
					"VECTOR_ARRAY",
					&vec![
						&cgmath::Vector4::<f32>::new(1.0, 2.0, 3.0, 4.0),
						&cgmath::Vector4::<f32>::new(1.5, 2.1, 3.7, 4.9)
					]
				)
				.source_string,
			ShaderBuilder::new(
				"test_shaders/put_array_definition_vectors_processed.wgsl",
				None
			)
			.unwrap()
			.source_string
		)
	}
}
