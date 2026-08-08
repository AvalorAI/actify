/// Tests that the actify macro rejects invalid inputs with clear error messages.
///
/// If a compile_error! is accidentally removed, these tests will fail because the test file
/// will suddenly compile when it shouldn't.
#[test]
fn compile_fail_tests() {
    // The .stderr files match the exact diagnostics of one rustc version. On CI,
    // only the dedicated trybuild job (pinned to that version) runs these tests;
    // the regular test matrix on unpinned stable skips them. Locally they always
    // run. See CONTRIBUTING.md for how to regenerate the .stderr files.
    if std::env::var_os("CI").is_some() && std::env::var_os("TRYBUILD_TESTS").is_none() {
        eprintln!("skipping trybuild tests: CI is set and TRYBUILD_TESTS is not");
        return;
    }

    let t = trybuild::TestCases::new();

    // Argument type validation
    t.compile_fail("tests/compile_fail/reference_arg.rs");
    t.compile_fail("tests/compile_fail/raw_pointer_arg.rs");
    t.compile_fail("tests/compile_fail/impl_trait_arg.rs");
    t.compile_fail("tests/compile_fail/unsupported_arg_type.rs");

    // Return type validation
    t.compile_fail("tests/compile_fail/reference_return.rs");
    t.compile_fail("tests/compile_fail/impl_trait_return.rs");

    // Method validation
    t.compile_fail("tests/compile_fail/static_method.rs");
    t.compile_fail("tests/compile_fail/unsafe_method.rs");
    t.compile_fail("tests/compile_fail/by_value_self.rs");

    // Superfluous broadcast attributes
    t.compile_fail("tests/compile_fail/superfluous_skip_broadcast.rs");
    t.compile_fail("tests/compile_fail/superfluous_broadcast.rs");
    t.compile_fail("tests/compile_fail/unnecessary_block_broadcast.rs");

    // Invalid custom name
    t.compile_fail("tests/compile_fail/invalid_custom_name.rs");

    // Error reporting quality
    t.compile_fail("tests/compile_fail/multiple_errors.rs");
    t.compile_fail("tests/compile_fail/error_does_not_cascade.rs");
}
