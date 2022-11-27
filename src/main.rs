use wgsl_preprocessor::ShaderBuilder;

fn main() {
    println!("{}", 			ShaderBuilder::new("test_shaders/bind_group_gen.wgsl")
    .unwrap()
    .bind_group_from_layout(0, &wgpu::BindGroupLayoutDescriptor {label: Some("layout_desc"), entries: &[wgpu::BindGroupLayoutEntry {binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering) , count: None}]}, vec![("some_val".into(), "vec3<f32>".into())])
    .source_string)
}