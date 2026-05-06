#[test]
fn attach_to_api() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/attach_to_no_effects_ok.rs");
    t.pass("tests/ui/attach_to_effectful_ok.rs");
    t.pass("tests/ui/attach_to_multiple_effects_ok.rs");
    t.compile_fail("tests/ui/attach_to_missing_effect.rs");
    t.compile_fail("tests/ui/attach_to_wrong_effect.rs");
    t.compile_fail("tests/ui/attach_to_duplicate_effect.rs");
    t.compile_fail("tests/ui/attach_to_missing_store.rs");
    t.compile_fail("tests/ui/attach_to_missing_env.rs");
    t.compile_fail("tests/ui/attach_to_raw_restate_bypass.rs");
    t.compile_fail("tests/ui/attach_to_definition_bind_bypass.rs");
    t.compile_fail("tests/ui/attach_to_direct_binding_bypass.rs");
}
