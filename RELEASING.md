# Releasing `txtfp`

Frozen procedure for cutting a new release. Follow it top-to-bottom; no
step is optional. Each step has a verification you can run locally
without pushing.

## Prerequisites (one-time)

```bash
cargo install cargo-semver-checks --locked
```

You also need write access to the GitHub repository and the crates.io
crate. Confirm `cargo login` is current.

## Procedure

### 1. Pick the version

Decide between **patch** (`x.y.Z+1`), **minor** (`x.Y+1.0`), and
**major** (`X+1.0.0`) per [SemVer](https://semver.org). For pre-1.0
versions, **any** breaking change requires a minor bump (`0.Y+1.0`)
because pre-1.0 SemVer treats minor as the breaking axis.

Anything that touches a `#[repr(C)] bytemuck::Pod` struct layout or
changes default-config signature bytes is **always** breaking, even on
a patch-bump branch.

### 2. Update CHANGELOG.md

Move the contents of `## [Unreleased]` into a new `## [X.Y.Z] - YYYY-MM-DD`
section. Re-create an empty `## [Unreleased]` block at the top.

Update the comparison links at the bottom:

```
[Unreleased]: https://github.com/themankindproject/txtfp/compare/vX.Y.Z...HEAD
[X.Y.Z]:      https://github.com/themankindproject/txtfp/compare/v<previous>...vX.Y.Z
```

### 3. Bump the version in `Cargo.toml`

```toml
[package]
version = "X.Y.Z"
```

### 4. Run the local gauntlet

```bash
cargo fmt --all -- --check
cargo clippy --no-default-features \
  --features "std,minhash,simhash,lsh,tlsh,markup,security,serde,parallel" \
  --lib --tests -- -D warnings
cargo test --all-features
cargo doc --no-deps --no-default-features \
  --features "std,minhash,simhash,lsh,tlsh,markup,security,serde,parallel"
```

All four must pass. If anything fails, fix it in a separate commit
before continuing — the release commit is the final commit.

### 5. SemVer check

```bash
cargo semver-checks check-release \
  --only-explicit-features \
  --features std,minhash,simhash,lsh,tlsh,markup,security,serde,parallel
```

If this reports a breaking change you didn't intend, **stop and fix**.
If the change is intentional, confirm step 1 chose the correct bump
axis.

### 6. Publish dry-run

```bash
cargo publish --dry-run --all-features
```

Inspect the file list. The published tarball must contain `README.md`,
`LICENSE-MIT`, `CHANGELOG.md`, `src/`, `examples/`, and not contain any
local-workspace files (slides, node_modules, IDE state, …).

### 7. Commit

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "release: vX.Y.Z"
```

Use the literal subject `release: vX.Y.Z`. Tooling and the changelog
generator both depend on it.

### 8. Tag and push

```bash
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin main
git push origin vX.Y.Z
```

### 9. Publish

```bash
cargo publish --all-features
```

### 10. Post-release verification

- Confirm the release appears on [crates.io](https://crates.io/crates/txtfp).
- Confirm [docs.rs](https://docs.rs/txtfp) builds the new version (give
  it ~10 min).
- Confirm the tag shows up on GitHub and the CI pipeline ran green on
  the release commit.
- Open a follow-up PR adding any post-release notes to `## [Unreleased]`
  (e.g. CVE references, deprecation notices).

## What to do if step 9 fails

`cargo publish` is one-shot — once a version is on crates.io it cannot
be overwritten, only **yanked**. If the publish errors out part-way
(network, auth), retry the same `cargo publish` command. If it
succeeds and you discover a defect:

1. **Yank** the broken version: `cargo yank --version X.Y.Z`.
2. Cut `X.Y.Z+1` immediately with the fix; do **not** edit X.Y.Z.

Yanking does not delete the version (downstream lockfiles still
resolve), it only stops new dependents from picking it up.

## Reference

- [SemVer 2.0.0](https://semver.org/spec/v2.0.0.html)
- [Cargo SemVer reference](https://doc.rust-lang.org/cargo/reference/semver.html)
- [crates.io publishing guide](https://doc.rust-lang.org/cargo/reference/publishing.html)
