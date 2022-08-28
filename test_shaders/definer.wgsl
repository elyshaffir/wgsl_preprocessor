//!define u3 vec3<u32>
@compute
@workgroup_size(64)
fn main(@builtin(global_invocation_id) id: u3) {
	// ...
}
