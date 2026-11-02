//! The quad shader must parse and validate (the only check possible without a GPU).
#[test]
fn the_quad_shader_is_valid_wgsl() {
    let src = include_str!("../src/shader.wgsl");
    let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
}
