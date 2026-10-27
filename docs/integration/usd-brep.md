# USD BRep integration

- **Project:** [NVIDIA-Omniverse/usd-brep](https://github.com/NVIDIA-Omniverse/usd-brep)
- **Reviewed release:** 1.0.1, 2026-10-21. Initial public release 1.0.0, 2026-10-08.
- **Role:** Omniverse/OpenUSD interoperability reference, external validator, and optional
  benchmark backend. It is not an in-process dependency of PolyCore.
- **PolyCore direction:** [RFC 0002](../rfcs/0002-ai-native-engineering-platform.md).

## Why it matters

USD BRep contains the complete SMLib NURBS B-rep kernel, a C-style `SM_API`, exact
`omniSolid`/`BrepArray` import/export, topology and geometry validators, tessellation, healing,
and agent-facing documentation. Plane, cylinder, cone, sphere and torus faces are written
analytically; other geometry is written as NURBS. It is the strongest open implementation for
testing exact-solid USD interchange and Omniverse workflows.

The first release is also a useful independent kernel benchmark: its booleans, fillets, sweeps,
shells and mass properties can be compared with PolyCore and the frozen OCCT reference program
without making any of them the answer key.

## Licence and provenance

- NVIDIA-authored source is Apache-2.0.
- The distributed OpenUSD runtime is OpenUSD 25.11 under the Tomorrow Open Source Technology
  License 1.0: Apache-2.0-based, with different trademark terms.
- The bundled oneTBB, hwloc, MaterialX, OpenSubdiv, CPython, HACL*, pybind11 and validator
  components have their own notices.
- The `omniSolid` proposal and preliminary schema are AOUSD work under the OpenUSD technical
  proposal supplemental terms.
- Any optional SMLib algorithm ported into PolyCore requires an explicit NVIDIA copyright line,
  NOTICE entry and provenance review. Reading Apache-2.0 source is permitted; linking or copying
  follows the normal adoption gate.

## Status and risks

1.0.1 fixed severe issues present in 1.0.0, including boolean use-after-free and crashes,
nondeterministic shell results, fillet crashes/hangs, missing mesh validity, malformed-input
reads, and several leaks. This is a young release, not a claim that the kernel is defect-free.

Current constraints:

- Upstream does not accept pull requests; fixes depend on NVIDIA's release cadence or local
  patches that we would have to maintain.
- The Python wheel is CPython 3.12 on Linux/Windows; macOS is a source build.
- `omniSolid` is preliminary and may change.
- The kernel exposes process-global context state and mutable handle semantics; wrapping it
  in-process would add unsafe bindings and ownership hazards.
- Asset authoring guidance recommends staying in-process and emitting `UsdGeom.Mesh`; exact
  `BrepArray` round trips are for interchange, not every intermediate operation.

## Integration modes

### Phase A: external validator and benchmark (first)

- Pin source revision, build recipe, and binary/wheel SHA-256 in `bench/`.
- Run it out of process only.
- Generate PolyCore STEP and USD fixtures; import into USD BRep; export again.
- Validate `BrepArray` with its schema and geometry validators.
- Compare volume, area, centroid, inertia, topology counts, tessellation closure, determinism,
  success and time against PolyCore and OCCT.
- Run malformed and hostile USD cases under a timeout and record crashes.

### Phase B: PolyCore-owned USD interchange

- Write and parse the pinned `omniSolid` snapshot with PolyCore's own deterministic code.
- Validate every file with external OpenUSD/usd-brep in CI.
- Preserve analytic and NURBS surfaces, topology, face materials, transforms and instancing.
- Refuse unsupported schema versions rather than guessing.
- Keep meshes and USD physics as interoperable derivatives, with a back-reference to the exact
  body in PolyCore metadata where practical.

### Phase C: optional backend

An in-process C++/C wrapper is considered only if all gates pass:

- corpus success and correctness beat or complement Truck/PolyCore;
- no crashes, memory errors, hangs or nondeterminism in repeated hostile runs;
- stable pinned API and a maintainable patch path;
- safe Rust wrapper review with no `unsafe` outside the approved boundary;
- wasm and desktop fallback behaviour proved;
- licence/NOTICE reviewed.

Otherwise it remains a benchmark and converter.

## Fixture gate

All fixtures are generated from PolyCore's own analytic cases or authored by us; no
commercial-CAD models.

| Fixture | Checks |
|---|---|
| Box, cylinder, sphere, torus | Analytic area/volume/inertia; analytic USD surface types; topology validity |
| Box minus offset cylinder | Boolean topology, closed shells, exact volume, p-curves, re-import |
| Plate with several fillets | NURBS/analytic face mix, curvature continuity, face history, tessellation |
| Shelled enclosure with open faces | Shell topology, orientation, outer/inner shells, watertight mesh |
| Swept/lofted tube | Seams, periodic surfaces, trim loops, volume and area |
| Multi-body assembly | Instancing, transforms, face materials, stable prim paths |
| Non-manifold contact case | Explicit regions and radial-edge topology, or a documented refusal |
| Malformed arrays | Bounds, indices, orders, knots, NaNs; clean error, no crash/hang |

For each fixture record: source command/hash, schema snapshot, validity, counts, volume, area,
centroid, inertia, closed-mesh result, both hashes, time and validator output.

## Decision log

- **2026-10-27:** accept as a primary external USD validator and benchmark candidate; do not link
  or vendor it in the default kernel yet.
