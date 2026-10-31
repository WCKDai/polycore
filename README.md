# PolyCore

<picture><source media="(prefers-color-scheme: dark)" srcset="assets/brand/polycore-dark.svg"><img src="assets/brand/polycore-light.svg" height="44" alt="polycore"></picture>

[polycore.dev](https://polycore.dev) · [Polykrom](https://polykrom.app) · [WCKD](https://wckd.ai) · [Creed / engineering for agents](https://creed.sh/creed.md)

PolyCore is the current 3D-kernel project; Nightfall is archived research. Public presentation follows
the [WCKD brand guide](https://wckd.ai/brand/). The phase gates below remain authoritative: a website
or brand refresh is not evidence of kernel readiness.

**The bar is what WCKD needs to make.** PolyCore is an open-source geometric kernel in Rust:
exact B-rep geometry and topology (NURBS and analytic), booleans, blends, offsets, shells, sweeps
and lofts, exact face and edge history, watertight tessellation, mass properties, and
analysis-ready geometry for simulation. It exists to make the parts and machines WCKD needs, not
to win a benchmark. OCCT is a reference, not a gate.

PolyCore is the kernel of [Polykrom](https://github.com/WCKDai/polykrom), an open,
agent-drivable parametric CAD app. Polykrom is its only consumer. Parameters, features, names
and assemblies stay in Polykrom; PolyCore takes geometry in and returns geometry with its history.

## Status

Phase 0 is underway. [RFC 0001](docs/rfcs/0001-polycore.md) is the extraction and phase-gate
contract; [RFC 0002](docs/rfcs/0002-ai-native-engineering-platform.md) defines the expanded
AI-native CAD, simulation, robotics and USD architecture. This workspace contains Polykrom's kernel and geometry, extracted
with their Git history from `crates/kernel` and `crates/geom` into `crates/polycore` and
`crates/polycore-geom`, plus the vendored Truck closure, tests and regression corpus. The
source/consumer revision is in `polykrom.rev`. The public packages are `polycore` and
`polycore-geom`; before Polykrom's second rename stage (Polykrom#304) they were `solidcraft-kernel`
and `solidcraft-geom`.

The repository also holds the [bake-off and gate harness](bench/README.md) and its first
[scorecard](bench/scorecard.md): today's Polykrom kernel measured against OpenCASCADE on the
kernel cases and ladder, with determinism and per-engine evidence.

The standalone native/wasm workspace and development tasks are the first extraction arc. Polykrom
builds on PolyCore through a pinned git dependency (since 2026-10-27; its CI refuses a pin whose
kernel CI isn't green), and every boolean or fallback change is gated on Polykrom's 96
robustness goldens. The complete corpus gate, the OCCT reference comparison and the remaining
Phase 0 baselines are not claimed complete yet: the baseline is #38.

**Next: exact booleans** ([Phase 2 plan, #41](https://github.com/WCKDai/polycore/issues/41)): the
analytic intersection table (#39) and the general-fuse pipeline (#40), with truck as the fallback
until each case passes. At `polykrom.rev` the tree has 400 regression
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
| 6 | The parts WCKD needs, speed, truck removed |

## Build and verify

Rust 1.95 or newer is required. See [CONTRIBUTING.md](CONTRIBUTING.md) for the clean-room,
licence, determinism and testing rules.

```sh
cargo xtask check         # all workspace targets
cargo xtask test          # native unit, integration and doc tests
cargo xtask wasm          # kernel and geometry on wasm32
cargo xtask verify-deps   # the renamed, local-only Truck package closure
cargo xtask ci            # fmt, check, Clippy, tests, wasm and dependency checks
cargo xtask task-check agent/tasks/example.json  # bounded agent task packet
```

Multi-agent ownership, model routing, evidence and token/cost policy are in
[`agent/README.md`](agent/README.md). GitHub environment/secret setup and R2-backed sccache are in
[`docs/operations/github-environments.md`](docs/operations/github-environments.md).

## Gates

Every phase ends only when all of these hold:

- **Kernel cases and ladder:** no area's pass count drops, and no case that passed fails
  (288 / 382 and 102 / 116 at the start).
- **Reference tests:** Polykrom's analytic and reference cases all pass (83 / 83 at the start).
- **Determinism:** identical results on one thread and on all cores.
- **Reference comparison:** the score against OCCT on Nightfall's RFC 0001 rubric is recorded as
  a reference, not a gate. The gate is the WCKD corpus and the parts the stack actually makes.
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

## Where this fits

PolyCore is the geometry kernel: exact B-rep, history and watertight tessellation for Polykrom, TinyRex and the world's objects. The bar is what WCKD needs to make; OCCT is a reference, not a gate.

[The brick map](https://wckd.ai/architecture/) · WCKD

## Licence

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Polykrom itself is MIT OR Apache-2.0.
