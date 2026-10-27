# Reuse survey: open-source code that shortens PolyCore's path to OCCT

- **Date:** 2026-10-27
- **Author:** Claude (agent), for the owner
- **Question:** which open-source code are we allowed to reuse that would shorten the time for
  PolyCore to match OpenCASCADE on its weak spots: robust booleans and general NURBS
  surface–surface intersection (SSI), blends, offsets and shells of free-form surfaces,
  healing, STEP import breadth, and exact mass properties.
- **Out of scope:** SMLib and tangency-labs/solidcore, evaluated separately under
  [RFC 0001](../rfcs/0001-polycore.md#solidcore-a-standing-practice).

## How licences were checked

Every licence below was read on 2026-10-27 from the repository's own licence file (through the
GitHub contents and licence APIs), not from a summary. Where a project has no licence file, that
is said. Usable means MIT, BSD, Apache-2.0, Zlib, ISC, MPL-2.0 (file-level) or public domain.
GPL, LGPL and AGPL projects are rejected: only their licence file was opened, never their source.
That matches RFC 0001's rule that no OCCT, FreeCAD or other GPL/LGPL source is read.

Effort is engineer time with AI assistance, the unit RFC 0001 uses: **Low** is under 2 weeks,
**Medium** 2–6 weeks, **High** over 6 weeks. Status is the last push to the default branch.

## Findings in brief

- **No permissive project gives us an OCCT-class kernel to adopt as is.** The closest is
  **Remus**, a new Apache-2.0 Rust kernel with General Fuse booleans on NURBS, walking fillets,
  offsets, healing and STEP. It is very large and written by AI agents. Its architecture follows
  OCCT's, and nothing we found says whether OCCT source was used to build it. Bench it now. Port
  from it only after a provenance review.
- **The best clean, peer-reviewed code is LLNL's Axom** (BSD-3-Clause). It has generalised
  winding numbers for trimmed NURBS and quadrature on trimmed surfaces, which give point
  classification without a watertight model and mass properties with an error bound. It is
  C++ to port, and it has tests and published papers behind it.
- **monstertruck**, a maintained truck fork, has fixes and features that apply almost directly
  to our vendored truck: STEP parsing, seam healing, CDT trimming, and a fillet engine.
- **SSI has no permissive production code.** SISL and GoTools, the classic open SSI code, are
  AGPL. The two 2026 ACM TOG NURBS SSI papers have no code release that we could find. Phase 3
  is implemented from papers (listed below).
- **Mesh-level robustness is solved and permissive.** Manifold and its pure-Rust port give
  guaranteed-manifold mesh booleans. They are useful as a bench oracle, a mesh fallback and for
  repairing imported meshes, but not as the exact boolean.

## Top 10

| Rank | Candidate | Licence (verified) | What it gives PolyCore | Effort | Recommendation |
|---|---|---|---|---|---|
| 1 | [Remus](https://github.com/esaueng/remus) (Rust) | Apache-2.0 (`LICENSE-APACHE`; `NOTICE` credits brepkit and truck) | Every weak spot at once: a General Fuse boolean on analytic and NURBS surfaces with typed refusals, walking fillets and chamfers (variable radius and N-way vertex blends experimental), offset, thicken and shell by face–face intersection, a healing pipeline (wire, face and shell fixes, sewing, tolerance management), STEP that keeps analytic surfaces, and inertia | Low to bench, High to adopt | **Learn from and bench now; port parts only after a provenance review** |
| 2 | [Axom](https://github.com/llnl/axom) `primal` (C++, LLNL) | BSD-3-Clause (`LICENSE`, © LLNS) | Generalised winding numbers for trimmed NURBS curves and patches, with caching (robust inside/outside tests without watertightness); surface and volume integrals on trimmed patches (Gunderman et al.), so exact mass properties; NURBS patches with trimming curves; Bézier and patch intersections; Gregory patches | Medium | **Port parts** |
| 3 | [monstertruck](https://github.com/virtualritz/monstertruck) (Rust) | Apache-2.0 (`LICENSE`) | A truck fork that is a ready source of patches: Part 21 parser fixes (`step-p21`), `strip_seam_edges` healing, CDT trim fixes, STEP assembly output, offset curves and surfaces, a rewritten fillet engine (per-edge and variable radii, chamfers, profile modes) | Low | **Port parts** (cherry-pick into `vendor/polycore-truck-*`) |
| 4 | [step-io](https://github.com/elgar328/step-io) (Rust) | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | STEP import breadth: one universal schema reads every mainstream AP, its entity set was curated from more than 50,000 real files, non-standard input is healed with a report, and it writes AP242 edition 2 | Low–Medium | **Adopt** behind `polycore-io`, after a corpus check |
| 5 | [manifold-rust](https://github.com/larsbrubaker/manifold-rust) / [Manifold](https://github.com/elalish/manifold) | Apache-2.0 (`LICENSE`); manifold-rust depends on clipper2-rust, BSL-1.0 | Guaranteed-manifold mesh booleans in pure Rust, plus a second engine on exact rationals for non-manifold input. Uses: a volume oracle for the bench, a bounded mesh fallback, repair of imported meshes, booleans on analysis meshes | Low | **Adopt** for the bench and mesh-level operations, not as the exact boolean |
| 6 | [transfinite](https://github.com/salvipeter/transfinite) + [libgeom](https://github.com/salvipeter/libgeom) (C++) | MIT (`LICENSE`, © Peter Salvi) | N-sided patches (generalised Coons, midpoint, side- and corner-based, generalised Bézier) for vertex blends and setback corners, and for filling holes during healing | Low–Medium | **Port parts** |
| 7 | [curvo](https://github.com/mattatz/curvo) (Rust) | MIT (`LICENSE`) | NURBS on nalgebra: curve–curve and curve–surface intersection, boolean operations on regions bounded by 2D curves (useful for trim loops), offset and fillet curves, trimmed-surface tessellation | Low | **Port parts** |
| 8 | [G+Smo](https://github.com/gismo/gismo) (C++) | MPL-2.0 (`LICENSE.txt`) | Least-squares and THB-spline adaptive fitting (to fit procedural SSI curves, and offset and blend surfaces), Coons patches, parametrisation, trimmed surfaces | Medium | **Learn from**; port a fitting file only under MPL file-level terms |
| 9 | [geogram](https://github.com/BrunoLevy/geogram) (C++) | BSD-3-Clause (`LICENSE`, © Inria) for its own files; `third_party/` holds TetGen and Triangle, which are not permissive | Exact predicates with expansion arithmetic and symbolic perturbation, mesh–surface intersection and mesh CSG, mesh repair and hole filling | Medium | **Learn from**; never take `third_party/` |
| 10 | [cadmpeg](https://github.com/cadmpeg/cadmpeg) (Rust) | Apache-2.0 (`LICENSE`) | A second STEP reader and IGES codecs, so the same corpus can be read two ways and the results compared | Low | **Learn from; use in the bench** |

## Other candidates

| Candidate | Licence (verified) | Status | Relevance | Recommendation |
|---|---|---|---|---|
| [truck](https://github.com/ricosjp/truck) | Apache-2.0 (`LICENSE`) | Active (2026-10-26) | Our starting point. RFC 0001 already decides to replace it piece by piece. | As planned |
| [Fornjot](https://github.com/hannobraun/fornjot) | 0BSD (`LICENSE.md`) | Archived 2026-06 | `fj-math` (scalars and predicates over `robust`), `fj-export` (3MF, STL, OBJ), a catalogue of validation checks, and the `models/` test models already listed in RFC 0001. No booleans or blends worth taking. | Learn from; keep the test models |
| [nacre](https://github.com/elgar328/nacre) | MIT OR Apache-2.0 | Active, version 0.0.z | Exact-arithmetic kernel; planar booleans done, quadrics in progress. Shows how to keep decisions exact while caching floating-point coordinates. | Learn from |
| [vcad](https://github.com/ecto/vcad) | Apache-2.0 (`LICENSE`) | Active | About 35k lines of B-rep among more than 90 crates of very broad scope (fillets, shell, STEP via `stepperoni`). No evidence of quality on blends or SSI. | Skip |
| [verb](https://github.com/pboyer/verb) | MIT (`LICENSE`) | Quiet since 2025-04 | NURBS with an SSI seeded from meshes and refined, which is truck's weakness again. | Learn from (curve and surface algorithms only) |
| [Fidget](https://github.com/mkeeter/fidget) | MPL-2.0 (`LICENSE.txt`) | Active | Implicit surfaces with interval arithmetic and a JIT, plus dual-contouring meshing. A robust offset or shell preview as a distance field, never an exact result. | Learn from |
| [libfive](https://github.com/libfive/libfive) | MPL-2.0 for the library (no licence file; stated in the README and in each source header); Studio and the Guile bindings are GPL | Active | Older implicit kernel by the same author as Fidget. | Skip (Fidget supersedes it) |
| [libigl](https://github.com/libigl/libigl) | Core MPL-2.0 (`LICENSE.MPL2`, per-file headers); `include/igl/copyleft/` (CGAL, TetGen) GPL (`LICENSE.GPL`) | Active | Winding numbers and signed distance for meshes. Its exact mesh booleans need CGAL (GPL). | Learn from the core; never take `copyleft/` |
| [Interactive and Robust Mesh Booleans](https://github.com/gcherchi/InteractiveAndRobustMeshBooleans) (Cherchi et al. 2022) | MIT (`LICENSE`), but it bundles Attene's `Indirect_Predicates`, which is LGPL-2.1 | Maintenance paused | A robust mesh-arrangement boolean pipeline | Learn from the paper; skip the code |
| [cinolib](https://github.com/mlivesu/cinolib) | MIT | Active | General mesh library behind Cherchi's booleans | Skip |
| [PMP library](https://github.com/pmp-library/pmp-library) | MIT (`LICENSE.txt`) | Active | Mesh repair, remeshing, hole filling, for tessellations and analysis meshes | Learn from |
| [kurbo](https://github.com/linebender/kurbo) | MIT OR Apache-2.0 | Active | 2D Bézier offsets (Levien's parallel curves), robust 2D curve tools; helps sketch-level offsets, not surfaces | Learn from |
| [parry](https://github.com/dimforge/parry) | Apache-2.0 (`LICENSE`) | Active | Mass properties, BVH, distance and contact on meshes and convex shapes; covers analysis-ready contact geometry, not exact B-rep integrals | Learn from |
| [spade](https://github.com/Stoeoef/spade) | MIT OR Apache-2.0 | Active | Constrained Delaunay triangulation in Rust, for tessellating trimmed faces | Adopt if tessellation needs a CDT |
| [CDT](https://github.com/artem-ogre/CDT) | MPL-2.0 | Active | Constrained Delaunay triangulation in C++ | Skip (spade is Rust) |
| [STEPcode](https://github.com/stepcode/stepcode) | BSD-3-Clause (`COPYING`) | Active | EXPRESS parser and schema code generator, Part 21 reader; used by BRL-CAD | Learn from |
| [ruststep](https://github.com/ricosjp/ruststep) / [step-p21](https://github.com/virtualritz/step-p21) | Apache-2.0 (`LICENSE`) | ruststep quiet since 2025-03; step-p21 is monstertruck's fork with the escape fixes | What truck-stepio uses today | Take step-p21's fixes (rank 3) |
| [foxtrot](https://github.com/Formlabs/foxtrot) | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | Quiet since 2022 | A fast STEP parser, NURBS evaluation and triangulation in Rust | Learn from |
| [NIST STEP File Analyzer and Viewer](https://github.com/usnistgov/SFA) | No licence file; NIST software, public domain in the US under the [NIST disclaimer](https://www.nist.gov/copyrights-disclaimers) | Active | Checks exported STEP files against the schemas and the CAx-IF recommended practices | Use as a separate program in the bench |
| [Geometric Tools](https://github.com/davideberly/GeometricTools) | BSL-1.0 (`LICENSE`): permissive, but not on our list | Active | Robust root finding, exact rational arithmetic, intersection queries, polyhedral mass properties | Learn from; adopting needs the owner's OK for BSL-1.0 |
| [inari](https://github.com/unageek/inari) / [robust](https://github.com/georust/robust) | MIT / Apache-2.0 | Quiet / quiet | Interval arithmetic and Shewchuk's predicates; ENGINES.md already lists them | Adopt where needed |
| [tinyspline](https://github.com/msteinbeck/tinyspline), [geomdl](https://github.com/orbingol/NURBS-Python), [tinynurbs](https://github.com/pradeep-pyro/tinynurbs) | MIT (`LICENSE`), MIT (`LICENSE`), BSD-3-Clause | Quiet since 2024 or 2025 | Basic NURBS evaluation and fitting; we already have this | Skip |
| [Open3D](https://github.com/isl-org/Open3D) | MIT (`LICENSE`) | Active | Point clouds and surface reconstruction; only matters for scan-to-CAD | Skip |
| [NASA refine](https://github.com/nasa/refine) | Apache-2.0 | Quiet since 2022 | Mesh adaptation; its geometry comes from EGADS (LGPL) | Skip |
| [openNURBS](https://github.com/mcneel/opennurbs) | **Not verified.** `LICENSE` says "All Rights Reserved" plus a warranty disclaimer and grants nothing; headers point to opennurbs.org for the terms | Active | 3DM file I/O and NURBS evaluation, which we don't need | Skip |
| [enomado/ogeom-rs](https://github.com/enomado/ogeom-rs) | Apache-2.0 snapshot of a project now LGPL-2.1 upstream | Inactive fork | Small fillet and boolean kernel | Skip |

## Rejected on licence

Only the licence file was read for these. None of their source was opened.

| Project | Licence | Note |
|---|---|---|
| OpenCASCADE, FreeCAD | LGPL-2.1 | Already a black-box reference only (RFC 0001) |
| [GoTools](https://github.com/SINTEF-Geometry/GoTools), [SISL](https://github.com/SINTEF-Geometry/SISL) (SINTEF) | AGPL-3.0 (`COPYING`) | The best open SSI and blending code, but excluded. Dokken's subdivision papers behind them are fine to implement from. |
| Gmsh | GPL (per the project; its repository's licence file needs a sign-in) | Excluded |
| Blender BMesh booleans | GPL (`COPYING`) | Excluded. Manifold's README lists Blender among its users. |
| CGAL; libigl `copyleft/` | GPL/LGPL; GPL-3.0 | Excluded |
| BRL-CAD; Netgen; [SolveSpace](https://github.com/solvespace/solvespace) | LGPL-2.1; LGPL-2.1; GPL-3.0 | Excluded |
| [gbs](https://github.com/ssg-aero/gbs) | LGPL-3.0 (`LICENSE.LGPLv3`) | A C++ NURBS library with an OCCT bridge; excluded |
| [andymai/brepkit](https://github.com/andymai/brepkit) v3 and later | AGPL-3.0 | Remus is the Apache-2.0 continuation of its v2 line (see below) |
| [gilbertorconde/ogeom-rs](https://github.com/gilbertorconde/ogeom-rs) | LGPL-2.1 | Excluded |
| [openbimrs/step](https://github.com/openbimrs/step) | AGPL-3.0 | Excluded |
| [Indirect_Predicates](https://github.com/MarcoAttene/Indirect_Predicates) | LGPL-2.1 | The paper (Attene 2020) is fine to implement from |
| IRIT (Elber) | Free for non-commercial use only | Excluded; its papers are fine to implement from |
| [OpenVSP](https://github.com/OpenVSP/OpenVSP) | NASA Open Source Agreement 1.3 (`LICENSE`) | Not on our list; excluded |
| Engineering Sketch Pad / EGADS (NASA) | LGPL-2.1 (per project) | Excluded; also built on OCCT |
| [jsketcher](https://github.com/xibyte/jsketcher), [next.BREP kernel](https://github.com/mmiscool/next.BREP.io_RUST_BREP_KERNEL) | Custom (`LICENSE`): modifications must be assigned to Autodrop3d, or every permission is void | Not open source in our sense; excluded |
| Axom's `quest/io/STEPReader` | BSD, but it compiles only with OpenCASCADE | Don't port; Axom's `primal` has no OCCT dependency |

## Notes on the leading candidates

### 1. Remus: large, close to OCCT's feature set, provenance unclear

- **What it is.** A from-scratch B-rep kernel in Rust that targets WebAssembly, with `unsafe`,
  `unwrap` and `panic` denied by lint. It is the Apache-2.0 continuation of brepkit, which was
  MIT OR Apache-2.0 up to `v2.129.15` (we read both licence files at that tag) and AGPL from v3.
  Its CI enforces that boundary and records it in a provenance ledger. Its `NOTICE` credits the
  brepkit contributors, and truck-geometry pinned at a stated commit.
- **Scale and activity.** About 3,100 commits, pushed daily, roughly 30 MB of Rust under
  `crates/` with about a third of it tests. The crates are `algo` (General Fuse: pave filler,
  face classification, solid assembly), `blend`, `heal`, `offset`, `check` (classification,
  validation, mass properties), `io`, `topology` (coedges, per-use p-curves).
- **Quality evidence.** A written capability matrix, an operation contract, a failure taxonomy,
  and versioned repro bundles that replay identically natively and in wasm. Its stability table
  marks NURBS booleans, shell, offset, healing and STEP as stable, and curved blends and vertex
  blends as experimental. A historical comparison with occt-wasm covers only simple operations.
  It has no external users we could find (0 stars).
- **Provenance risk.** By its own `AI-DISCLOSURE` files, the code was written by AI agents. Its
  module structure and vocabulary follow OCCT's (pave filler, General Fuse, a healing pipeline
  of analysis, fixing and upgrading). OCCT publishes its General Fuse design, so the vocabulary
  alone proves nothing. But neither repository says that OCCT source was kept out, and RFC 0001
  forbids anything derived from OCCT. Gate any port on: (a) asking the maintainers in writing
  whether OCCT source or OCCT-derived code was used; (b) porting only units whose algorithm we
  can trace to a paper or public documentation; (c) PolyCore's own tests as the oracle.
- **How to use it now, at no risk.** Remus reads and writes STEP. So, like OCCT, it can run as a
  separate program in `polycore-bench` and score on our corpus without our reading its code. That
  shows which of its units would actually win before any provenance work is spent.

### 2. Axom `primal`: peer-reviewed geometry with a national lab's tests

- `winding_number.hpp` and `detail/winding_number_{2d,3d}_impl.hpp` compute generalised winding
  numbers for points against 2D NURBS curves, curved polygons and 3D trimmed NURBS patches,
  with caching for batches of queries. This is the containment method of Spainhour & Weiss
  (ACM TOG 2024 for curves; ACM TOG 45(3), 2026 for trimmed NURBS surfaces). It is the
  "winding numbers as fallback" of RFC 0001's Phase 2, and it tolerates the gaps that imported
  models have.
- `evaluate_integral_surface.hpp` implements the trimmed-surface quadrature of Gunderman, Weiss
  and Evans (2021): Bézier extraction with Gauss–Legendre on untrimmed pieces, and a
  reduction to the trimming curves on trimmed ones. With `compute_moments.hpp` this gives
  volume, centroid and inertia straight from the exact B-rep, with the error controlled by
  quadrature order. That is Phase 1's "mass properties from the exact B-rep, with the error bound
  achieved".
- Also there: `NURBSPatch` with trimming curves, `GregoryPatch`, closest point, curvature,
  Bézier curve–curve and ray–patch intersection.
- **Effort.** Header-heavy C++ templates. The port needs NURBS patches and trimming curves, the
  two winding-number paths, the quadrature, and their unit tests. Estimated at 3–6 weeks.

### 3. monstertruck: patches that fit our vendored truck

Our vendored truck crates and monstertruck share their history, so its fixes port with little
translation. In order of value: `step-p21` (two Part 21 syntax fixes that real exports need:
`''` as an escaped apostrophe and `()` as an empty aggregate), `strip_seam_edges` (STEP seam
wires on cylinders and cones), the CDT trim fixes, and STEP assembly output. Its fillet engine
(`monstertruck-fillet`, about 170 KB) and healing crate (about 210 KB) are references for Phase 4
and Phase 6. They inherit truck's numeric design, so read them as references and don't base
PolyCore's own engines on them. Each ported commit carries the upstream SHA, as monstertruck
itself does for truck.

### 4. step-io

It is young (version 0.3.0 on 2026-10-21) and by the author of nacre. Its design suits our
"import breadth" gate: one universal schema read through one pipeline, and a report of what was
healed or dropped and why. Adopt it behind `polycore-io` if it reads our `step-conformance`
and `step-regressions` sets at least as well as `step_in/`. The ISO EXPRESS schemas it derives
from carry ISO's own terms, as its README notes.

### 5. Manifold and manifold-rust

Manifold is used by OpenSCAD, Blender, Godot and BRL-CAD. manifold-rust ports version 3.5.0 to
pure Rust and passes Manifold's full test suite with identical results. It adds an engine on
exact rationals (dashu, MIT OR Apache-2.0) that was run over all of Thingi10K. Its clipper2-rust
dependency is BSL-1.0, so it needs a note in our licence list. Uses for PolyCore: an independent
volume check for boolean results in the bench, booleans on analysis meshes, and repair of
imported STL and 3MF.

## Papers and algorithms per weak spot

These are fair to implement from. Full references are in each paper. Where a permissive
reference implementation exists, it is named.

### Booleans and surface–surface intersection

1. Requicha & Voelcker, "Boolean operations in solid modeling: boundary evaluation and merging
   algorithms", Proc. IEEE 73(1), 1985. The boundary-evaluation framework.
2. Mäntylä, "Boolean operations of 2-manifolds through vertex neighborhood classification",
   ACM TOG 5(1), 1986.
3. Hoffmann, Hopcroft & Karasick, "Robust set operations on polyhedral solids", IEEE CG&A 9(6),
   1989; and Hoffmann, *Geometric and Solid Modeling* (1989, free online).
4. Jackson, "Boundary representation modelling with local tolerances", ACM Solid Modeling 1995.
   Tolerant edges and vertices; already in RFC 0001.
5. OCCT's public *Boolean Operations* specification. General Fuse settles coincidences lowest
   dimension first; RFC 0001 allows reading the documentation.
6. Sederberg & Meyers, "Loop detection in surface patch intersections", CAGD 5(2), 1988; and
   Hohmeyer, "A surface intersection algorithm based on loop detection", 1991. Normal-cone
   (Gauss map) tests so no small loop is missed.
7. Barnhill, Farin, Jordan & Piper, "Surface/surface intersection", CAGD 4, 1987; and Bajaj,
   Hoffmann, Lynch & Hopcroft, "Tracing surface intersections", CAGD 5(4), 1988. Marching.
8. Krishnan & Manocha, "An efficient surface intersection algorithm based on lower-dimensional
   formulation", ACM TOG 16(1), 1997; and Grandine & Klein, "A new approach to the surface
   intersection problem", CAGD 14(2), 1997.
9. Mukundan, Ko, Maekawa, Sakkalis & Patrikalakis, "Tracing surface intersections with
   validated ODE system solver", ACM Solid Modeling 2004 (certified marching); and
   Patrikalakis & Maekawa, *Shape Interrogation for CAD/CAM*, ch. 5 (free online).
10. Yang & Jia, "A Robust and Efficient Intersection Algorithm for NURBS Surfaces: Handling Small
    Loops and Tangent Intersections", ACM TOG 2026 (compared with SISL, OCCT and ACIS); and Li,
    Wu, Luo, Na & Cheng, "A Precision Controlled Surface-Surface Intersection Algorithm for
    NURBS", ACM TOG 45(5), 2026. No code release found for either.
11. Classification and robustness: Shewchuk, "Adaptive precision floating-point arithmetic and
    fast robust geometric predicates", DCG 18, 1997 (the `robust` crate); Jacobson, Kavan &
    Sorkine-Hornung, generalised winding numbers, ACM TOG 32(4), 2013; Spainhour & Weiss, ACM
    TOG 2024 and 2026 (code in Axom); Sederberg et al., "Watertight trimmed NURBS", ACM TOG 27(3),
    2008; Cherchi et al., "Interactive and robust mesh booleans", ACM TOG 41(6), 2022.

### Fillets and blends, including vertex blends

1. Rossignac & Requicha, "Constant-radius blending in solid modelling", Comp. Mech. Eng. 3, 1984.
2. Choi & Ju, "Constant-radius blending in surface modelling", CAD 21(4), 1989. The rolling ball.
3. Barnhill, Farin & Chen, "Constant-radius blending of parametric surfaces", Computing Suppl. 8,
   1993.
4. Vida, Martin & Várady, "A survey of blending methods that use parametric surfaces", CAD 26(5),
   1994.
5. Várady & Rockwood, "Vertex blending based on the setback split", 1989; and Várady & Hoffmann,
   "Vertex blending: problems and solutions", 1997.
6. Braid, "Non-local blending of boundary models", CAD 29(2), 1997. Rollover, cliff edges and
   overflow; already in ENGINES.md.
7. Chiyokura & Kimura, "Design of solids with free-form surfaces", SIGGRAPH 1983. Gregory
   patches for corners; code in Axom.
8. Salvi, Várady & Rockwood, "Ribbon-based transfinite surfaces", CAGD 31(9), 2014; and Várady,
   Salvi & Karikó, "A multi-sided Bézier patch with a simple control structure", CGF 35(2),
   2016. N-sided corner patches; code in transfinite (MIT).
9. Kós, Martin & Várady, "Methods to recover constant radius rolling ball blends in reverse
   engineering", CAGD 17(2), 2000. Spine and contact-curve formulation, also for recognising
   blends on import.

ENGINES.md asks for a freedom-to-operate check on setback blends before Phase 4's advanced
blends. That still stands.

### Offsets and shells of free-form surfaces

1. Rossignac & Requicha, "Offsetting operations in solid modelling", CAGD 3(2), 1986.
2. Farouki, "The approximation of non-degenerate offset surfaces", CAGD 3(1), 1986.
3. Pham, "Offset curves and surfaces: a brief survey", CAD 24(4), 1992; and Maekawa, "An overview
   of offset curves and surfaces", CAD 31(3), 1999.
4. Piegl & Tiller, "Computing offsets of NURBS curves and surfaces", CAD 31(2), 1999.
5. Forsyth, "Shelling and offsetting bodies", ACM Solid Modeling 1995. Shelling a B-rep by
   extending, re-intersecting and re-topologising: Phase 5's engine.
6. Wallner, Sakkalis, Maekawa, Pottmann & Yu, "Self-intersections of offset curves and surfaces",
   IJSM 7(1), 2001.
7. Kumar, Shastry & Prakash, "Computing non-self-intersecting offsets of NURBS surfaces", CAD
   34(3), 2002.
8. Seong, Elber & Kim, "Trimming local and global self-intersections in offset curves/surfaces
   using distance maps", CAD 38(3), 2006.
9. Elber, Lee & Kim, "Comparing offset curve approximation methods", IEEE CG&A 17(3), 1997. For
   the fitting step, see G+Smo's least-squares fitting.

### Healing and repair of imported geometry

1. Bøhn & Wozny, "A topology-based approach for shell closure", 1993.
2. Barequet & Sharir, "Filling gaps in the boundary of a polyhedron", CAGD 12(2), 1995; and
   Barequet & Kumar, "Repairing CAD models", IEEE Visualization 1997.
3. Butlin & Stops, "CAD data repair", 5th International Meshing Roundtable, 1996.
4. Mezentsev & Woehler, "Methods and algorithms of automated CAD repair for incremental surface
   meshing", 8th International Meshing Roundtable, 1999.
5. Sheffer, Bercovier, Blacker & Clements, "Virtual topology operators for meshing", IJCGA 10(3),
   2000.
6. Bischoff & Kobbelt, "Structure preserving CAD model repair", CGF 24(3), 2005.
7. Jackson 1995 (local tolerances) and Sederberg et al. 2008 (watertight trimmed NURBS): tolerant
   edges are the core of healing.
8. Ju, "Fixing geometric errors on polygonal models: a survey", JCST 24(1), 2009. For the
   mesh side (PMP, geogram and Manifold implement parts of it).

### STEP import breadth

1. ISO 10303-21 (clear-text encoding) and ISO 10303-11 (EXPRESS), with the AP242 schemas.
2. CAx-IF *Recommended Practices* (free), especially geometric validation properties, assembly
   structure and PMI. These say what real exporters put in files.
3. Pratt, "Introduction to ISO 10303 — the STEP standard for product data exchange", JCISE 1(1),
   2001.
4. Kemmerer (ed.), *STEP: The Grand Experience*, NIST SP 939, 1999.
5. NIST STEP File Analyzer and Viewer, public domain, as a checker for what we export.
   NIST's test models stay out under RFC 0001 decision 7.

### Exact mass properties

1. Lee & Requicha, "Algorithms for computing the volume and other integral properties of
   solids", CACM 25(9), 1982.
2. Lien & Kajiya, "A symbolic method for calculating the integral properties of arbitrary
   nonconvex polyhedra", IEEE CG&A 4(10), 1984.
3. Mirtich, "Fast and accurate computation of polyhedral mass properties", JGT 1(2), 1996.
4. Gonzalez-Ochoa, McCammon & Peters, "Computing moments of objects enclosed by piecewise
   polynomial surfaces", ACM TOG 17(3), 1998. Divergence theorem on spline faces.
5. Gunderman, Weiss & Evans, "Spectral mesh-free quadrature for planar regions bounded by
   rational parametric curves", CAD 130, 2021.
6. Gunderman, Weiss & Evans, "High-accuracy mesh-free quadrature for trimmed parametric surfaces
   and volumes", CAD 141, 2021. Code in Axom.
7. Laurie, "Calculation of Gauss–Kronrod quadrature rules", Math. Comp. 66, 1997. Nested rules
   give the error estimate that RFC 0001 asks us to report.

## Plan: the top three adoptions

| # | Adoption | Phase | Work | Effort | Expected time saved |
|---|---|---|---|---|---|
| 1 | **Port Axom `primal`'s trimmed-NURBS winding numbers and trimmed-surface quadrature** into `polycore-geom` and `polycore-props`, with their unit tests | 1 (mass properties), 2 (classification fallback) | NURBS patch with trimming curves; GWN 2D and 3D with caching; surface integrals; moments; error estimate from nested rules. Attribution in `NOTICE` (BSD-3-Clause, © LLNS). | 3–6 weeks | **1–2 eng.-months**: two published, tested methods instead of designing our own, and the "error bound achieved" part of Phase 1 |
| 2 | **Cherry-pick monstertruck** into `vendor/polycore-truck-*` | 0–1 | `step-p21` escape fixes, `strip_seam_edges`, CDT trim fixes, STEP assembly output; read the fillet and healing crates as Phase 4 and Phase 6 references | 1–2 weeks | **0.5–1.5 eng.-months**, mostly on the import (19/23) and tessellation (12/14) gates |
| 3 | **Bench Remus, then port what wins** under the same standing practice as solidcore | 0 bench; 2, 5, 6 ports | Add Remus as a column in `polycore-bench` (STEP in, STEP out, a separate program like OCCT); ask its maintainers about OCCT provenance; port winning units in healing, General Fuse coincidence handling and offset, with attribution | 1–2 weeks for the bench; ports as found | **0 if provenance fails; 3–8 eng.-months** if healing, offset and boolean units port |

Two cheap additions for the same period: **step-io** behind `polycore-io` (Low; most of the
import-breadth work for Phase 3's "import ≥ 22/23") and **manifold-rust** in the bench as an
independent volume check (Low).

**Total.** Without Remus, about 1.5–3.5 engineer-months saved for about 2 months of porting.
With Remus ports, 5–11. RFC 0001 budgets 25–50 engineer-months for Phases 0–5, so this is about
5–20 %. These are estimates, not measurements. The bench decides each port, as RFC 0001
requires.

**What does not get faster.** General NURBS SSI (Phase 3) and advanced blends (Phase 4) have no
permissive production code. Phase 3 is the papers above. For Phase 4, Remus's experimental
blends, monstertruck's fillet engine and transfinite's corner patches help, but they don't
replace the work.

## Before adopting anything

- **Third-party notices.** Each adopted crate or ported file gets a `NOTICE` entry, and keeps
  its copyright line where files are ported. MPL-2.0 files stay MPL-2.0 (file-level). BSL-1.0
  (clipper2-rust, Geometric Tools) is permissive but not on the agreed list, so it needs the
  owner's OK.
- **AI-written sources.** Remus, monstertruck, manifold-rust and step-io all say they are
  written partly or wholly with AI agents. Licences still apply, but test before trusting:
  PolyCore's corpus and analytic cases are the oracle, never the source's own claims.
- **Not verified here.** Feature claims are from each project's README and source tree, not
  from running it. Remus's stability labels, monstertruck's fillet quality and step-io's AP
  coverage need the bench before any decision.
