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
After building `main.wgsl`, it's compiled contents would be identical to:
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

# Example: Defining a Constant Struct Array

Let's say some color constants are calculated before shader compile time and should be injected into the
code for performance reasons.
`main.wgsl` would contain:
```wgsl
//!define STRUCT_ARRAY
```
In the Rust code, `Struct` is defined, and given an implementation of [`WGSLType`] it can be translated to
a WGSL struct with a single `vec4<f32>` member named `data`.
The Rust code building and compiling the shaders will contain:
```ignore
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
```ignore
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
	.build()
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
use std::{any, borrow, collections::HashMap, path};

const INSTRUCTION_PREFIX: &str = "//!";
const INCLUDE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "include");
const DEFINE_INSTRUCTION: &str = const_format::concatcp!(INSTRUCTION_PREFIX, "define");

/// Type for data types that can be defined in WGSL.
/// [`WGSLType`] is already implemented for some primitive types.
pub trait WGSLType {
	/// Returns the name of the type in WGSL syntax.
	fn type_name() -> String;

	/// Returns a string that defines the type in WGSL syntax.
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
				module_string.push('\n');
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
				.source_string,
			ShaderBuilder::new("test_shaders/put_array_definition_vectors_processed.wgsl")
				.unwrap()
				.source_string
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
