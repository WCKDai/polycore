# RFC 0003: a second consumer (Trigon), sections and drawing projection

- **Status:** Proposed, 2026-11-01. Needs the owner's acceptance; the questions are under
  [Decisions needed](#decisions-needed).
- **Author:** Claude (agent), for the owner
- **Extends:** [RFC 0001](0001-polycore.md) and [RFC 0002](0002-ai-native-engineering-platform.md).
  It replaces only RFC 0001's statement that Polykrom is PolyCore's only consumer.
- **Answers:** [#45](https://github.com/WCKDai/polycore/issues/45) (bounded sections and drawing
  projection with topology provenance), from Trigon's
  [`docs/planning/polycore-requirements.md` §2.2](https://github.com/WCKDai/trigon/blob/main/docs/planning/polycore-requirements.md).

## Summary

On 2026-11-01 the owner said PolyCore "is now also being used or will be used in Trigon", the
open-source building modeller ([WCKDai/trigon](https://github.com/WCKDai/trigon)). PolyCore then
has two consumers: Polykrom (parts and machines) and Trigon (buildings). This RFC says what that
changes for the API and its gates, and proposes the first operations both need and the `Kernel`
trait lacks: a **section** of bodies by a plane and a **drawing projection** (hidden-line
removal), each drawn curve carrying the face or edge it came from.

Trigon has no PolyCore dependency yet (its `Cargo.toml` at `d6e2d9e`). Nothing here forces one.

## A second consumer

What changes:

| Before (RFC 0001) | With two consumers |
|---|---|
| Polykrom is the only consumer; the API can change with it in one step. | A public-API change lands with both consumers green, or behind a deprecation that keeps the old form for one pinned revision. |
| Kernel changes are gated on Polykrom's goldens. | Also on Trigon's fixtures, once Trigon depends on PolyCore (built and tested at the candidate revision, as Polykrom's goldens are). |
| Building scale is out of scope. | Bodies are modelled in a **local frame**; map coordinates never reach the kernel (Trigon §2.1). The kernel states the size box its model resolution holds over and refuses input outside it (`RefusalKind::InvalidInput`). |

What doesn't: one kernel, a Rust API, no C ABI, Apache-2.0, the evidence and determinism rules of
RFC 0002, and consumers pinning PolyCore by git revision.

## Sections

```rust
fn section(&self, cx: &Cx, bodies: &[Body], spec: &SectionSpec) -> Outcome<Section>;

pub struct SectionSpec {
    /// The cutting plane, in the bodies' frame (mm).
    pub plane: Plane,
    /// Where drawing coordinates live: an origin on the plane and its u, v axes. A drawing frame
    /// is separate from any model transform.
    pub frame: DrawingFrame,
    /// Keep only what lies inside this rectangle of the drawing frame (a bounded section, e.g. one
    /// storey or one bay). `None`: everything the plane cuts.
    pub bound: Option<Rect2>,
}

pub struct Section {
    /// Cut curves, in drawing coordinates, each with where it came from.
    pub curves: Vec<DrawnCurve>,
    /// Closed regions of the cut (solid material), as loops of `curves` indices: one outer loop
    /// and its holes, per body.
    pub regions: Vec<Region>,
}
```

A section curve's source is the **face** the plane cut; a curve that lies along an edge in the
plane is owned by that **edge**.

## Drawing projection

```rust
fn project(&self, cx: &Cx, bodies: &[Body], view: &ViewSpec) -> Outcome<Projection>;

pub struct ViewSpec {
    /// Parallel (orthographic) view: the direction looked along and the drawing frame.
    pub direction: Vec3,
    pub frame: DrawingFrame,
    pub bound: Option<Rect2>,
    /// Also return hidden curves (dashed in a drawing).
    pub hidden: bool,
    /// Cut first at this plane and draw only what lies behind it (a section view).
    pub cut: Option<Plane>,
}

pub struct Projection {
    pub curves: Vec<DrawnCurve>,
}
```

Each curve is `Visible` or `Hidden`, and an `Edge`, a `Silhouette` (the outline of a curved face
seen edge-on) or a `Cut` (from `cut`).

## Provenance (both operations)

```rust
pub struct DrawnCurve {
    pub geometry: Curve2,          // line, arc, circle, ellipse or B-spline, drawing coordinates
    pub kind: CurveKind,           // Edge | Silhouette | Cut
    pub visibility: Visibility,    // Visible | Hidden (sections: always Visible)
    pub source: Source,
}

pub struct Source {
    pub body: usize,               // index into the `bodies` argument
    pub entity: Entity,            // Face(usize) | Edge(usize), the body's own indices
    pub span: Option<(f64, f64)>,  // the part of a source edge this curve is, as edge parameters
    pub also: Vec<(usize, Entity)>,// coincident sources this curve stands for (see ties)
}
```

- **The consumer's identity, not the kernel's.** `body` and the face/edge indices are the ones the
  consumer already maps through each body's `History` (RFC 0002 entity references), so a picked
  segment resolves to the same wall, window or part in 2D and 3D. When an edit changes a body, its
  new `History` says whether that face or edge was kept, changed or is gone, so a dimension on it
  survives or is invalidated explicitly, never silently re-attached.
- **Splitting keeps the source.** An edge split by occlusion or by `bound` yields several curves
  with the same `source` and adjacent `span`s.
- **Ties are deterministic.** Coincident curves from several sources (a wall face flush with a slab
  edge) are emitted once. The lowest `(body, entity)` owns the curve and the rest are listed in
  `also`, so a consumer can pick either element.
- **Order is deterministic:** by `(body, entity, span start)`, the same for every thread count.

## Tolerance, scale and refusals

- Results are within the model tolerance in `Cx`. `Evidence` reports the achieved tolerance and
  how many curves are exact (analytic) or fitted.
- Input outside the size box: `InvalidInput`, naming the body and its extent.
- A face type the backend can't section or project exactly: `Unsupported`, naming the faces
  (`EntityRef`). The whole call fails rather than dropping faces. Approximate curves from
  tessellation are a [decision](#decisions-needed), never a silent fallback.
- Work budget and cancellation through `Cx`: `OutOfBudget` / `Cancelled`, as every operation.

## Scope and order

1. **Planar bodies, exactly.** Sections are segments and projection is exact polygon HLR. This is
   most of architecture (walls, slabs, openings, roofs) and most prismatic parts. It doesn't
   wait for Phase 2's general booleans and can use exact predicates (Trigon §2.3).
2. **Analytic faces**: cylinders, cones and spheres, with ellipse/circle cut curves and silhouettes.
3. **Free-form**: `Unsupported` until the general intersection work (Phase 2, #39) lands.

## Fixtures (corpus cases, with correctness and bounded-work evidence)

From #45: a wall with windows and doors; an opening crossing storeys; slab voids; an oblique
section; coincident and tangent cuts; building-scale geometry with small details (a 200 m
building with a 5 mm gasket). From Polykrom: a part drawing with holes and fillets (step 2).
Graded answers are analytic or hand-calculated, as RFC 0001 requires. Trigon can supply its
current drawing outputs (`crates/engine/src/sheet/cut.rs`, `project.rs`) as consumer evidence.

Consumer tests: a picked 2D segment resolves to the same element in 3D, and after an edit its
reference is kept or explicitly invalidated.

## Decisions needed

1. **Accept Trigon as a second consumer**, with the gates above (both consumers green for an API
   change; Trigon's fixtures in the kernel merge gate once it depends on PolyCore).
2. **Approximate curves.** May a caller opt in to tessellation-based section/projection curves for
   faces the exact path can't do yet, flagged per curve and in `Evidence`? Proposed: yes, opt-in
   only, never the default.
3. **Where the planar path lives**: a polyhedral backend in `CompositeKernel` (Trigon §2.3), or
   inside the current backend. Proposed: a backend, so it's measured on its own like any other.
