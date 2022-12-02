
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
