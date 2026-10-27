# RFC 0002: an AI-native engineering platform

- **Status:** Accepted owner direction, 2026-10-27.
- **Extends:** [RFC 0001](0001-polycore.md). RFC 0001's extraction, determinism, evidence,
  licence and delivery gates still hold. Where this RFC broadens ownership (simulation and
  interchange adapters), it replaces only the narrower scope statements.
- **Owner goal:** the best optimized open-source kernel for CAD, simulation, CFD, robotics,
  MuJoCo, USD and NVIDIA Omniverse, controllable by people and AI agents, from TinyRex-sized
  projects to complete assembly lines and advanced products.

## Summary

PolyCore is one logical engineering kernel with several measured backends. It owns exact
geometry, topology, naming provenance, tessellation, mass properties and the data needed by
analysis and simulation. Established projects are reused where they are strong and can be used
under compatible licences; none is allowed to become an unmeasured or hidden second kernel.

The stable product boundary is a deterministic Rust API. Polykrom remains the collaboration
surface where people, agents, documents, parameters, assemblies, studies and workflows live.
PolyCore returns values, history, capabilities, diagnostics and evidence.

Heavy, unsafe, copyleft or non-Rust engines run as versioned separate programs behind adapters.
Permissive native Rust code is preferred when it passes the same corpus. Interchange with
Omniverse and simulation tools uses exact USD, STEP, mesh, MJCF, URDF and SDFormat files; it does
not require an unstable C ABI.

## Non-negotiables

1. **Open source first.** PolyCore's own code is Apache-2.0. Native dependencies must be
   permissive (MIT, Apache-2.0, BSD, ISC, Zlib, MPL where linking is legally reviewed). GPL,
   LGPL and AGPL projects may run only as separate programs whose source is not read, copied or
   linked.
2. **One kernel, explicit backends.** `CompositeKernel` may route operation families to
   implementations, but every result records the backend and build identity. There is no silent
   fallback and no second geometry truth.
3. **Evidence, not bare answers.** Every operation returns achieved tolerance, validity,
   history, backend, time, budget use and any fits or convergence data. Every refusal names its
   kind, stage, entities and a useful message.
4. **Same input, same output.** CPU results are bit-identical across thread counts. GPU results
   preserve topology and history exactly and coordinates stay within the model resolution.
   Cache keys include backend identity, version, features, units and tolerances.
5. **Never crash.** Production code does not panic on hostile geometry or input-derived values.
   Third-party calls are guarded; sidecars have a watchdog, timeout and cancellation.
6. **AI operable, not AI inside.** No model is required by the kernel. The API exposes typed
   capabilities, selectors, budgets, progress, structured failures and evidence that agents can
   reason about deterministically.
7. **One source of truth for units.** Geometry is millimetres, radians and Z-up. Simulation and
   interchange boundaries convert explicitly from a recorded input unit; no implicit conversion
   is allowed.

## Architecture

```
Polykrom
  document, parameters, features, assemblies, joints, materials, studies,
  commands, MCP, agents, UI, undo, review and collaboration
        │ typed values and operation specs; results + history + evidence
        ▼
PolyCore facade
  Kernel + Study traits, capabilities, selectors, budgets, cancellation,
  composite backend routing, content hashes, cache identity
        │
        ├── geometry: math → geom → topo → (ops, mesh, naming, props)
        ├── analysis: tagged meshes, fields, contacts, units, error bounds
        ├── interchange: native schema, STEP, IGES, 3MF, glTF, USD/BrepArray
        ├── robotics: MJCF, URDF, SDFormat, exact inertia/contact data
        ├── simulation: FEA/CFD/dynamics study contracts and evidence
        ├── adapters: MuJoCo, Drake, Gazebo, MFEM, SU2/OpenFOAM, fTetWild/MMG
        └── legacy: Truck behind capability-scoped adapters until removed
```

### Workspace targets

| Crate or component | Responsibility | Status |
|---|---|---|
| `crates/polycore` (`solidcraft-kernel`) | Facade, API contract, current Truck-backed operations | Extracted |
| `crates/solidcraft-geom` | Math, meshes, B-spline and native schema foundations | Extracted |
| `polycore-math`, `-geom`, `-topo`, `-ops` | First-class geometry and topology replacement | Phase 1 |
| `polycore-mesh` | Watertight tessellation, analysis surface meshes, BVHs | Phase 1 |
| `polycore-naming`, `-props` | Exact history/identity and B-rep mass properties | Phases 1–2 |
| `polycore-io` | STEP/IGES/native, glTF/3MF, USD `omniSolid` and UsdPhysics | Phases 1–3 |
| `polycore-robotics` | Robot model assembly, exact inertia, contacts, actuators | Phase 1 |
| `polycore-study` | Study graph, units, materials, convergence/error evidence | Phase 1 |
| `polycore-fea` | Native Rust P2 linear static/thermal/modal, faer solvers | Phases 1–2 |
| `polycore-cfd` | CFD geometry, boundary preparation, adapter and convergence contracts | Phases 2–3 |
| `polycore-legacy` | Every remaining Truck operation; deleted in Phase 6 | Bridge |
| `polycore-bench` | Corpus, OCCT/engine bake-offs, minimiser, determinism and scoring | Started |
| `polycore-agent` | Task packets, reference runner, evidence and cost/eval harness | Phase 0 |

Non-Rust engines live under `sidecars/<engine>/` with a pinned revision, build recipe, binary
hash, machine-readable protocol, timeout and licence record. They never become build
requirements for the browser or the default desktop kernel.

## Kernel contract

The names may change before Phase 2; the semantics do not.

```rust
pub trait Kernel: Send + Sync {
    fn identity(&self) -> &KernelId;
    fn capabilities(&self) -> &Capabilities;
    fn primitive(&self, cx: &Cx, spec: &Primitive) -> Outcome<Body>;
    fn extrude(&self, cx: &Cx, profile: &Profile, spec: &Extrude) -> Outcome<Vec<Body>>;
    fn revolve(&self, cx: &Cx, profile: &Profile, spec: &Revolve) -> Outcome<Vec<Body>>;
    fn sweep(&self, cx: &Cx, spec: &Sweep) -> Outcome<Vec<Body>>;
    fn loft(&self, cx: &Cx, spec: &Loft) -> Outcome<Vec<Body>>;
    fn boolean(&self, cx: &Cx, op: BoolOp, targets: &[Body], tools: &[Body]) -> Outcome<Vec<Body>>;
    fn blend(&self, cx: &Cx, body: &Body, spec: &Blend) -> Outcome<Body>;
    fn offset(&self, cx: &Cx, body: &Body, spec: &Offset) -> Outcome<Body>;
    fn split(&self, cx: &Cx, body: &Body, tool: &SplitTool) -> Outcome<Vec<Body>>;
    fn check(&self, body: &Body) -> Validity;
    fn mass(&self, cx: &Cx, body: &Body) -> Outcome<MassProps>;
    fn tessellate(&self, cx: &Cx, body: &Body, spec: &Tessellation) -> Outcome<Mesh>;
    fn analysis_mesh(&self, cx: &Cx, body: &Body, spec: &AnalysisMesh) -> Outcome<TaggedMesh>;
    fn read(&self, cx: &Cx, format: Format, bytes: &[u8]) -> Outcome<Vec<Body>>;
    fn write(&self, cx: &Cx, format: Format, bodies: &[Body]) -> Outcome<Vec<u8>>;
}

pub struct Done<T> {
    pub value: T,
    pub history: History,
    pub evidence: Evidence,
}

pub enum RefusalKind { Unsupported, InvalidInput, Failed, OutOfBudget, Cancelled, Internal }
```

`Cx` carries deadlines, attempt and memory budgets, thread budget, progress/cancellation,
determinism mode and a trace sink. Bodies are immutable, `Send + Sync`, content-addressed and
cheap to clone. Selectors use durable history plus geometric predicates and state expected
cardinality; an index alone is never an agent-facing durable reference.

### Capabilities and routing

Capabilities are versioned data, not documentation claims: surface and curve types, operations,
maximum sizes, history guarantees, tessellation guarantees, GPU requirements, supported formats
and known limitations. `CompositeKernel` routes by an explicit policy:

1. `Exact`: require the first backend that can guarantee the requested operation.
2. `Prefer(id)`: try one backend, then a named ordered fallback list; every attempt is in the
   evidence.
3. `BenchmarkWinner`: choose from a pinned scorecard for that build and corpus slice.
4. `EvaluateAll`: bench-only; compare all implementations and never mutate a design.

No policy may fall back after returning partial geometry. A refusal is atomic.

### Entity references

Agent and document references combine:

- history origin and feature lineage;
- stable content hash where appropriate;
- geometric predicates (`type`, `normal`, `near`, `radius`, `convex`, area/length bounds);
- a required cardinality (`one`, `at_least(N)`, exact range).

Wrong cardinality is a structured error with candidates. This avoids brittle raw face indices
and lets agents recover without blindly guessing.

## Backend strategy

Licences were checked from project metadata and licence files. A table entry is not permission
to copy code.

| Project | Licence | PolyCore role | Initial integration |
|---|---|---|---|
| Truck | Apache-2.0 | Existing B-rep, meshing and booleans | Vendored bridge; replaced layer by layer |
| SMLib / USD BRep | Apache-2.0 | Complete NURBS kernel, OpenUSD B-rep, interoperability reference | Sidecar/evaluation first; port or link only after corpus, safety and provenance gates |
| solidcore | Apache-2.0 | Safe-Rust SMLib port; source of proven units | Watch, benchmark, port winning units with attribution |
| Axom `primal` | BSD-3-Clause | Predicates, trimmed-NURBS winding numbers, quadrature, exact props | Benchmark; port documented algorithms or use a safe sidecar |
| Manifold | Apache-2.0 | Guaranteed-manifold mesh booleans and repair | Mesh-only fallback/sidecar after validation |
| OpenCASCADE | LGPL-2.1 | Frozen external reference/benchmark | Separate process only; never link/read |
| MFEM | BSD-3-Clause | FEM verification and heavy multiphysics | Sidecar first; native Rust solver is the default path |
| faer | MIT | Pure-Rust sparse/dense linear algebra | Native dependency |
| MuJoCo | Apache-2.0 | High-fidelity articulated dynamics/contact | MJCF + separate process; optional safe binding later |
| Rapier | Apache-2.0 | Fast in-process real-time dynamics/contacts | Optional native feature after accuracy gates |
| Drake | BSD-3-Clause | Independent robotics verification | Separate process/URDF-SDFormat adapter |
| SDFormat / Gazebo | Apache-2.0 | Robot/environment interchange and simulation | File writer + sidecar adapters |
| OpenUSD / UsdPhysics | Tomorrow Open Source Technology 1.0 (Apache-2.0-based; trademark terms differ) | Scene and physics interchange | Own Rust reader/writer; external validators |
| SU2 | LGPL-2.1 | Established RANS/CFD cross-check and production sidecar | Separate process |
| OpenFOAM | GPL-3.0 | Broad CFD production sidecar | Separate process only |
| Nalu-Wind / Nalu | BSD-3-Clause | Permissive unstructured low-Mach/LES CFD candidate | Evaluate as sidecar |
| fTetWild | MPL-2.0 (needs GMP) | Robust volume meshing | Separate process |
| MMG / Netgen | LGPL | Adaptation/volume meshing | Separate process |
| IHP/TetGen/Gmsh | Reciprocal/GPL/AGPL variants | Rejected for linked use | At most external tools with owner approval |

Before adoption, every candidate gets: licence confirmation, provenance review, deterministic
build, checksum, corpus results, memory/failure tests, determinism result and removal cost.

## Simulation and CFD

Solvers are not allowed to invent their own geometry truth. PolyCore supplies:

- watertight, consistently oriented, face/edge-tagged surface meshes;
- exact volume, centroid, inertia and principal axes from the B-rep;
- recognised analytic contact primitives and convex collision proxies;
- named boundaries, materials regions and unit conversions;
- mesh-quality, closure, self-intersection and convergence evidence.

The study contract owns specifications and results, not CAD parameters:

```rust
pub trait Study: Send + Sync {
    fn identity(&self) -> &StudyId;
    fn capabilities(&self) -> &StudyCapabilities;
    fn validate(&self, model: &StudyModel) -> Result<StudyReport, StudyRefusal>;
    fn solve(&self, cx: &Cx, model: &StudyModel, spec: &StudySpec) -> StudyOutcome;
}
```

Initial tracks:

| Track | First implementation | Verification |
|---|---|---|
| Linear static, modal, thermal | Native Rust P2 + faer | Analytic cases, NAFEMS LE1/LE10, manufactured solutions, three refinements, GCI |
| Articulated robots | MuJoCo sidecar | Watertight meshes, exact MJCF inertia, deterministic runner, Drake cross-check |
| Real-time mechanisms | Optional Rapier | Closed-form mechanisms and MuJoCo comparison before UI use |
| Low-Mach/RANS CFD | SU2/OpenFOAM sidecar; evaluate Nalu | Lid-driven cavity, backward-facing step, airfoil polar, mesh convergence, invariants |
| Volume meshing | fTetWild/MMG sidecars, later Rust mesher | Boundary recovery, positive volumes, feature preservation, quality |
| Omniverse/digital twin | USD B-rep + meshes + UsdPhysics | OpenUSD validator, usd-brep readback, round-trip hashes |
| Assembly lines/robotics scenes | MJCF/URDF/SDFormat + USD | Joint/actuator/contact/eval checks; deterministic replay |

Every result must include assumptions, code-verification coverage, solution verification,
sanity checks, singularities and a trust level. CFD/FEA results without evidence are incomplete.

## USD and Omniverse

PolyCore writes and reads exact `omniSolid` `BrepArray` data, meshes, transforms, instancing,
materials and physics through its own deterministic writers. OpenUSD and usd-brep run as separate
validators/converters in CI. A live Omniverse connector, hydra delegate or Kit extension is an
adapter outside the kernel and communicates through USD files or a versioned process boundary.

The `omniSolid` schema is still preliminary (`prelimUsdSolid`, proposal snapshot
`811f03c603b06d3b175515a4bf991dee8f42f651`) and must be pinned and version-gated. USD BRep 1.0.1
is Apache-2.0 for NVIDIA-authored code but bundles OpenUSD under the Apache-based Tomorrow Open
Source Technology License 1.0. Its first patch release fixed crashes, hangs, leaks and
nondeterministic shells, and upstream does not currently accept pull requests; adoption is
therefore sidecar-first, with all memory-safety and determinism evidence recorded.

This keeps the kernel usable on Linux/macOS/browser without a proprietary runtime while allowing
NVIDIA Omniverse users to load exact solids, not lossy approximations.

## Performance and scale

- Immutable arenas, stable indices, content hashes and operation caches.
- BVH broad phase, exact predicates, analytic intersections first, adaptive mesh fallback.
- Ordered parallel mapping; no output depends on worker completion order.
- Incremental assembly evaluation, instancing and shared meshes.
- Mesh LOD, streaming and out-of-core conversion for assembly lines.
- SIMD and GPU compute (wgpu baseline; optional CUDA backend) only when they pass the CPU
  reference within tolerance.
- Sidecars shard by part/region with deterministic merge; no floating-point reduction depends on
  shard completion.
- Performance gates report medians and tail latency against pinned corpora and hardware.

## Polykrom boundary

Polykrom keeps parameters, expressions, units, configurations, feature timeline, persistent names,
sketch and assembly constraints, joints, motion/study authoring, materials, commands, undo,
collaboration, MCP, agent sessions and UI. PolyCore receives explicit values and typed specs.

This prevents PolyCore from becoming an application framework and prevents Polykrom from owning a
second geometry implementation.

## AI-native operation

Polykrom's MCP and agent layer can:

- inspect capabilities and known limitations before attempting work;
- request operations with explicit units, budgets and selection cardinality;
- receive structured refusals, candidate entities and evidence;
- compare backend results and history;
- request simulation only when required evidence and mesh quality gates pass;
- replay a deterministic command sequence and reproduce hashes.

The kernel never reads prompts, credentials or user conversations. Mechanical CAD operations are
the same whether invoked by UI, script, CLI or agent.

## CI/CD

### Change tiers

| Tier | Trigger | Work |
|---|---|---|
| Fast | Every push | format, metadata/dependency boundary, affected check, docs links, script syntax |
| Review | Pull request | affected Clippy/tests, wasm compile, render/export smoke, security/source audit |
| Merge | Merge queue | build once, full native test archive, four shards, wasm, corpus smoke, reference tests |
| Phase | Closing PR | full corpus, ladder, determinism, bake-off, TinyRex median, machine evaluations |
| Nightly | Schedule | property tests, fuzz seeds, full corpus both thread modes, OCCT/engine bake-off, dependency audit |
| Hardware | Nightly/manual | Linux NVIDIA GPU, large CFD/FEA, Omniverse validation, Windows/macOS matrix |

- Build artifacts are content-addressed and uploaded once; test shards download the same archive.
- Corpus shards are keyed by case hash and backend identity; unchanged pinned runs reuse results.
- Pull requests do not rebuild unchanged dependencies or re-record timings.
- Full gates run on merge/nightly, not on every draft push.
- Merge queue groups compatible PRs but never weakens phase gates.
- CI reports time, cache hits, shard balance and cost per green change.

### Test evidence

A change is complete only with the smallest applicable set:

1. analytic unit/regression test proving the changed behavior;
2. affected integration tests and deterministic repeat;
3. full corpus/no-regression for kernel behavior;
4. bake-off/determinism for backend or performance changes;
5. simulation verification and cross-engine comparison for solver/adapter changes;
6. exact round-trip and external validator for interoperability changes;
7. TinyRex/application build evidence when the consumer boundary changes.

## Multi-agent orchestration

Agents are coordinated as a dependency graph of bounded tasks, not free-running editors on one
checkout.

### Task packet

Every task records: objective, owned paths, forbidden paths, dependencies, acceptance tests,
budget (attempts, wall time, tokens/cost), model tier, evidence required, risk and rollback.

### Roles

| Role | Scope |
|---|---|
| Coordinator | Splits tasks, resolves dependencies, prevents overlap, owns integration windows |
| Explorer | Read-only repository/code mapping and cache maintenance |
| Geometry | Math, topology, operations and exact history |
| Verification | Corpus, minimisation, determinism, fuzz and analytic oracles |
| Simulation | FEA/CFD/multibody verification and adapters |
| Interchange | USD/STEP/robot formats and external validators |
| Application | Polykrom commands, UI/MCP and end-to-end evidence |
| Reviewer | Independent code/licence/security review; never the implementer |

Each agent works in its own worktree and owns a draft PR/claim. Agents do not edit another
agent's owned paths. The coordinator merges only green dependency nodes. CI reacts to labels and
status events; agents do not poll repositories or re-run unchanged failures.

### Model routing

- Deterministic scripts and reference agents handle setup, reproduction, formatting, corpus
  collection and comparisons.
- Small models handle search, triage, summaries, issue routing and mechanical edits.
- Strong models handle architecture, numerical methods, difficult failures, integration and
  final review.
- A stronger model is invoked only with a compact failure packet: command, seed/case, relevant
  diff, logs, attempts already tried and budget remaining.

### Token and cost control

Optimize **cost per verified merge**, not raw tokens:

- local builds/tests and static tools before model calls;
- cached repository map and task state, not repeated full-file context;
- API prompt caching and resumable sessions;
- structured diffs, logs and test summaries instead of dumps;
- parallel independent tasks; serialize shared files;
- bounded retries with no duplicate unchanged CI run;
- expensive simulations and full-corpus runs only after cheap gates;
- phase evaluations use the specified sample count (for example TinyRex median of three), not
  repeated ad hoc runs;
- track tokens, dollars, wall time, turns, CI minutes and cache hits per merged change;
- review dashboard for duplicate work, failure loops and expensive low-value tasks.

## Delivery plan

| Stage | Deliverable | Gate |
|---|---|---|
| 0A | Extraction, standalone workspace, dependency closure, current CI | Completed locally; pending consumer pin |
| 0B | Polykrom consumes pinned PolyCore; contract types; smoke corpus; baseline reports | Polykrom CI green, reference tests unchanged, wasm green |
| 0C | Exact history selectors, backend identity, bridge/refusal evidence; CI cost tiers | Contract tests, no silent fallback, measured CI/cache |
| 1 | Own geometry/topology/tessellation/exact mass; analysis meshes; native FEA solver; MJCF exact inertia | Phase 1 RFC 0001 gates plus simulation analytic suite |
| 2 | General-fuse booleans, analytic SSI, contact primitives, initial USD B-rep | Booleans ≥ target, determinism, MuJoCo/Drake checks |
| 3 | Free-form SSI, sweeps/lofts, STEP/USD breadth, CFD adapters | SSI and interchange gates |
| 4 | General blends and vertex blends | Fillet gate and bake-off target |
| 5 | Offsets, shell, thicken, local operations, mid-surfaces | Shell/offset gates |
| 6 | OCCT parity and speed, all legacy/sidecar defaults retired | ≥100 % OCCT score, performance target, all product gates |

Stages may overlap only when they touch independent operations and the integration graph shows
no blocking ownership conflict.

## Decisions

1. **PolyCore is one logical kernel with explicit measured backends**, not a monolith and not
   multiple silent kernels.
2. **Open source is the first filter; correctness, speed and evidence decide adoption.**
3. **Native Rust owns the stable API and default deterministic paths.** Non-Rust and copyleft
   engines are sidecars unless a safe, licence-compatible, measured integration is approved.
4. **SMLib/USD BRep and solidcore are strongly evaluated**, not blindly adopted: the corpus,
   safety record, provenance and performance decide.
5. **Simulation and CFD adapters belong to PolyCore; study authoring and product workflows
   belong to Polykrom.**
6. **USD is the Omniverse boundary**, via exact own-written `omniSolid` data with external
   validators.
7. **AI-native means typed, introspectable, reproducible operations**, not a model inside the
   geometry kernel.
8. **CI/CD and agent orchestration optimize cost per verified engineering outcome.**
