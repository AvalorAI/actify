# Contributing

## Building and testing

The MSRV is 1.85. CI runs the following, and all of it must pass locally before
pushing:

```sh
cargo test --workspace
cargo test -p actify                                                  # default features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

Workspace feature unification builds actify with the `profiler` feature whenever
`actify-test` is in the graph, so `cargo test -p actify` is the only run that
covers the default build.

Documentation is checked twice because feature-gated items cannot be linked from
text that is always compiled.

The MSRV is declared in three places that must agree: `rust-version` in
`[workspace.package]`, the `MSRV` variable in `ci.yml`, and the line above.

## Pinned actions

Every action in `ci.yml` and `release.yml` is pinned to a commit SHA, with the
ref it came from in a trailing comment. A tag can be moved to a different commit,
so a tag is not a pin.

To move one, resolve the ref and replace both the SHA and the comment:

```sh
gh api repos/actions/checkout/git/ref/tags/v5 --jq '.object.sha'
gh api repos/dtolnay/rust-toolchain/branches/master --jq '.commit.sha'
```

`dtolnay/rust-toolchain` takes its default toolchain from the branch the ref
points at, so pinning by SHA would hide which toolchain a job installs. The two
jobs that do not want plain stable use the `master` SHA and pass `toolchain`
explicitly, from `MSRV` and `TRYBUILD_TOOLCHAIN`.

## Compile-fail snapshots

`actify-test/tests/compile_fail/` holds trybuild cases with committed `.stderr`
files. They are skipped unless `TRYBUILD_TESTS` is set, because they assert exact
rustc diagnostics and only match the toolchain in `TRYBUILD_TOOLCHAIN`
(`.github/workflows/ci.yml`).

Run them, and regenerate the snapshots after changing a macro diagnostic:

```sh
TRYBUILD_TESTS=1 cargo test -p actify-test --test unsupported_arg_types
TRYBUILD=overwrite TRYBUILD_TESTS=1 cargo test -p actify-test --test unsupported_arg_types
```

Regenerate with the pinned toolchain, otherwise the committed output will not
match what CI produces. The pin tracks the version contributors develop on, for
the same reason: a snapshot that quotes a rustc diagnostic rather than one of
actify's own `compile_error!` messages can only match one toolchain at a time.
`skipped_method_not_on_handle.stderr` is such a case, since the absence of a
generated method can only be shown by rustc's own "no method named" error.

## Releasing

Publishing runs on GitHub Actions (`.github/workflows/release.yml`), triggered by
a version tag. Local publishing is not part of the process.

1. Update `CHANGELOG.md`.
2. Bump `actify/Cargo.toml`. Bump `actify-macros/Cargo.toml` if the macro crate
   changed, and update actify's dependency requirement to match.
3. Merge to `main`.
4. Tag the merge commit and push it:

   ```sh
   git tag v0.8.3
   git push origin v0.8.3
   ```

The workflow asserts that the tag matches actify's version and that actify's
requirement on actify-macros matches the workspace, runs the full CI gate, then
publishes actify-macros followed by actify. actify-macros is skipped when that
version is already on crates.io.

Publishing is irreversible. crates.io allows yanking, never deletion or version
reuse.

### One-time setup

The `publish` job authenticates through crates.io trusted publishing, so no token
is stored in the repository. Both crates need a trusted publisher registered at
`https://crates.io/crates/<crate>/settings`, with:

| Field | Value |
| --- | --- |
| Repository owner | `AvalorAI` |
| Repository name | `actify` |
| Workflow filename | `release.yml` |
| Environment | `release` |

The `release` GitHub environment gates the publish step. Add required reviewers to
it to approve each release before it is pushed to crates.io.
