/// Tests that the actify macro rejects invalid inputs with clear error messages.
///
/// If a compile_error! is accidentally removed, these tests will fail because the test file
/// will suddenly compile when it shouldn't.
#[test]
fn compile_fail_tests() {
    let t = trybuild::TestCases::new();

    // Argument type validation
    t.compile_fail("tests/compile_fail/reference_arg.rs");
    t.compile_fail("tests/compile_fail/raw_pointer_arg.rs");
    t.compile_fail("tests/compile_fail/impl_trait_arg.rs");
    t.compile_fail("tests/compile_fail/unsupported_arg_type.rs");

    // Method validation
    t.compile_fail("tests/compile_fail/static_method.rs");

    // Superfluous broadcast attributes
    t.compile_fail("tests/compile_fail/superfluous_skip_broadcast.rs");
    t.compile_fail("tests/compile_fail/superfluous_broadcast.rs");
}
