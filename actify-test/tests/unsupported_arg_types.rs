/// Tests that the actify macro rejects unsupported argument types with clear error messages.
///
/// If a compile_error! is accidentally removed, these tests will fail because the test file
/// will suddenly compile when it shouldn't.
#[test]
fn compile_fail_tests() {
    let t = trybuild::TestCases::new();

    t.compile_fail("tests/compile_fail/reference_arg.rs");
    t.compile_fail("tests/compile_fail/raw_pointer_arg.rs");
    t.compile_fail("tests/compile_fail/impl_trait_arg.rs");
}
