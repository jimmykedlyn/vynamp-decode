# Contributing

## Global rules

- No crates.io dependencies at all, development dependencies included. Only `std`.
- `#![forbid(unsafe_code)]` everywhere, enforced by the workspace lint
  `unsafe_code = "forbid"`.
- Licence: `MIT OR Apache-2.0`.
- `agulha-core` must never depend on `agulha-testkit`. The testkit may depend on `agulha-core`.
- Commits end with the `Co-Authored-By:` trailer of the agent that authored them.

### The single `unsafe` exception

`crates/agulha-testkit/src/alloc.rs` implements `GlobalAlloc` for the allocation
counter, which requires `unsafe impl`. The testkit crate (`publish = false`,
test-only) therefore sets `unsafe_code = "allow"` in its own `[lints.rust]`
instead of inheriting the workspace lint. Every `unsafe` block there carries a
`// SAFETY:` comment and delegates to `std::alloc::System`. No other crate may do this.

## Clean-room

Decoders are reimplemented from public specifications and recorded under
`docs/sources/`. Never copy code from other implementations. Permissively licensed
sources may be studied with credit in `docs/sources/`; copyleft sources follow the
policy in [LEGAL.md](LEGAL.md).

## TDD

Write the failing test first, watch it fail for the expected reason, then implement.
Lossless codecs are verified bit-exact; lossy codecs against official conformance
vectors where they exist.

## Local gate (required before every commit)

```
cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked
```

`Cargo.lock` is committed.
