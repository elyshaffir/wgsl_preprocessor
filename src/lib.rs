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
ShaderBuilder::new("main.wgsl")
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
use core::str;
use std::{
	any,
	borrow::{self},
	collections::{HashMap, LinkedList},
	io,
};

const INSTRUCTION_PREFIX: &str = "//!";
const INCLUDE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "include");
const DEFINE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "define");
const UNDEF_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "undef");
const IFDEF_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "ifdef");
const IFNDEF_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "ifndef");
const ELSE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "else");
const ENDIF_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "endif");
lazy_static::lazy_static! {
	static ref DEFINE_REGEX: regex::Regex = regex::Regex::new(&format!(r"{DEFINE_INSTRUCTION} (\S+) (.+)")).unwrap();
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
	source_path: String,
	definitions: HashMap<String, Option<String>>,
}

impl ShaderBuilder {
	/// Creates a new [`ShaderBuilder`].
	///
	/// # Arguments
	/// - `source_path` - Path to the root WGSL module.
	///		All includes will be relative to the parent directory of the root WGSL module.
	/// 	Code is generated recursively with attention to `include` and `define` statements.
	/// 	See "Examples" for more details on include and macro functionality.
	pub fn new(source_path: &str) -> Self {
		Self {
			source_path: source_path.to_string(),
			definitions: HashMap::new(),
		}
	}

	/// Performs the WGSL's parallel to C's `#define` statement for a definition later used
	/// in conditional compilation.
	/// # Arguments
	/// - `name` - Name of the definition.
	pub fn define(&mut self, name: &str) -> &mut Self {
		self.definitions.insert(name.to_string(), None);
		self
	}

	/// Performs the WGSL's parallel to C's `#define` statement for a constant with a value.
	///
	/// # Arguments
	/// - `name` - Name of the constant; the string to replace in the code.
	/// - `value` - Value of the constant.
	pub fn put_constant(&mut self, name: &str, value: impl WGSLType) -> &mut Self {
		self.definitions
			.insert(name.to_string(), Some(value.string_definition()));
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

		self.definitions
			.insert(name.to_string(), Some(string_definition));
		self
	}

	// todo how to make this method invalidate the object
	/// Generates a WGSL source from all previous calls on this [`ShaderBuilder`].
	pub fn build_source(&mut self) -> Result<String, io::Error> {
		self.load_shader_module(&self.source_path.clone())
	}

	/// Builds a [`wgpu::ShaderModuleDescriptor`] from the shader.
	/// The `label` member of the built [`wgpu::ShaderModuleDescriptor`] is the name of the shader file without the postfix.
	pub fn build(&mut self) -> Result<wgpu::ShaderModuleDescriptor, io::Error> {
		let source_string = self.build_source()?;
		Ok(wgpu::ShaderModuleDescriptor {
			label: Some(
				&self
					.source_path
					.rsplit(['/', '.'])
					.nth(1)
					.unwrap_or(&self.source_path),
			),
			source: wgpu::ShaderSource::Wgsl(borrow::Cow::Owned(source_string)),
		})
	}

	// todo run tests for all feature configurations
	// todo add doctests about all new features
	// todo mention/demonstrate new features in the readme
	fn load_shader_module(&mut self, module_path: &str) -> Result<String, io::Error> {
		let module_source = std::fs::read_to_string(module_path)?;
		let mut module_string = String::new();
		let mut defined_conditions: LinkedList<(&str, bool)> = LinkedList::new();
		for line in module_source.lines() {
			if line.starts_with(ENDIF_INSTRUCTION) {
				if let None = defined_conditions.pop_back() {
					return Err(io::Error::other("Unexpected endif statement"));
				}
				continue;
			} else if line == ELSE_INSTRUCTION {
				if let Some(condition) = defined_conditions.pop_back() {
					defined_conditions.push_back((condition.0, !condition.1));
				} else {
					return Err(io::Error::other("Unexpected else statement"));
				}
				continue;
			} else if line.starts_with(UNDEF_INSTRUCTION) {
				let undefs: Vec<&str> = line.split_whitespace().skip(1).collect();
				if undefs.len() != 1 {
					return Err(io::Error::other(
						"Invalid number of keywords after undef statement",
					));
				}
				if let None = self.definitions.remove(undefs[0]) {
					return Err(io::Error::other("Attempt to undef an undefined symbol"));
				}
				continue;
			}
			let relevant = defined_conditions.iter().all(|&(name, should_be_defined)| {
				(should_be_defined && self.definitions.contains_key(name))
					|| (!should_be_defined && !self.definitions.contains_key(name))
			});
			if !relevant {
				continue;
			}
			if line.starts_with(IFDEF_INSTRUCTION) || line.starts_with(IFNDEF_INSTRUCTION) {
				let conditions: Vec<&str> = line.split_whitespace().skip(1).collect();
				if conditions.len() != 1 {
					return Err(io::Error::other(
						"Invalid number of keywords after ifdef/ifndef statement",
					));
				}
				defined_conditions.push_back((conditions[0], line.starts_with(IFDEF_INSTRUCTION)));
			} else if line.starts_with(INCLUDE_INSTRUCTION) {
				for include in line.split_whitespace().skip(1) {
					let included_module_string = self.load_shader_module(include)?;
					module_string.push_str(&included_module_string);
				}
			} else if let Some(captures) = DEFINE_REGEX.captures(line) {
				self.definitions
					.insert(captures[1].to_string(), Some(captures[2].to_string()));
			} else if line.starts_with(DEFINE_INSTRUCTION) {
				let defines: Vec<&str> = line.split_whitespace().skip(1).collect();
				if defines.len() != 1 {
					return Err(io::Error::other(
						"Invalid number of keywords after define statement",
					));
				}

				if !self.definitions.contains_key(defines[0]) {
					self.define(defines[0]);
				} else {
					module_string.push_str(defines[0]);
					module_string.push('\n');
				}
			} else {
				module_string.push_str(line);
				module_string.push('\n');
			}
		}
		self.definitions.iter().for_each(|(name, value)| {
			if let Some(replacement) = value {
				module_string = module_string.replace(name, replacement);
			}
		});
		if !defined_conditions.is_empty() {
			return Err(io::Error::new(
				io::ErrorKind::UnexpectedEof,
				"Module Missing endif statements",
			));
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
				.build_source()
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
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/included.wgsl")
				.build_source()
				.unwrap()
		);
	}

	#[test]
	fn missing_include() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/missing_include.wgsl")
				.build_source()
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
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/includer.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn multiple_includes() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/multiple_includes.wgsl")
				.build_source()
				.unwrap(),
			format!(
				"{}{}",
				ShaderBuilder::new("test_shaders/included.wgsl")
					.build_source()
					.unwrap(),
				ShaderBuilder::new("test_shaders/included2.wgsl")
					.build_source()
					.unwrap()
			)
		)
	}

	#[test]
	fn multiple_inline_includes() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/multiple_inline_includes.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/multiple_includes.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn define() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/definer.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/defined.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn included_define() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/included_define.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/included_define_processed.wgsl")
				.build_source()
				.unwrap(),
		)
	}

	#[test]
	fn define_with_spaces() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/define_with_spaces.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/define_with_spaces_processed.wgsl")
				.build_source()
				.unwrap(),
		)
	}

	#[test]
	fn put_constant() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/set_constants.wgsl")
				.put_constant("ONE", 1u32)
				.put_constant("TWO", 2u32)
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/set_constants_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn put_constant_map() {
		let mut constants = HashMap::new();
		constants.insert("ONE", 1u32);
		constants.insert("TWO", 2u32);
		assert_eq!(
			ShaderBuilder::new("test_shaders/set_constants.wgsl")
				.put_constant_map(&constants)
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/set_constants_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn load_proper_label() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/included.wgsl")
				.build()
				.unwrap()
				.label
				.unwrap(),
			"included"
		);
	}

	#[test]
	fn put_array_definition_bools() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_bools.wgsl")
				.put_array_definition("BOOL_ARRAY", &vec![&true, &false])
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/put_array_definition_bools_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn put_array_definition_scalar() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_scalars.wgsl")
				.put_array_definition("SCALAR_ARRAY", &vec![&1, &0])
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/put_array_definition_scalars_processed.wgsl")
				.build_source()
				.unwrap()
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
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/put_array_definition_structs_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[cfg(feature = "array_vectors")]
	#[test]
	fn put_array_definition_array_vectors() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_vectors.wgsl")
				.put_array_definition(
					"VECTOR_ARRAY",
					&vec![&[1.0, 2.0, 3.0, 4.0], &[1.5, 2.1, 3.7, 4.9]]
				)
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/put_array_definition_vectors_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[cfg(feature = "cgmath_vectors")]
	#[test]
	fn put_array_definition_cgmath_vectors() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/put_array_definition_vectors.wgsl")
				.put_array_definition(
					"VECTOR_ARRAY",
					&vec![
						&cgmath::Vector4::<f32>::new(1.0, 2.0, 3.0, 4.0),
						&cgmath::Vector4::<f32>::new(1.5, 2.1, 3.7, 4.9)
					]
				)
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/put_array_definition_vectors_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn ifdef() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/ifdef.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/ifdef_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn ifdef_else() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/ifdef_else.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/ifdef_else_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn ifdef_else_else_branch() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/ifdef_else_else_branch.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/ifdef_else_else_branch_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn ifdef_nested() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/ifdef_nested.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/ifdef_nested_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn ifdef_with_include() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/ifdef_with_include.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/included.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn ifdef_no_endif() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/ifdef_no_endif.wgsl")
				.build_source()
				.err()
				.unwrap()
				.kind(),
			io::ErrorKind::UnexpectedEof
		);
	}

	#[test]
	fn ifndef() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/ifndef.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/ifndef_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn ifndef_undefined() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/ifndef_undefined.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/ifndef_undefined_processed.wgsl")
				.build_source()
				.unwrap()
		)
	}

	#[test]
	fn ifdef_included_define() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/ifdef_included_define.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/ifdef_included_define_processed.wgsl")
				.build_source()
				.unwrap(),
		)
	}

	#[test]
	fn ifdef_undef() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/ifdef_undef.wgsl")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/ifdef_undef_processed.wgsl")
				.build_source()
				.unwrap(),
		)
	}

	#[test]
	fn ifdef_from_outside() {
		assert_eq!(
			ShaderBuilder::new("test_shaders/ifdef_from_outside.wgsl")
				.define("DEFINE_VECTOR")
				.build_source()
				.unwrap(),
			ShaderBuilder::new("test_shaders/ifdef_from_outside_processed.wgsl")
				.build_source()
				.unwrap(),
		)
	}
}
