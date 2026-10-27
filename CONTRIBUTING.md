# Contributing to PolyCore

PolyCore is Polykrom's Rust geometric kernel. Read [RFC 0001](docs/rfcs/0001-polycore.md)
before choosing work: it defines the phase gates, repository boundary, determinism rules and
licence constraints. Phase 0 is extraction and measurement, not a rewrite.

## Build and test

Use Rust 1.95 or newer and the `ci` profile. The profile disables debug assertions because
Truck's debug-only checks reject valid seam and closed-edge geometry.

```sh
cargo xtask fmt           # rustfmt on our crates, not vendored upstream sources
cargo xtask check         # every workspace target
cargo xtask clippy        # Clippy with warnings denied
cargo xtask test          # unit/integration/doc tests; no random property cases
cargo xtask wasm          # kernel and geometry on wasm32-unknown-unknown
cargo xtask verify-deps   # no registry Truck package can enter the graph
cargo xtask ci            # all of the above
cargo xtask property-test # random boolean properties; run separately
```

For the web check, install the target with `rustup target add wasm32-unknown-unknown`.
Run `cargo xtask ci` before every commit and keep pull requests to one coherent topic.

## Build hygiene

Use the shared `target/` cache incrementally and run one heavy build or corpus run at a time.
Check free disk before a full build, and do not start one that would leave less than 10 GiB free.
Remove only obsolete clones and artifacts you created; never clean a shared checkout or another
agent's target directory. Avoid duplicate target directories, especially for Polykrom and
PolyCore joint development, and use the existing release cache where it is appropriate.

## Non-negotiables

- **Never crash.** Production code returns `Result`; unsupported geometry is refused with a
  useful error. Do not add `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `unreachable!`
  or `unsafe` to production crates. Input-derived numbers and sizes are hostile: check finiteness,
  bounds and indexing. Every crash fix includes a regression test in the existing test area.
- **Clean room and licences.** Do not read, copy or link GPL/LGPL/AGPL code. OpenCASCADE,
  FreeCAD, CadQuery/OCP and similar projects may only be used as the separate programs the RFC
  explicitly allows; OCCT is a black-box cross-check, never the answer key. Do not use models,
  measurements, files or other data made with commercial CAD products. Graded answers are
  analytic, hand-calculated or invariants.
- **PolyCore is not the app.** It takes geometry and operation values and returns geometry,
  history and evidence. It never depends on a Polykrom crate and never owns parameters, feature
  timelines, document names, constraint solvers, rendering or physics solvers.
- **Determinism.** Output cannot depend on hash iteration, addresses, worker completion order,
  global mutable state or unseeded randomness. Parallel work collects in input order. Keep the
  wasm build working.
- **No silent Truck mixing.** Only renamed `polycore-truck-*` path packages may appear in the
  dependency graph. Preserve original Rust crate names and do not reintroduce downstream
  `[patch.crates-io]` requirements. `cargo xtask verify-deps` enforces this.

## Extraction and corpus rules

`polykrom.rev` records the Polykrom source/consumer revision. Extraction changes must not alter
kernel behavior; compare the source and measured results rather than fixing unrelated kernel
bugs in the same change. Kernel behavior work belongs in a separate, measured change.

Every regression case has its own analytic or invariant answer. Never bless expected values from
PolyCore's current output. A phase gate permits no previously passing case to regress and no
area's pass count to drop. Keep generated reports, timings and baseline changes explicit and
explained in the pull request.

Vendored third-party sources are not workspace lint or formatting targets. Their behavior is
covered through kernel tests and the vendored-fix equivalence hooks. Keep their licence texts
and notices, and document local modifications in `vendor/README.md`.

By contributing, you agree that your contribution is licensed under Apache-2.0.
