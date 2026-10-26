# RFC 0001: PolyCore

**The bar is OCCT.** PolyCore's purpose is an open-source geometric kernel, written in Rust,
that rivals the commercial kernels: first OpenCASCADE on our corpus, then Parasolid-class
robustness on blends, offsets and free-form work.

- **Status:** Accepted, 2026-10-26, with the owner's decisions under [Decisions](#decisions).
- **Author:** Claude (agent), for the owner
- **Created:** 2026-10-26
- **Home:** `docs/rfcs/` in wckdboy/polycore. Until that repository is created (it waits for
  the owner's OK), the same text is in Polykrom PR
  [#237](https://github.com/wckdboy/polykrom/pull/237).
- **Builds on:** Polykrom's [ENGINES.md](https://github.com/wckdboy/polykrom/blob/main/docs/ENGINES.md)
  (kernel stages K0 to K7). This RFC does not change those stages. It says what the kernel
  becomes, where it lives, how the code gets there and how each step is judged.

## Summary

PolyCore is the kernel; Polykrom is the app and PolyCore's only consumer. PolyCore has its own
repository and Cargo workspace, closely tied to Polykrom: Polykrom depends on it by a pinned git
revision, and both CIs test the pair. PolyCore grows out of Polykrom's `crates/kernel` and the
vendored truck crates by incremental replacement, not a rewrite. The app ships on every phase,
and every phase has an exit gate measured on the existing corpora and on TinyRex, the flagship
robot an agent rebuilds from scratch.

Nightfall, the earlier macOS CAD app, is archived as of 2026-10-26. It is a source to harvest:
its Rust NURBS math, its kernel bake-off, its selector ideas, its study work, its USD plan and
its eval harness move into PolyCore or Polykrom. Its OCCT C++ and its Swift app do not.

OpenCASCADE stays useful without its licence reaching us: a frozen program that answers the
same questions, so PolyCore can be scored against it. Graded answers are analytic,
hand-calculated or invariants. OCCT is the bar and a cross-check, never the answer key.

## Direction (owner, 2026-10-26)

| Direction | Consequence |
|---|---|
| One kernel, in Rust, named **PolyCore**; the bar is OCCT, the goal is commercial-kernel class | All kernel work goes into PolyCore. No second kernel is started. |
| PolyCore has **its own repository and workspace** | wckdboy/polycore, Apache-2.0. Created when the owner says so. |
| **Polykrom** is the product and PolyCore's only consumer | No C ABI, no external kernel protocol. The API is a Rust API. Polykrom stays MIT OR Apache-2.0. |
| **Nightfall is archived** | No Nightfall work. Its useful parts are harvested ([Harvest from Nightfall](#harvest-from-nightfall)). |
| Convergence, not rewrite | Each phase replaces one layer behind a stable boundary; the app keeps working. |
| **TinyRex** is the flagship acceptance test | It has its own repository, wckdboy/tinyrex. The agent rebuilds it from scratch on every phase, through Polykrom's own tools, with results at least as good as the phase before. |

## Where we start

Measured on `wckdboy/polykrom` `main` at `0489d63e` (2026-10-26) unless stated.

| Thing | State |
|---|---|
| `crates/kernel` (`solidcraft-kernel`) | 153 files, about 98,000 lines of Rust. About 70 public entry points, all free functions; no truck type in the public API. `#![forbid(unsafe_code)]`; panics contained by `guard`. |
| `crates/geom` (`solidcraft-geom`) | About 15,700 lines: vectors, planes, B-splines, meshes, BVH, profiles, and the native B-rep schema. No dependencies but serde and thiserror. The kernel, the sketch solver and the renderer use it. |
| Vendored truck (`vendor/`) | truck-geometry 10,065 lines, truck-shapeops 6,664, truck-meshalgo 6,253, truck-stepio 4,974, with Polykrom's fixes listed in `vendor/README.md`, wired in by `[patch.crates-io]`. truck-topology and truck-modeling come from crates.io. Apache-2.0. |
| Native B-rep schema 3 | `solidcraft_geom::brep` ([native-brep.md](https://github.com/wckdboy/polykrom/blob/main/docs/native-brep.md)). Exact, kernel-neutral, already the cache format inside designs. |
| [kernel-cases](https://github.com/wckdboy/polykrom/blob/main/docs/kernel-cases.md) | **288 / 382 pass.** Booleans 64/89, crashes 6/10, fillets 38/50, import 19/23, sheet metal 57/57, shell 9/20, surfaces 0/8, sweeps 62/83, tessellation 12/14, threads 21/28. Cases are design recipes run by `polykrom-cli` (`cargo xtask kernel-cases`). |
| [Kernel ladder](https://github.com/wckdboy/polykrom/blob/main/docs/kernel-ladder.md) | **102 / 116 pass**, with a timing golden (`tests/kernel-cases/ladder/golden.json`). |
| [Reference tests](https://github.com/wckdboy/polykrom/blob/main/docs/oracle.md) | **83 / 83 pass** (62 analytic, 21 FreeCAD run as a separate program). |
| Recent kernel work | 18 merged kernel PRs since 2026-10-19, among them PRs #189 to #236: K2 broad phase, mesh reuse, direct split, exact plane–plane SSI, presearch grids; K4 rolling-ball fillets across loft seams and vertex blends (`loopblend.rs`, `curveblend.rs`, `blend_corner.rs`); K5 NURBS offset fitting, free-form shell and thicken (`offset_fit.rs`, `offset_free.rs`). Several are still open (#224 to #236). |
| Known weak points (ENGINES.md) | Surface–surface intersection starts from a mesh collision. Boolean tolerance is a fraction of part size, wrapped in a retry ladder (4 shifts × 2 tolerances) and about ten special-case modules. Cylinders, cones, spheres and tori are not first-class surfaces. Ten corpus cases hang past 180 s. |

Other kernels and code we can draw on:

| Source | Licence (checked 2026-10-26) | State |
|---|---|---|
| Nightfall (wckdboy/nightfall) | Apache-2.0, our own | Archived 2026-10-26. OCCT 7.9.3 behind a C++ `IKernel` in an XPC service. Rust crates under its RFC 0008: `nightfall-math` (about 780 lines: checked knot vectors, basis functions, NURBS curves and surfaces with mixed partial derivatives, std only, no unsafe); thin Rust wrappers over the OCCT adapter; `nightfall-assembly` (an SE(3) placement solver). A study service in C++ (MFEM physics, Netgen meshing, a sparse Cholesky solve). |
| SMLib (in NVIDIA-Omniverse/usd-brep 1.0.0) | Apache-2.0 (LICENSE: © NVIDIA) | A complete C++ kernel. Scored **50.7** against OCCT's **77.4** in Nightfall's RFC 0001 bake-off: no operation history API, area errors up to 2e-4 on curved faces, a use-after-free in booleans on closed edges (SMLIB-001), all-or-nothing fillet requests. On Polykrom's `kernel-smlib` branch it passed 317 of 372 kernel-cases (ENGINES.md). |
| solidcore (tangency-labs/solidcore) | Apache-2.0 (LICENSE: © NVIDIA, as a port) | A safe-Rust port of SMLib's kernel and SM_API, started 2026-10-12. 238 of 261 units ported, 116 green (identical to the C++ within 1e-9), 122 partial; 89.8 % of upstream lines. Integration and end-to-end solids are still in progress; API 0.0.x. Upstream SolidCraft has paused its own truck work for it (tangency-labs/solidcraft#1597). |
| truck (ricosjp/truck) | Apache-2.0 | Active. Clean, but booleans are mesh-seeded and fillets are weak. |
| Fornjot | 0BSD (LICENSE) | Archived June 2026, goals unreached. |
| OpenCASCADE 7.9.3 | LGPL-2.1 | The only open kernel with the full set. Out of process only. |

## Sources: what we may read, and how we use it

| Source | Read the code? | Use |
|---|---|---|
| Polykrom, Nightfall | Yes | Our own code. Port freely, keep the history in commit messages. |
| truck | Yes | Kept behind adapters until replaced; pieces ported where they pass the corpus. |
| SMLib and SM_API (`source/SMLib`, `source/SM_API` in usd-brep) | Yes | A reference implementation. Algorithms ported with attribution: NVIDIA's copyright line in the file header, a `NOTICE` entry. Never linked. |
| solidcore | Yes | As SMLib, plus the [standing practice](#solidcore-a-standing-practice) below. |
| usd-brep `omniSolid` schema | Yes (the schema and its docs) | A data format, read and written by our own code. |
| usd-brep's OCCT converters (`tools/occt_to_usd`, `tools/usd_to_occt`, `OCCT_BREP_IMPORT`) | **No** | Built on OCCT headers. Those directories are skipped. |
| OpenCASCADE | **No** | Public documentation only (for example the Boolean Operations specification and the test-system guide), and black-box runs of the frozen reference program. Never read, copied or ported. |
| FreeCAD, CadQuery and OCP | **No** | Separate programs only: FreeCAD for Polykrom's reference values; CadQuery or build123d only to turn permissive scripts into test models. |
| Netgen, GMP, TetGen | **No** | Not linked and not read. fTetWild needs GMP, so it runs as a separate program. |
| fTetWild | Not needed | MPL-2.0. A separate program for volume meshing in Polykrom. |
| MFEM | Yes | BSD-3-Clause. Nightfall's `physics.cpp` on MFEM is the reference the Rust study code is checked against. |
| faer | Yes | MIT. faer 0.24.4 (crates.io, 2026-10-26) has sparse Cholesky with AMD ordering, simplicial and supernodal LLᵀ and LDLᵀ. |
| Papers and books | Yes | Jackson's local tolerances, Patrikalakis–Maekawa, the 2026 ACM TOG NURBS SSI papers, Rossignac–Requicha offsets, Choi–Ju and Várady–Rockwood blends, generalised winding numbers for trimmed NURBS. Full list in ENGINES.md. |
| Commercial CAD | **No** | No data, measurements or files made with them (Polykrom's AGENTS.md). |

## Licences

- **PolyCore is Apache-2.0**: one `LICENSE`, one `NOTICE`. Code moved from Polykrom's
  `crates/kernel` and `crates/geom` is MIT OR Apache-2.0 and is taken under its Apache-2.0
  option; its authors' notices are kept, and upstream SolidCraft is credited in `NOTICE`.
  Vendored truck keeps its Apache-2.0 notices. Anything ported from SMLib or solidcore carries
  NVIDIA's copyright line in the file and a `NOTICE` entry.
- **Polykrom stays MIT OR Apache-2.0.** Its binaries include PolyCore and so carry PolyCore's
  Apache-2.0 notice.

## Scope

### What PolyCore owns

| Area | Contents |
|---|---|
| Geometry | Points, vectors, tolerances, intervals, robust predicates. Curves: line, circle, ellipse, B-spline, NURBS, procedural intersection curves with a fitted spline, p-curves. Surfaces: plane, cylinder, cone, sphere, torus, extrusion, revolution, offset, B-spline, NURBS. Evaluation with derivatives, projection, closest point. |
| Topology | Exact B-rep: bodies, shells, faces, loops, coedges, edges, vertices. Tolerant edges and vertices with an absolute model resolution of 1e-6 mm. A validity checker run after every operation in debug builds. |
| Operations | Primitives, extrude, revolve, sweep, loft, booleans (union, cut, intersect, split, general fuse), blends (fillets, chamfers, vertex blends), offsets, shell, thicken, draft, replace and delete face, sewing and healing. |
| Naming and lineage | For every operation, the exact history of every result face and edge (as `history.rs` records today), entity identity that survives cloning, and composition of histories across operations. |
| Tessellation | Display and export meshes with a **watertight guarantee**: a closed body gives a closed, consistently oriented 2-manifold mesh with welded vertices, or a refusal that says why. Face and edge tags on every triangle and segment. |
| Mass properties | Volume, area, centre of mass, inertia tensor and principal axes from the exact B-rep, with the error bound achieved. |
| Analysis-ready geometry | Watertight, face-tagged surface meshes for volume meshers; exact inertia for robot export; contact geometry (recognised analytic primitives, convex pieces, clearances); mid-surfaces with a thickness field for thin-walled FEA. |
| Interchange | STEP, IGES, the native schema 3, 3MF meshes with face tags, USD `omniSolid` B-rep and USD meshes ([Interchange](#interchange)). |

### What stays in Polykrom

"Fully parametric" lives in Polykrom's document layer, not in PolyCore.

| Polykrom layer | Owns | Talks to PolyCore with |
|---|---|---|
| `solidcraft-doc` | Parameters, expressions, units, configurations, the feature timeline, dependency-aware rebase, persistent names (`lineage.rs` turns kernel histories into names), face attachments, components, joints, motion studies | Explicit geometry and operation specs: profiles, distances, edge sets. Never parameters, features or names. |
| `solidcraft-sketch` | The 2D constraint solver and profiles | Profiles as curves |
| `solidcraft-engine`, `solidcraft-mcp`, control channel | Commands, MCP, scripts, undo | Through the document, never directly for modelling |
| Studies ([Study program](#the-study-program)) | Volume meshing, FEA, result fields | PolyCore's analysis-ready geometry |
| Simulation | MuJoCo (`tools/mujoco/runner.py`), MJCF and URDF export | Watertight meshes, inertia, contact primitives |

The boundary rule: PolyCore takes values and returns values plus history. It never reads a
design, a parameter or a feature. The document decides what a face is called; PolyCore
reports where each face came from. Being a separate workspace, PolyCore cannot depend on a
Polykrom crate.

### What PolyCore does not own

- Constraint solving for sketches and assemblies.
- Volume meshing and solvers.
- Rendering.

## Architecture

### Repository and workspace

```
polycore/
  crates/
    polycore/            the public API: the Kernel trait, types, re-exports
    polycore-math/       vectors, tolerances, predicates, B-splines and NURBS
    polycore-geom/       curves, surfaces, SSI, procedural intersection curves
    polycore-topo/       the B-rep arena, tolerant entities, p-curves, validity checker
    polycore-ops/        every modelling operation
    polycore-mesh/       tessellation with the watertight guarantee, BVHs
    polycore-naming/     histories, entity identity, history composition
    polycore-props/      mass properties, distance, clearance, contact, mid-surfaces
    polycore-io/         STEP, IGES, schema 3, 3MF meshes, USD
    polycore-legacy/     every remaining truck call; shrinks each phase, deleted in Phase 6
    polycore-bench/      corpus runner, bake-off scoring, OCCT client, minimiser (dev only)
  vendor/                truck crates, renamed polycore-truck-* (until Phase 6)
  tests/kernel-cases/    the regression cases and the ladder (moved from Polykrom)
  xtask/                 ci, kernel-cases, ladder, bench
  docs/rfcs/
  LICENSE  NOTICE  README.md
```

Dependencies point one way: math ← geom ← topo ← (ops, mesh, naming, props, io) ← polycore.

### How Polykrom consumes it

- **Pinned by revision.** Polykrom's workspace `Cargo.toml` names
  `polycore = { git = "https://github.com/wckdboy/polycore", rev = "<sha>" }`. Polykrom crates
  depend on `polycore` only, never on a `polycore-*` crate directly.
- **Joint development.** A developer working on both checks them out side by side and adds an
  untracked override, so CI always builds the pinned revision:

  ```toml
  # polykrom/.cargo/config.toml, untracked; or cargo --config polycore-dev.toml
  [patch."https://github.com/wckdboy/polycore"]
  polycore = { path = "../polycore/crates/polycore" }
  ```

- **No truck patch downstream.** Cargo applies `[patch]` only in the root workspace, so
  Polykrom's `[patch.crates-io]` entries for truck would not follow PolyCore. Phase 0 therefore
  vendors every truck crate the kernel uses (adding truck-topology, truck-modeling,
  matext4cgmath and the truck crates they need) under renamed packages, `polycore-truck-*`.
  Both workspaces then resolve the same code.
- **A change that spans both** lands as a PolyCore PR first, then a Polykrom PR that bumps the
  pinned revision and carries PolyCore's CI result for that revision.

### CI

| Where | Runs | On |
|---|---|---|
| PolyCore | `cargo xtask ci`: fmt, Clippy with warnings denied, unit tests, the wasm32 build, the determinism check | every PR |
| PolyCore | The kernel cases and the ladder. The cases are design recipes, so the driver is `polykrom-cli`: CI checks out Polykrom at the revision in `polykrom.rev`, patches it to the PolyCore commit under test, builds the CLI and runs `cargo xtask kernel-cases`. It regenerates `docs/kernel-cases.md`, `docs/kernel-ladder.md` and the ladder golden. | every PR |
| PolyCore | The bake-off against the OCCT reference program | nightly, and on a phase's closing PR (a macOS runner: the reference program is built for Apple silicon) |
| Polykrom | `cargo xtask ci` against the pinned PolyCore revision, including the reference tests (`tests/oracle/`), which stay in Polykrom | every PR |
| Polykrom | A smoke subset of the kernel cases against the pinned revision | every PR that bumps it |

From Phase 2, a kernel-level case format (a sequence of `Kernel` calls, entities picked by
sample points) lets most cases run in PolyCore without the Polykrom driver; the recipe-level
run stays as the end-to-end check.

### How today's code moves

| Today (Polykrom) | Becomes | When and how |
|---|---|---|
| `crates/kernel` | `polycore` | Phase 0: moved with its history (`git filter-repo` of its paths), the package renamed, the `Kernel` trait extracted; no behaviour change. Its modules then move out to the crates above, one area per PR. |
| `crates/geom` | `polycore-math`, `polycore-mesh`, `polycore-io` (schema 3) | Phase 0 moves it whole, since the kernel, the schema and the sketch solver all use it; Polykrom's sketch and render crates then depend on PolyCore's. Phase 1 splits it. |
| `vendor/truck-*`, `vendor/matext4cgmath`, truck-topology, truck-modeling | `vendor/polycore-truck-*` | Phase 0, renamed so no downstream patch is needed. Then replaced: geometry and tessellation in Phase 1, topology in Phases 1–2, booleans in Phase 2. Deleted in Phase 6. |
| `tests/kernel-cases/` (cases and ladder), `xtask` kernel-cases | `tests/kernel-cases/`, `xtask` | Phase 0. Polykrom's xtask keeps a `--cases DIR` option for its smoke subset. |
| `nightfall-math` (archived Nightfall) | `polycore-math` | Phase 1: copied with its tests and probes. Where it overlaps `bspline.rs`, the faster one that passes both test sets stays. |
| Blends, offsets, sweeps, lofts | `polycore-ops` | Re-based onto `polycore-topo` in Phases 3 to 5, keeping their cases. |

Schema 3 is the bridge during the move: any body can go truck → schema 3 → PolyCore and back
exactly, so old and new code can run in one feature and in one design.

### The API

One trait, so that the same corpus and the same bake-off run on any implementation: PolyCore,
the legacy truck path while it exists, and (in the bench only) the OCCT reference program.

```rust
pub trait Kernel: Send + Sync {
    /// Name, version and build hash; part of every cache key.
    fn identity(&self) -> &KernelId;
    fn capabilities(&self) -> &Capabilities;

    fn primitive(&self, cx: &Cx, spec: &Primitive) -> Outcome<Body>;
    fn extrude(&self, cx: &Cx, profile: &Profile, spec: &Extrude) -> Outcome<Body>;
    fn revolve(&self, cx: &Cx, profile: &Profile, spec: &Revolve) -> Outcome<Body>;
    fn sweep(&self, cx: &Cx, spec: &Sweep) -> Outcome<Body>;
    fn loft(&self, cx: &Cx, spec: &Loft) -> Outcome<Body>;
    fn boolean(&self, cx: &Cx, op: BoolOp, targets: &[Body], tools: &[Body]) -> Outcome<Vec<Body>>;
    fn blend(&self, cx: &Cx, body: &Body, spec: &Blend) -> Outcome<Body>;
    fn offset(&self, cx: &Cx, body: &Body, spec: &Offset) -> Outcome<Body>; // shell, thicken, offset faces, draft
    fn split(&self, cx: &Cx, body: &Body, tool: &SplitTool) -> Outcome<Vec<Body>>;

    fn check(&self, body: &Body) -> Validity;
    fn mass(&self, cx: &Cx, body: &Body) -> Outcome<MassProps>;
    fn tessellate(&self, cx: &Cx, body: &Body, spec: &Tessellation) -> Outcome<Mesh>;
    fn analysis_mesh(&self, cx: &Cx, body: &Body, spec: &AnalysisMesh) -> Outcome<TaggedMesh>;
    fn read(&self, cx: &Cx, format: Format, bytes: &[u8]) -> Outcome<Vec<Body>>;
    fn write(&self, cx: &Cx, format: Format, bodies: &[Body]) -> Outcome<Vec<u8>>;
}

/// A result with its history and evidence, or a refusal that says why.
pub type Outcome<T> = Result<Done<T>, Refusal>;
pub struct Done<T> { pub value: T, pub history: History, pub evidence: Evidence }
pub enum Refusal { Unsupported, InvalidInput, Failed, OutOfBudget, Cancelled, Internal }

/// Model resolution, time and attempt budget, thread budget, progress and cancellation.
pub struct Cx { /* … */ }
```

- `Body` is immutable, `Send + Sync`, shared by `Arc`, and carries a content hash.
- `Evidence` says how a result was checked: tolerance achieved, fit errors, validity, time.
  No caller gets a bare number (ENGINES.md principle 4).
- Every refusal carries the stage and the entities involved. No operation returns a partial body.
- Today's 70 entry points map onto these methods with typed specs. App-level features built
  from them (emboss, lips, threads, sheet metal) stay in Polykrom unless they need kernel internals.
- The trait is semver-versioned. It is stable from the end of Phase 2; before that it may
  change in any PolyCore release whose Polykrom bump updates every caller.
- There is no C ABI. Polykrom is Rust and is the only consumer.

### Determinism and threading

1. **Same input, same output.** The same inputs on the same build give bit-identical bodies,
   histories and meshes, whatever the thread count. Across targets (Linux x86-64, macOS
   aarch64, wasm32) topology and history are identical and coordinates agree within the model
   resolution. CI runs the corpus on 1 thread and on all cores and compares hashes.
2. **No hash order.** No output depends on iterating a `HashMap` or on addresses. Ordered maps
   and arena indices only (truck's face order once followed hash order; see Polykrom's
   `vendor/README.md`). A Clippy `disallowed-types` rule enforces it.
3. **Ordered parallelism.** Parallel work maps independent items and collects in item order
   (the `par::map` pattern). Floating-point sums run in a fixed order, never in the order
   workers finish.
4. **No global state.** No statics with interior mutability. The current thread-local `Watch`
   becomes a field of `Cx`.
5. **Immutable bodies.** Operations are pure functions of their inputs and `Cx`.
6. **Budgets.** Every operation runs under a time and attempt budget and checks cancellation.
   A hang is a failure the corpus records, never an accepted state.
7. **No nudges.** Retrying with shifted operands is retired with the retry ladder in Phase 2:
   a result that depends on a random-looking shift is not reproducible in meaning.
8. **wasm32.** Single-threaded, same results. Polykrom's browser build stays green in every phase.
9. **No randomness** outside seeded tests.

## Convergence plan

### Gates for every phase

A phase ends when all of these hold, measured on that phase's PolyCore and attached to its
closing PR:

| Gate | Rule |
|---|---|
| **G-cases** | kernel-cases: no area's pass count drops, and no case that passed fails. New cases may be added; the baseline is regenerated with them. |
| **G-ladder** | Ladder: no case that passed fails; no tier's total time exceeds its golden by more than 10 % without a recorded reason. |
| **G-oracle** | Polykrom's reference tests stay 83/83 (or all of a larger set). |
| **G-determinism** | The determinism check passes on the whole corpus. |
| **G-bakeoff** | The weighted score against OCCT is not lower than the phase before, and meets the phase's target. |
| **G-tinyrex** | TinyRex rebuilt from scratch by the agent, results at least as good ([below](#tinyrex-the-flagship-test)). |
| **G-machines** | The printer and the dog evaluate from their saved designs with no feature errors, the same body and face counts, and mass properties within 1e-6 relative of the phase before (or a recorded reason, backed by an analytic check, where a better kernel changes the shape); the dog's `sim.mujoco` walk is `ok`. |
| **G-ci** | PolyCore's and Polykrom's CI green at the pinned revision, including Polykrom's wasm32 build. |

### TinyRex, the flagship test

TinyRex is a walking robot built in Polykrom through Polykrom's own AI tooling: the MCP
server, engine commands and the control channel. It found #216 (MJCF meshes with gaps) and the
free-form shell cases behind #194. It lives in its own repository, **wckdboy/tinyrex**, which
PolyCore and Polykrom reference by revision. That repository holds the spec, the prompt, the
hand-worked checks and every phase's results.

On every phase the agent builds TinyRex **from an empty design**, using only Polykrom's tools,
on Polykrom pinned to that phase's PolyCore. Polykrom's `tools/agent-eval` runs the task from
wckdboy/tinyrex and grades the saved design, never the agent's account of it.

| Measure | At least as good means |
|---|---|
| Build | Every feature evaluates. Every body is one valid closed shell by PolyCore's checker. |
| Mass | Total mass and each body's mass, centre of mass and inertia from the exact B-rep, within the spec's tolerances. |
| Meshes | Every closed body exports a watertight mesh (no open or over-shared edges); MuJoCo prints no "not a closed solid" warning. |
| Simulation | The walk study is `ok` (finite, no resets, not fallen), with every body's mass counted. Peak servo loads are reported; a load past a rating is a design finding for the spec, not a pass. |
| Finish | `inspect.polish` score not lower than the phase before. |
| Kernel | Refusals and workarounds the agent hit: not more than the phase before. Kernel time: not more. |
| Agent | Turns, wall time and cost: median within 1.25× of the phase before (agent runs vary). |

The agent runs three times per phase; the gate reads the median run, and all three reports
go to wckdboy/tinyrex. Phase 0 records the baseline on truck.

### The bake-off against OCCT

Nightfall's RFC 0001 rubric, with its weights and normalisation, becomes PolyCore's standing
score against the bar. OCCT 7.9.3 runs as a separate program; its results are never the answer key.

| Metric | Weight | Change from Nightfall's RFC 0001 |
|---|---|---|
| Operation success | 30 % | Unchanged: valid closed solids ÷ operations attempted. |
| Correctness | 20 % | Graded against analytic or invariant answers only, within 1e-6 relative. Cases without one are reported, not scored. |
| Operation history | 20 % | Measured by a probe that traces every result face and edge. |
| Fillet and blend quality | 10 % | Unchanged. |
| Performance | 10 % | Unchanged: median time over shared successes, release build. |
| Robustness | 10 % | Replaces "integration cost", which only made sense for C++ kernels: 40 for no panic and no hang on the corpus, 30 for the determinism check, 30 for a green wasm32 build. |

Corpus: the 20 reference parts from Nightfall's bake-off (re-expressed as Polykrom recipes with
their analytic answers), the kernel cases, the ladder, the reference tests, the 119-case SSI
corpus, our own generated cases, and the open [test models](#test-models) below.

Targets, as a share of OCCT's weighted score on the same corpus and machine:

| Phase | Target |
|---|---|
| 2 | ≥ 90 % on the prismatic and turned subset |
| 3 | ≥ 90 % on the whole corpus |
| 4 | Fillet metric ≥ OCCT's |
| 6 | ≥ 100 % overall, and performance metric 100 (PolyCore's median time no slower than OCCT's) |

Past Phase 6 the bar moves to the commercial kernels: the corpus keeps growing from every
failure, and the published behaviour of commercial kernels (documentation, papers, test
methodology, never their software or its output) sets the next cases.

### The OCCT reference program

A frozen, pinned Nightfall `nf` build, used only as a tool. `nf kernel serve` (Nightfall
RFC 0015) reports every result face's history, which the history metric needs. It is never
changed.

| | |
|---|---|
| Source | wckdboy/nightfall at `08035ea591f22891694d40fa47172e7d55ada457` (`main`, 2026-10-26). The repository is archived; archived repositories stay cloneable. |
| OCCT | 7.9.3 at `a016080bf6738d6aeae020badee4e888ad1540a5`, built by `third_party/build-occt.sh`, dynamically linked (LGPL-2.1). |
| Platform | macOS on Apple silicon: the CMake presets fix `arm64` and deployment target 15.0. A Linux build would need a preset change in a copy kept with the bench, not in the frozen tree. |
| Use | `nf kernel serve`: one JSON request per line on stdin, STEP operands in, STEP results with face history out. |

Build recipe (Phase 0 runs it and records the binary's SHA-256 in `polycore-bench`):

```sh
git clone https://github.com/wckdboy/nightfall nightfall-oracle
cd nightfall-oracle
git checkout 08035ea591f22891694d40fa47172e7d55ada457
third_party/build-occt.sh --jobs 4
cmake --preset occt -DNF_WITH_STUDIES=OFF   # no Netgen or MFEM needed
cmake --build --preset occt --target nf
build/occt/tools/cli/nf kernel serve --timeout 120
```

### Test models

No model made in commercial CAD is used (Polykrom's AGENTS.md). That rules out the ABC,
Fusion 360 Gallery and NIST sets. We create our own cases, and we use existing open models
that are permissively licensed and were made in open tools. Licences below were read from each
project's own licence file or licence metadata on 2026-10-26.

| Source | Licence | Made in | What it gives | How we use it |
|---|---|---|---|---|
| FreeCAD/FreeCAD-library | Models: CC-BY-3.0 (`LICENSE-Assets`) | FreeCAD | Thousands of parts as `.FCStd`, STEP and BREP | Only parts authored in FreeCAD (checked per part: some may be imported vendor files); each part credited to its author from the git history. |
| CadQuery/cadquery-contrib | MIT | CadQuery | Parametric example parts and projects | Scripts run as a separate program to make STEP; we don't read CadQuery or OCP. |
| gumyr/cq_warehouse | Apache-2.0 | CadQuery | Fasteners, bearings, chains, sprockets, threads | As above. Hand-checkable dimensions from the standards they model. |
| gumyr/build123d (examples) | Apache-2.0 | build123d | Tutorial and example parts | As above. |
| meadiode/cq_gears | Apache-2.0 | CadQuery | Involute spur, helical, bevel and ring gears | As above; gear geometry has closed forms. |
| sethfischer/cq-electronics | MIT | CadQuery | Boards and connectors | As above. |
| BelfrySCAD/BOSL2 | BSD-2-Clause | OpenSCAD | CSG shapes, threads, gears, rounded and chamfered parts | OpenSCAD makes meshes: used for mesh import, and re-expressed as Polykrom recipes. |
| kennetek/gridfinity-rebuilt-openscad | MIT | OpenSCAD | Gridfinity bins and baseplates | As BOSL2. |
| hannobraun/fornjot (`models/`) | 0BSD | Fornjot | Small kernel test models | Re-expressed as recipes. |
| Our own | Apache-2.0 | Polykrom | Kernel cases, ladder, reference tests, SSI corpus, Nightfall's 20 parts, TinyRex, the printer, the dog | The core of the corpus. |

Checked and excluded:

| Source | Why |
|---|---|
| KiCad/kicad-packages3D | CC-BY-SA-4.0: share-alike, not permissive. |
| OpenFlexure microscope | CERN-OHL-S-2.0: strongly reciprocal. |
| VoronDesign/Voron-2, prusa3d/Original-Prusa-i3, nophead/NopSCADlib | GPL. |
| openscad/MCAD | LGPL-2.1. |
| TheRobotStudio/SO-ARM100 | Apache-2.0, but its STEP files name "Autodesk Translation Framework" as originating system: made in commercial CAD. |
| ABC, Fusion 360 Gallery, NIST CAD models | Made in commercial CAD. |

### Phases

| Phase | ENGINES | What | Phase-specific exit gate | Effort (eng.-months, AI-assisted) |
|---|---|---|---|---|
| **0 Extraction and harness** | K0 | Create wckdboy/polycore (on the owner's OK). Move `crates/kernel`, `crates/geom`, the vendored truck crates (renamed `polycore-truck-*`, with truck-topology and truck-modeling added) and `tests/kernel-cases` into it; Polykrom switches to the pinned git dependency. Extract the `Kernel` trait with no behaviour change. Build `polycore-bench`: the RFC 0001 rubric, the OCCT reference program, Nightfall's 20 reference parts. Create wckdboy/tinyrex with the spec and checks. Record every baseline. | Polykrom builds and passes CI on the pinned revision with identical kernel-case, ladder and reference-test results; PolyCore CI green; first bake-off score published; TinyRex baseline run on truck. | 1–2 |
| **1 Own geometry and topology** | K1 | `polycore-math`, `-geom`, `-topo`, `-mesh`; analytic surfaces first-class; model resolution 1e-6 mm, fixed after measuring the corpus; tolerant edges and vertices, p-curves; the validity checker; tessellation and STEP export off truck; mass properties from the exact B-rep; analysis meshes and analytic contact primitives. Truck survives only in `polycore-legacy`. At the end, re-evaluate adopting solidcore wholesale. | Tessellation 14/14; #216 closed (watertight meshes and B-rep inertia in MJCF); volume and area within 1e-6 relative on every analytic reference test; every corpus body round-trips truck ↔ PolyCore exactly; the solidcore re-evaluation published. | 2–4 |
| **2 Booleans** | K2 | General-fuse pipeline (coincidences settled lowest dimension first), the analytic SSI table, the existing BVH broad phase, procedural intersection curves, classification by ray casting with winding numbers as fallback. The truck booleans, the retry ladder and the special-case modules are deleted as their cases pass. Kernel-level case format. | Booleans ≥ 84/89 and crashes 10/10; ladder tier 02 20/20; no hangs; boolean median time ≤ half of truck's; bake-off target. | 4–8 |
| **3 Free-form SSI, sweeps, lofts** | K3 | Subdivision seeding with normal-cone loop tests, marching, tangent and overlap handling; sweeps and lofts on PolyCore topology; USD `omniSolid`. | SSI corpus 119/119; sweeps ≥ 78/83; surfaces ≥ 6/8; import ≥ 22/23; bake-off target. | 6–12 |
| **4 Blends** | K4 | Rolling-ball fillets, chamfers and vertex blends on PolyCore topology, porting today's `loopblend`, `curveblend`, `blend_corner`; SMLib's and solidcore's fillet units read as references. | Fillets ≥ 47/50; ladder tier 07 17/17; action-matrix fillet failures (#556, 986 today) ≤ 250; bake-off target. | 6–12 |
| **5 Offsets and local operations** | K5 | One extend, re-intersect, re-topologise engine for offset, shell, thicken, draft, replace and delete face, with topology change; mid-surfaces for thin-walled FEA. | Shell ≥ 18/20; ladder tier 09 11/11; no seeded fuzz golden fails on a shell error. | 6–12 |
| **6 Parity and speed** | K6, K7 | Healing and tolerant import, advanced blends as the corpus asks, profiling. `polycore-legacy` and `vendor/polycore-truck-*` deleted. | kernel-cases ≥ 95 %; bake-off ≥ 100 % of OCCT including performance. | ongoing |

Phases 3, 4 and 5 can overlap once Phase 2 ends: they touch different operations, and all
three build on the same topology. Polykrom consumes PolyCore from Phase 0, on desktop and in
the browser.

### What happens to each kernel

| Project | Decision | Why |
|---|---|---|
| truck | **Replace piecewise, then drop** (Phase 6) | Its numeric design (mesh-seeded SSI, no local tolerances, no p-curves) is the root cause of most failures. Fixes that help truck users still go upstream to ricosjp/truck. |
| solidcore | **A reference to port from; watched; contributed to** | See the standing practice below. |
| SMLib / usd-brep kernel | **Learn from; never link** | Readable, Apache-2.0, complete. As a C++ library it had 250 memory-safety errors per 1,000 sanitised operations. Polykrom's `kernel-smlib` and `smlib-link` branches stay experiments, not merged. |
| usd-brep `omniSolid` schema | **Interop only** | A data format for exact solids in USD. |
| OpenCASCADE | **The bar; a black-box program only** | Scores and cross-checks. Never code. |
| Fornjot | **Literature, and its test models** | Archived. |

### solidcore: a standing practice

solidcore shares our language, licence family, determinism and wasm goals, and upstream
SolidCraft is betting on it. So, for as long as both projects run:

- **Watch it.** `polycore-bench` follows solidcore's progress page and releases. When a unit
  goes green or improves, the bench runs it through our corpus in its area.
- **Port what wins.** A solidcore unit that beats PolyCore on its area of our corpus is ported
  (or depended on), with attribution.
- **Give back.** Anything we build that helps solidcore goes upstream to tangency-labs/solidcore
  as PRs, following its contribution rules: bug fixes found by our corpus, corpus cases and
  regression tests, robustness and determinism fixes, and difftest findings. Bugs we can't fix
  go upstream as reports.
- **Re-evaluate at the end of Phase 1.** Adopting solidcore wholesale as the base of
  `polycore-geom` and `polycore-ops` is judged on our corpus and the bake-off, given it is
  89.8 % ported.

## Harvest from Nightfall

Nightfall is archived (2026-10-26); another agent handles its notices. It is Apache-2.0 and our
own, so anything can move, read from the archived repository. Only what earns its place does.

| Item | What it is | Goes to | How |
|---|---|---|---|
| Rust NURBS math (RFC 0008) | `nightfall-math`: checked knot vectors, basis functions, NURBS curves and surfaces, mixed partial derivatives, fixed stack workspaces (degree ≤ 9), finite-difference and property tests, probes and a bench | `polycore-math` | Phase 1: copy the source and tests; merge with `bspline.rs`, keeping the one that passes both test sets faster. |
| Bake-off (RFC 0001) | The rubric, the decision rule, the per-process runner (crashes become results), sanitiser runs, the history probe, the 20 reference parts, findings F1–F13 | `polycore-bench`; the parts into Polykrom's `tests/oracle/` | Phase 0. Port the Python runner and scoring; re-express the 20 parts as Polykrom recipes with their analytic answers. The C++ kernel drivers stay behind. |
| Selectors and feature IR (RFC 0005) | References by how an entity came to be, narrowed by geometry; cardinality (`expect`); content hashes per feature | Polykrom's document and MCP | Polykrom's names already follow exact kernel history, which is stronger than generative names plus OCCT's `Modified`/`Generated`. Take three ideas: a reference that resolves to the wrong count is an error with a pointer, never a guess; geometric filters (`type`, `normal`, `near`, `radius`, `convex`) as an agent-facing query in MCP, so an agent can name a face without a pick; per-feature content hashes, measured against today's dependency-aware rebase before adopting. The IR schema itself is not taken. |
| Studies (RFC 0014) | `services/study`: a study runner, MFEM physics (P2 elasticity and heat), a sparse Cholesky solve, unit handling; the P2 surface field (mid-edge nodes, each P2 triangle drawn as four); the cantilever and thermal checks within 2 % | Polykrom ([Study program](#the-study-program)) | The physics and the checks become the reference for the Rust study code; the field format is kept for Polykrom's stress contours (#29). Netgen is dropped. |
| USD exchange plan (RFC 0002) | Exact solids in USD through usd-brep's `omniSolid` `BrepArray` | `polycore-io` | Take the plan, not the build: PolyCore writes and reads `BrepArray` prims as `.usda` with its own code. OpenUSD and usd-brep's validator run in CI as separate checking tools. Nothing of OpenUSD is linked. |
| Assemblies and URDF (RFC 0013) | IR v2 components, occurrences and joints; `nightfall-assembly` with residual, rank and DOF diagnostics; URDF export in C++ | Tests in Polykrom's `crates/doc` | Polykrom already has joints, closed loops with DOF and rank, motion studies, URDF and MJCF. Take the four-bar and slider-crank closure cases (residuals < 1e-9) and the URDF test expectations as extra tests. The code is not needed. |
| Evals harness (`agent/evals`) | build, edit and repair tasks over reference parts; a deterministic `reference` runner that checks the harness and the MCP contract without a model; `SKIP` for sessions that could not start; `--resume`; the rule that the pass rate may not drop | Polykrom's `tools/agent-eval` | Polykrom's harness is already broader (parts and systems). Add the edit and repair task kinds, the reference runner, `SKIP` and `--resume`, and the rule. TinyRex runs there from wckdboy/tinyrex. |

Not worth keeping:

| Item | Why |
|---|---|
| The OCCT C++: `core/kernel` (`IKernel` and its OCCT backend), `core/kernel/native`, the C++ evaluator in `services/kernel`, `urdf.cpp` | OCCT-specific. PolyCore replaces the kernel; Polykrom already has the evaluator and URDF. The frozen `nf` binary is kept only as the reference program. |
| The Swift app and the XPC service | Polykrom is the app. |
| `nf kernel serve` as a consumer (RFC 0015) and Polykrom's `feat/nightfall-backend` branch | There is no second kernel to call. The branch closes unmerged. |
| `core/ffi` and the RFC 0008 C ABI | No non-Rust consumer. |
| The Nightfall IR schema and its migrations | Polykrom has its own format (`solidcraft/N`). |
| The OpenUSD, SMLib and Netgen builds | Interop is through file formats; Netgen is dropped. |
| Nightfall's vendored copy of Polykrom's sketch solver | It came from Polykrom. |

## The study program

The study program lives in Polykrom. PolyCore supplies the analysis-ready geometry:
watertight, face-tagged surface meshes, mid-surfaces with thickness, and exact mass
properties. The aim is to integrate it as natively as possible, for speed and precision.

Today Polykrom's `analysis.static_stress` runs Nightfall's `nf study solve` (Netgen and MFEM)
as a separate program. Existing `nf` builds keep working until the replacement passes its gate.

### Options

| Option | Licence | In process | Browser (wasm) | Precision | Speed | Effort | Notes |
|---|---|---|---|---|---|---|---|
| **A. Rust-native FEA** (our own P2 tetrahedra for linear elasticity, heat and modal; faer for the solve) | ours; faer MIT | Yes | Yes | The same P2 discretisation as Nightfall's MFEM code; verified by manufactured solutions and NAFEMS benchmarks | No process or file round trip; faer's supernodal Cholesky | 4–8 weeks to linear static, thermal and modal | One language and toolchain; deterministic; we own the verification. |
| **B. MFEM linked in process** (Nightfall's `physics.cpp` behind a small C shim) | BSD-3-Clause | Yes (desktop) | Not planned | Proven | Good | 2–4 weeks | Adds a C++ toolchain to Polykrom's desktop build. The broadest physics later (Tribol for contact is MIT). |
| **C. MFEM as a separate program** | BSD-3-Clause | No | No | Proven | File round trip | 2–3 weeks | Today's model without Nightfall. |
| D. fenris (Rust FEM) | Apache-2.0 | Yes | Likely | – | – | – | No commits since 2023-09; learn from only. |
| E. gemlab / pmsim / russell (Rust, cpmech) | MIT | Yes | Partly | – | – | – | Active, geomechanics focus; russell's sparse solvers wrap C libraries. Learn from its element and integration tables. |

Volume meshing:

| Option | Licence | In process | Notes |
|---|---|---|---|
| fTetWild as a separate program | MPL-2.0, needs GMP (LGPL-3.0-or-later or GPL-2.0-or-later) | No | Robust tetrahedra from PolyCore's surface mesh. MPL-2.0 alone would allow linking unmodified files, but GMP is LGPL, which Polykrom never links (ENGINES.md principle 2). |
| Our own Rust tetrahedral mesher | ours | Yes | Delaunay refinement with boundary recovery from the published methods (TetGen's code is AGPL and not read). Later, when in-process meshing matters for speed or the browser. |

### Recommendation

1. **Physics: option A.** A Rust-native solver in Polykrom (`crates/study`), in process and in
   the browser, with faer's sparse Cholesky on every platform and Accelerate optional on macOS.
   Nightfall's MFEM physics and its cantilever and thermal checks are the reference it must match.
2. **Meshing: fTetWild as a separate program now**, fed by PolyCore's analysis mesh, with face
   tags carried to the boundary triangles; our own Rust mesher later.
3. **Keep option C for what A doesn't cover** (frictional contact, plasticity, large
   deformation), as a separate MFEM program, when a demonstration needs it. If A misses its
   gate, B is the fallback.

Gate before `analysis.static_stress` switches: the cantilever and the thermal slab within 2 % of
their analytic values on three refinements; NAFEMS LE1 and LE10 within their published
tolerances; manufactured solutions converge at the expected order for P2 elements; the P2
surface field's maximum equals the reported maximum to 1e-9.

## Interchange

| Format | Direction | Owner | Plan |
|---|---|---|---|
| STEP AP203/AP214 | read, write | `polycore-io` | Import is ours today (`step_in/`); export moves off truck-stepio in Phase 1. AP242 B-rep next. Checked on Polykrom's `tests/step-conformance` and `tests/step-regressions` and on the open [test models](#test-models), with the OCCT reference program reading our files back as a cross-check. |
| IGES | read, write | `polycore-io` | As today. |
| Native schema 3 | read, write | `polycore-io` | PolyCore's persistent body format inside Polykrom designs. A change bumps the schema; old versions still read. |
| USD `omniSolid` | read, write | `polycore-io` | `BrepArray` prims with analytic and NURBS faces, as the schema defines (Apache-2.0). Our own `.usda` reader and writer. Phase 3. |
| USD meshes and UsdPhysics | write | Polykrom, with PolyCore meshes and inertia | For robot export (ENGINES.md S2). |
| 3MF | write; read as meshes | `polycore-io` for meshes and face tags; Polykrom's `solidcraft-io` for the package | As today ([3mf-face-tags.md](https://github.com/wckdboy/polykrom/blob/main/docs/formats/3mf-face-tags.md)), on PolyCore's watertight meshes. |
| MJCF, URDF | write | Polykrom | Assembly formats stay in Polykrom. PolyCore supplies watertight meshes, exact mass, centre of mass and inertia, and contact primitives. Fixes #216 in Phase 1. |

## Risks and honest cost

### Why this is hard

- Pure-Rust kernels have stalled. Fornjot was archived with its goals unreached. Truck's
  booleans are mesh-seeded and its fillets weak. solidcore has 116 of 261 units identical to
  the C++, and has not yet built general solids end to end.
- The hard parts are known: booleans with tangent and coincident geometry, free-form
  surface–surface intersection, blends where they meet at vertices or roll over an edge,
  and offsets that change topology. Commercial kernels took decades on these.
- Our own kernel already carries about ten special-case modules and a retry ladder. Agents
  that fix symptoms one case at a time will grow more of them.
- Two kernels live side by side until Phase 6, and two repositories from Phase 0. That
  doubles some maintenance.
- "Fastest" is a goal, not a fact. Nothing here has been timed against OCCT on this corpus yet;
  Phase 0 records that number.

### What makes this attempt different

- **A corpus with answers.** 382 regression cases, 116 ladder cases, 83 reference tests, 119
  SSI cases, 20 bake-off parts, nearly all with analytic or invariant answers, plus an OCCT
  program that scores without being trusted. Fornjot had no such harness.
- **Incremental replacement.** The product ships on every phase. A phase that misses its gate
  stops; nothing already shipped is lost.
- **A complete, readable reference.** SMLib and solidcore are Apache-2.0: a full kernel we can
  read and port, with attribution. Earlier Rust efforts had none they could legally read.
- **Published methods.** Local tolerances, general fuse, procedural intersection curves,
  rolling-ball and setback blends: all documented, and the blend patents found have expired.
- **Agent throughput.** 18 kernel PRs merged in the last week; PRs #189 to #236 were opened in
  about 27 hours. The gates turn that speed into measured progress instead of more special cases.

### Risks

| Risk | Mitigation |
|---|---|
| Special cases keep growing | A special-case module needs a recorded reason; Phase 2 deletes the existing ones as their cases pass. The ladder frontier, not the latest bug, picks the next work. |
| A phase stalls | Each phase is useful alone: Phase 1 already fixes #216 and the MJCF masses. The corpus shows the stall early. |
| Two repositories drift | Polykrom pins a revision; PolyCore CI runs the cases through a pinned Polykrom CLI; a change that spans both lands PolyCore first, then the bump. |
| An accidental read of OCCT or other LGPL code | The sources table; reviewers check where an algorithm came from; usd-brep's OCCT converters are named as off limits. |
| Designs change meaning when topology changes | Persistent names follow history, and the naming tests run in every phase. Schema 3 is versioned. |
| The reference program rots | A pinned commit, a recorded recipe and a recorded binary hash; Polykrom's FreeCAD oracle is a second black box for volume and area. |
| Polykrom drifts from upstream SolidCraft, which is moving to solidcore | Upstream merges outside the kernel keep working. Upstream kernel changes (paused there) are ported to PolyCore by hand. Schema 3 lets bodies cross between the kernels; contributions to solidcore keep the projects close. |
| Agent runs vary | TinyRex is judged on the median of three runs. |
| The Rust study code is less precise than MFEM | It must match Nightfall's MFEM results and pass the NAFEMS and manufactured-solution checks before it replaces `nf`; MFEM stays available as a separate program. |

### Effort

From ENGINES.md, in engineering-months with AI assistance:

| Phase | Effort |
|---|---|
| 0 Extraction and harness | 1–2 |
| 1 Geometry and topology | 2–4 |
| 2 Booleans | 4–8 |
| 3 Free-form SSI, sweeps, lofts | 6–12 |
| 4 Blends | 6–12 |
| 5 Offsets and local operations | 6–12 |
| 6 Parity and speed | ongoing |
| Study program (Polykrom, in parallel) | 1–2 |

Roughly 25 to 50 engineering-months to reach OCCT's score on our corpus. Phases 3 to 5 overlap,
so calendar time is shorter than the sum. Rivalling the commercial kernels on blends, offsets
and free-form corner cases is a multi-year effort, corpus-driven, and this plan does not
claim otherwise.

## Naming

- **Polykrom** is the product: the app, the CLI, the MCP server.
- **PolyCore** is the kernel: the wckdboy/polycore repository and its `polycore` and
  `polycore-*` crates. Polykrom's own crates keep their `solidcraft-*` names (Polykrom's
  AGENTS.md: not renamed piecemeal).
- **TinyRex** is the flagship robot and its repository, wckdboy/tinyrex.

## Decisions

Made by the owner on 2026-10-26. Where a later decision overrides an earlier one, only the
final one is listed.

| # | Topic | Decision |
|---|---|---|
| 1 | Where PolyCore lives | Its own repository and workspace, wckdboy/polycore, closely tied to Polykrom. Created only on the owner's explicit OK. Phase 0 extracts `crates/kernel` and the vendored truck into it. Polykrom consumes it as a git dependency pinned by revision, with an untracked `[patch]` to a local checkout for joint development. PolyCore CI runs the kernel cases and the ladder; Polykrom CI pins and tests a PolyCore revision. |
| 2 | solidcore | A reference to port from, watched for improvements. Any unit that wins on our corpus is ported. Anything we build that helps it goes upstream to tangency-labs/solidcore as PRs; bugs we can't fix go as reports. At the end of Phase 1, adopting it wholesale is re-evaluated on our corpus. |
| 3 | The bar | OCCT. The goal is an open-source kernel that rivals the commercial kernels. |
| 4 | OCCT reference program | A frozen, pinned Nightfall `nf` build used as a tool, since it reports operation history. Commit and recipe under [The OCCT reference program](#the-occt-reference-program). |
| 5 | Model tolerance | 1e-6 mm, fixed in Phase 1 after measuring the corpus. |
| 6 | Bake-off targets | As listed under [The bake-off against OCCT](#the-bake-off-against-occt). |
| 7 | Test models | No models made in commercial CAD; ABC, Fusion 360 Gallery and NIST are out. Our own cases, plus existing high-quality open models with permissive licences made in open tools ([Test models](#test-models)). |
| 8 | TinyRex | Its own repository, wckdboy/tinyrex, referenced by PolyCore and Polykrom. The gate points there: rebuilt from scratch by the agent on every phase, judged on the median of three runs. |
| 9 | Study program | Lives in Polykrom, integrated as natively as licences allow; PolyCore supplies analysis-ready geometry. Recommended: Rust-native P2 FEA with faer in process, fTetWild as a separate program (GMP prevents linking it), MFEM as a separate program for heavier physics. Netgen is dropped. ([The study program](#the-study-program).) |
| 10 | Licences | PolyCore is Apache-2.0, with `NOTICE` entries for anything ported from SMLib or solidcore. Polykrom stays MIT OR Apache-2.0. |
| 11 | Nightfall | Archived now (2026-10-26). Another agent handles the notices. The harvest reads from the archive. |
