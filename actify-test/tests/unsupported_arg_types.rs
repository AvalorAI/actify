/// Tests that the actify macro rejects invalid inputs with clear error messages.
///
/// If a compile_error! is accidentally removed, these tests will fail because the test file
/// will suddenly compile when it shouldn't.
#[test]
fn compile_fail_tests() {
    // The .stderr files match the exact diagnostics of one rustc version, so
    // every CI job must choose: the trybuild job, pinned to that version, sets
    // TRYBUILD_TESTS=1 and runs these; the test matrix on unpinned stable sets
    // TRYBUILD_TESTS=0. An unset variable on CI fails rather than skips, so the
    // suite cannot go dark through a lost job or variable. Locally the tests
    // always run. See CONTRIBUTING.md for how to regenerate the .stderr files.
    if std::env::var_os("CI").is_some() {
        match std::env::var("TRYBUILD_TESTS").as_deref() {
            Ok("1") => {}
            Ok("0") => {
                eprintln!("skipping trybuild tests: TRYBUILD_TESTS=0");
                return;
            }
            _ => panic!("CI is set but TRYBUILD_TESTS is not: set 1 to run or 0 to skip"),
        }
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
    t.compile_fail("tests/compile_fail/superfluous_skip_broadcast_ref_self.rs");
    t.compile_fail("tests/compile_fail/superfluous_broadcast.rs");
    t.compile_fail("tests/compile_fail/unnecessary_block_broadcast.rs");

    // Skipped methods
    t.compile_fail("tests/compile_fail/skipped_method_not_on_handle.rs");
    t.compile_fail("tests/compile_fail/superfluous_broadcast_on_skip.rs");

    // Invalid custom name
    t.compile_fail("tests/compile_fail/invalid_custom_name.rs");

    // Error reporting quality
    t.compile_fail("tests/compile_fail/multiple_errors.rs");
    t.compile_fail("tests/compile_fail/error_does_not_cascade.rs");
}
