//!define OUTER
//!ifdef OUTER
//!ifdef INNER
struct InnerBad {
    vector: vec4<f32>;
};
//!else
struct Inner {
    vector: vec4<f32>;
};
//!endif
//!endif