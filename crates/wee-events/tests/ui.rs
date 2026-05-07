#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/unregistered_command.rs");
    t.compile_fail("tests/ui/missing_has_command.rs");
    t.compile_fail("tests/ui/service_name_collision.rs");
    t.compile_fail("tests/ui/service_macro_rejects_any_effect_filter.rs");
    t.compile_fail("tests/ui/service_macro_rejects_trailing_fields.rs");
    t.compile_fail("tests/ui/service_macro_rejects_malformed_predicate.rs");
    t.pass("tests/ui/definition_only_compiles.rs");
}
