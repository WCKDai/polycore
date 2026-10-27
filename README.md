# PolyCore

**The bar is OCCT.** PolyCore is an open-source geometric kernel in Rust that aims to rival the
commercial kernels: exact B-rep geometry and topology (NURBS and analytic), booleans, blends,
offsets, shells, sweeps and lofts, exact face and edge history, watertight tessellation, mass
properties, and analysis-ready geometry for simulation.

PolyCore is the kernel of [Polykrom](https://github.com/wckdboy/polykrom), an open,
agent-drivable parametric CAD app. Polykrom is its only consumer. Parameters, features, names
and assemblies stay in Polykrom; PolyCore takes geometry in and returns geometry with its history.

## Status

Phase 0 is underway. [RFC 0001](docs/rfcs/0001-polycore.md) is the extraction and phase-gate
contract; [RFC 0002](docs/rfcs/0002-ai-native-engineering-platform.md) defines the expanded
AI-native CAD, simulation, robotics and USD architecture. This workspace contains Polykrom's kernel and geometry, extracted
with their Git history from `crates/kernel` and `crates/geom` into `crates/polycore` and
`crates/solidcraft-geom`, plus the vendored Truck closure, tests and regression corpus. The
source/consumer revision is in `polykrom.rev`. The public package names remain `solidcraft-kernel`
and `solidcraft-geom` until Polykrom's coordinated rename stage.

The repository also holds the [bake-off and gate harness](bench/README.md) and its first
[scorecard](bench/scorecard.md): today's Polykrom kernel measured against OpenCASCADE on the
kernel cases and ladder, with determinism and per-engine evidence.

The standalone native/wasm workspace and development tasks are the first extraction arc.
Polykrom's pinned git dependency, the complete corpus gate, the OCCT bake-off and the remaining
Phase 0 baselines are not claimed complete yet. At `polykrom.rev` the tree has 400 regression
cases and 116 ladder cases; the imported generated report still covers its older 382-case run,
so the added cases need a fresh measured baseline rather than an assumed pass.

| Phase | What |
|---|---|
| 0 | Extraction, the `Kernel` trait, the bench against OCCT, baselines |
| 1 | Own geometry and topology, watertight tessellation, exact mass properties |
| 2 | Booleans: general fuse, analytic intersections |
| 3 | Free-form intersections, sweeps, lofts |
| 4 | Blends |
| 5 | Offsets and local operations |
| 6 | Parity with OCCT, speed, truck removed |

## Build and verify

Rust 1.95 or newer is required. See [CONTRIBUTING.md](CONTRIBUTING.md) for the clean-room,
licence, determinism and testing rules.

```sh
cargo xtask check         # all workspace targets
cargo xtask test          # native unit, integration and doc tests
cargo xtask wasm          # kernel and geometry on wasm32
cargo xtask verify-deps   # the renamed, local-only Truck package closure
cargo xtask ci            # fmt, check, Clippy, tests, wasm and dependency checks
```

## Gates

Every phase ends only when all of these hold:

- **Kernel cases and ladder:** no area's pass count drops, and no case that passed fails
  (288 / 382 and 102 / 116 at the start).
- **Reference tests:** Polykrom's analytic and reference cases all pass (83 / 83 at the start).
- **Determinism:** identical results on one thread and on all cores.
- **Bake-off:** the score against OCCT, on Nightfall's RFC 0001 rubric, does not drop and meets
  the phase's target (100 % of OCCT, including speed, by Phase 6).
- **TinyRex:** an agent rebuilds the [TinyRex](https://github.com/wckdboy/tinyrex) robot from
  scratch through Polykrom's own tools, with results at least as good as the phase before
  (median of three runs).
- **Machines:** Polykrom's printer and robot dog evaluate unchanged, and the dog still walks in
  MuJoCo.
- **CI:** PolyCore's and Polykrom's CI are green at the pinned revision, including the browser
  (wasm32) build.

Graded answers are analytic, hand-calculated or invariants. OCCT runs only as a separate
program and is a cross-check, never the answer key. No OCCT, FreeCAD or other GPL/LGPL source
is read or used, and no model made in commercial CAD is used for testing.

## Licence

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Polykrom itself is MIT OR Apache-2.0.
