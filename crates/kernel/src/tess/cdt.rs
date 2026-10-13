//! Faces the structured grid cannot take (trimmed faces, holes): a constrained Delaunay
//! triangulation of the boundary loops in (u, v) scaled by the surface's mean metric, with
//! interior seed points, refinement wherever a triangle sags more than the tolerance from the
//! surface, then Ruppert refinement for shape. Constraint edges are never split, so the
//! boundary stays exactly the shared edge nodes.

use std::collections::HashMap;

use solidcraft_geom::Vec3;

use spade::handles::FixedVertexHandle;
use spade::{AngleLimit, ConstrainedDelaunayTriangulation, Point2, RefinementParameters, Triangulation};

type Cdt = ConstrainedDelaunayTriangulation<Point2<f64>>;

/// Smallest angle Ruppert refinement aims for.
const MIN_ANGLE_DEG: f64 = 20.0;

/// Rounds of sag refinement.
const SAG_ROUNDS: usize = 16;

/// The surface, for refining where the triangles leave it.
pub(crate) struct Sag<'a> {
    pub eval: &'a dyn Fn(f64, f64) -> Option<Vec3>,
    pub tol: f64,
    /// Scale of u and v in the triangulation (the surface's mean |S_u|, |S_v|).
    pub scale: [f64; 2],
}

/// How to fill the inside of the loops.
pub(crate) struct Fill<'a> {
    /// Interior points (already inside the loops).
    pub seeds: &'a [[f64; 2]],
    /// Refinement, adding at most this many points in all.
    pub refine: Option<(Sag<'a>, usize)>,
}

/// Triangulate the region bounded by `loops` (ranges of `boundary`, each closed). Boundary
/// vertices keep their indices; added points follow. Triangles are counter-clockwise in
/// (u, v). `None` when a loop cannot be kept as constraints (it crosses itself or another).
pub(crate) fn triangulate(boundary: &[[f64; 2]], loops: &[std::ops::Range<usize>], fill: Fill) -> Option<(Vec<[f64; 2]>, Vec<[u32; 3]>)> {
    if boundary.iter().flatten().any(|x| !x.is_finite()) {
        return None;
    }
    let scale = match &fill.refine {
        Some((sag, _)) if sag.scale.iter().all(|k| k.is_finite() && *k > 0.0) => sag.scale,
        _ => [1.0, 1.0],
    };
    let to = |p: &[f64; 2]| Point2::new(p[0] * scale[0], p[1] * scale[1]);
    let from = |p: Point2<f64>| [p.x / scale[0], p.y / scale[1]];
    let mut cdt = Cdt::new();
    let mut index: HashMap<FixedVertexHandle, u32> = HashMap::new();
    let mut handles: Vec<FixedVertexHandle> = Vec::with_capacity(boundary.len());
    for (i, p) in boundary.iter().enumerate() {
        let h = cdt.insert(to(p)).ok()?;
        index.entry(h).or_insert(u32::try_from(i).ok()?);
        handles.push(h);
    }
    for r in loops {
        let n = r.len();
        for k in 0..n {
            let (a, b) = (*handles.get(r.start + k)?, *handles.get(r.start + (k + 1) % n)?);
            if a == b || cdt.exists_constraint(a, b) {
                continue;
            }
            if !cdt.can_add_constraint(a, b) {
                return None;
            }
            cdt.add_constraint(a, b);
        }
    }
    for s in fill.seeds {
        if s.iter().all(|x| x.is_finite()) {
            let _ = cdt.insert(to(s));
        }
    }
    if let Some((sag, budget)) = &fill.refine {
        let constraints = cdt.num_constraints();
        let added = refine_sag(&mut cdt, sag, *budget, &from);
        let params = RefinementParameters::<f64>::new()
            .keep_constraint_edges()
            .exclude_outer_faces(true)
            .with_angle_limit(AngleLimit::from_deg(MIN_ANGLE_DEG))
            .with_max_additional_vertices(budget.saturating_sub(added));
        cdt.refine(params);
        // A split constraint would put a point on the boundary the neighbour does not have.
        if cdt.num_constraints() != constraints {
            return None;
        }
    }
    let mut uv: Vec<[f64; 2]> = boundary.to_vec();
    let mut tris = Vec::new();
    let region = region(&cdt);
    for face in cdt.inner_faces() {
        if !region.get(face.index()).copied().unwrap_or(false) {
            continue;
        }
        let vs = face.vertices();
        let mut t = [0u32; 3];
        for (slot, v) in t.iter_mut().zip(vs) {
            *slot = match index.get(&v.fix()) {
                Some(&i) => i,
                None => {
                    let i = u32::try_from(uv.len()).ok()?;
                    uv.push(from(v.position()));
                    index.insert(v.fix(), i);
                    i
                }
            };
        }
        tris.push(t);
    }
    let extra = uv.split_off(boundary.len());
    Some((extra, tris))
}

/// Insert the parameters of the surface point under every triangle centre and every
/// non-constraint edge middle that lies more than the tolerance from the triangle, until none
/// does or `budget` points were added. Returns the points added.
fn refine_sag(cdt: &mut Cdt, sag: &Sag, budget: usize, from: &dyn Fn(Point2<f64>) -> [f64; 2]) -> usize {
    let mut added = 0;
    let mut cache: HashMap<FixedVertexHandle, Option<Vec3>> = HashMap::new();
    for _ in 0..SAG_ROUNDS {
        let mut todo: Vec<Point2<f64>> = Vec::new();
        let mut at = |h: FixedVertexHandle, p: Point2<f64>| {
            *cache.entry(h).or_insert_with(|| {
                let q = from(p);
                (sag.eval)(q[0], q[1])
            })
        };
        let region = region(cdt);
        for face in cdt.inner_faces() {
            if !region.get(face.index()).copied().unwrap_or(false) {
                continue;
            }
            let vs = face.vertices();
            let ps = vs.map(|v| v.position());
            let c = Point2::new((ps[0].x + ps[1].x + ps[2].x) / 3.0, (ps[0].y + ps[1].y + ps[2].y) / 3.0);
            let (Some(a), Some(b), Some(d)) = (at(vs[0].fix(), ps[0]), at(vs[1].fix(), ps[1]), at(vs[2].fix(), ps[2])) else { continue };
            let off = |q: Point2<f64>, chord: Vec3| {
                let uv = from(q);
                (sag.eval)(uv[0], uv[1]).is_some_and(|s| s.dist(chord) > sag.tol)
            };
            if off(c, (a + b + d) * (1.0 / 3.0)) {
                todo.push(c);
                continue;
            }
            for (e, (pa, pb)) in face.adjacent_edges().into_iter().zip([(a, b), (b, d), (d, a)]) {
                if e.is_constraint_edge() {
                    continue;
                }
                let [p, q] = e.positions();
                let m = Point2::new(0.5 * (p.x + q.x), 0.5 * (p.y + q.y));
                if off(m, (pa + pb) * 0.5) {
                    todo.push(m);
                }
            }
        }
        if todo.is_empty() {
            break;
        }
        todo.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        todo.dedup();
        for p in todo {
            if added >= budget {
                return added;
            }
            if cdt.insert(p).is_ok() {
                added += 1;
            }
        }
    }
    added
}

/// Per face (by index): inside the region the constraint loops bound. A flood fill from the
/// outer face that flips parity each time it crosses a constraint edge (holes are out), linear
/// in the number of triangles.
fn region(cdt: &Cdt) -> Vec<bool> {
    use spade::handles::{FixedFaceHandle, InnerTag};
    let mut parity: Vec<Option<bool>> = vec![None; cdt.num_all_faces()];
    let mut todo: Vec<(FixedFaceHandle<InnerTag>, bool)> = Vec::new();
    for e in cdt.convex_hull() {
        // Hull edges have the outer face on one side; the other side is an inner face.
        for side in [e, e.rev()] {
            if let Some(inner) = side.face().as_inner() {
                todo.push((inner.fix(), e.is_constraint_edge()));
            }
        }
    }
    while let Some((h, odd)) = todo.pop() {
        match parity.get_mut(h.index()) {
            Some(slot @ None) => *slot = Some(odd),
            _ => continue,
        }
        for e in cdt.face(h).adjacent_edges() {
            if let Some(next) = e.rev().face().as_inner() {
                todo.push((next.fix(), odd ^ e.is_constraint_edge()));
            }
        }
    }
    parity.into_iter().map(|p| p.unwrap_or(false)).collect()
}

/// The grid points `us` × `vs` inside the region bounded by the loops (even–odd, so holes are
/// out): per grid line, the loops' crossings sorted once, then each point by binary search.
pub(crate) fn inside_grid(us: &[f64], vs: &[f64], polys: &[&[[f64; 2]]]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let mut xs: Vec<f64> = Vec::new();
    for &v in vs {
        xs.clear();
        for poly in polys {
            let n = poly.len();
            for k in 0..n {
                let (Some(a), Some(b)) = (poly.get(k), poly.get((k + 1) % n)) else { continue };
                if (a[1] > v) != (b[1] > v) {
                    xs.push(a[0] + (v - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for &u in us {
            if xs.partition_point(|x| *x < u) % 2 == 1 {
                out.push([u, v]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(uv: &[[f64; 2]], tris: &[[u32; 3]]) -> f64 {
        tris.iter()
            .map(|t| {
                let (a, b, c) = (uv[t[0] as usize], uv[t[1] as usize], uv[t[2] as usize]);
                0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]))
            })
            .sum()
    }

    #[test]
    fn square_with_a_hole_keeps_its_boundary() {
        let mut b = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        b.extend([[0.4, 0.4], [0.4, 0.6], [0.6, 0.6], [0.6, 0.4]]);
        let loops = [0..4, 4..8];
        let flat = |u: f64, v: f64| Some(Vec3::new(u, v, 0.0));
        for refine in [false, true] {
            let refine = refine.then_some((Sag { eval: &flat, tol: 1e-3, scale: [2.0, 1.0] }, 200));
            let label = refine.is_some();
            let Some((extra, tris)) = triangulate(&b, &loops, Fill { seeds: &[[0.2, 0.2], [0.5, 0.5]], refine }) else { panic!("meshes") };
            let mut uv = b.clone();
            uv.extend(extra);
            assert!((area(&uv, &tris) - 0.96).abs() < 1e-12, "refined: {label}");
            // No added point on the boundary: boundary segments stay whole.
            for r in &loops {
                for k in r.clone() {
                    let next = if k + 1 == r.end { r.start } else { k + 1 };
                    let (p, q) = (k as u32, next as u32);
                    let uses =
                        tris.iter().filter(|t| (0..3).any(|e| (t[e] == p && t[(e + 1) % 3] == q) || (t[e] == q && t[(e + 1) % 3] == p))).count();
                    assert_eq!(uses, 1, "segment {p}-{q}");
                }
            }
        }
    }

    #[test]
    fn crossing_loops_are_refused() {
        let b = [[0.0, 0.0], [1.0, 1.0], [1.0, 0.0], [0.0, 1.0]];
        let whole = std::slice::from_ref(&(0..4));
        assert!(triangulate(&b, whole, Fill { seeds: &[], refine: None }).is_none());
    }
}
