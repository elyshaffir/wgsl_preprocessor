//!include test_shaders/ifdef_included_definer.wgsl
//!ifdef DEFINED
struct DefinedIncluded {
    value: u32;
};
//!endif