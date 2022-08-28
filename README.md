This crate provides a library for performing similar actions to what is expected from a preprocessor in WGSL.
Since WGSL [will not have a preprocessor](https://github.com/gpuweb/gpuweb/issues/568) at least for version 1.0,
this crate provides solutions to some common problems like including shader files and defining constants from
Rust code.

### Example: Include Multiple Shader Files

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

### Example: Define Macros

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

### Example: Defining a Constant Struct Array

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
```rust
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
```rust
use wgsl_preprocessor::ShaderBuilder;

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

### Crate features

#### Inserting Arrays of Suitable Lengths as Vectors

By default, none of the following features are enabled.
* **array_vectors** -
  When enabled, implementations of [`WGSLType`] are compiled for all array types of suitable lengths and scalar types.
  This feature forces the translation of (for example) `[f32; 4]` to the WGSL type `vec4<f32>` in methods like [`ShaderBuilder::put_array_definition`].
* **cgmath_vectors** -
  This feature is similar to **array_vectors** but with [`cgmath`] vector objects like [`cgmath::Vector3<u32>`]
  which would be translated to `vec3<u32>`.