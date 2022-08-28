@compute
@workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
	// ...
}
