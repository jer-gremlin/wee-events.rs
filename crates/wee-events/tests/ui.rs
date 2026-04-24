#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/unregistered_command.rs");
    t.compile_fail("tests/ui/missing_has_command.rs");
    t.pass("tests/ui/definition_only_compiles.rs");
}
