//! Shells of bodies whose holes come closer to another wall than the shell is thick (a hole
//! drilled next to a chamfer): moving every face in by the wall keeps the topology only where
//! each face keeps some of its area, and there the ring of a face round the hole vanishes.
//!
//! A body with holes is the body with the holes filled less the holes, and its cavity is the
//! filled body's cavity less the holes grown by the wall (a point is a wall away from the body
//! exactly when it is that far inside the filled body and that far from every hole). So the
//! holes are filled (Delete Face heals them), the filled body's cavity is made by the
//! topology-preserving offset (`offset_free`), each hole grown by the wall is cut from it, and
//! the shell is the body less that cavity. A hole is grown with sharp edges, so the wall round
//! the edge where a hole meets its face is a little thicker than asked, never thinner.

use std::collections::{HashMap, HashSet};

use mt::{BoundedCurve, ParametricCurve};
use solidcraft_geom::{Aabb3, Vec3};
use truck_modeling as mt;

use crate::body::{Body, Solid, from_p3};
use crate::history::{FaceOrigin, History};
use crate::offset::{Surf, surf_of};
use crate::{KernelError, Result, guard};

/// Most holes a shell fills and grows.
const MAX_HOLES: usize = 64;
/// Most faces a body may have for its holes to be found (as the free-form offset takes).
const MAX_FACES: usize = 512;

/// A hole: the faces that line it (indices in the body's face order) and the inner loops of
/// planar faces where it opens (each with the plane's face index).
struct Hole {
    faces: Vec<usize>,
    rims: Vec<(usize, mt::Wire)>,
}

/// Points sampled along a wire, as it runs.
fn wire_points(w: &mt::Wire) -> Vec<Vec3> {
    let mut pts = Vec::new();
    for e in w.edge_iter() {
        let c = e.oriented_curve();
        let (t0, t1) = c.range_tuple();
        pts.extend((0..8).map(|k| from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 8.0))));
    }
    pts
}

/// The signed area a wire encloses, seen along `n`.
fn wire_area(w: &mt::Wire, n: Vec3) -> f64 {
    let pts = wire_points(w);
    let Some(&first) = pts.first() else { return 0.0 };
    let mut sum = Vec3::ZERO;
    for (a, b) in pts.iter().zip(pts.iter().skip(1).chain(std::iter::once(&first))) {
        sum += (*a - first).cross(*b - first);
    }
    sum.dot(n) * 0.5
}

/// The holes of `b`: from each inner loop of a planar face, the faces reached across it without
/// crossing another inner loop of a planar face, where they lie behind that face (inside the
/// body: a hole, not a boss standing on the face).
fn holes(b: &Body) -> Vec<Hole> {
    let faces: Vec<mt::Face> = b.solid.face_iter().cloned().collect();
    if faces.len() > MAX_FACES {
        return Vec::new();
    }
    let tol = (b.size() * 1e-7).max(1e-9) * 100.0;
    let planes: Vec<Option<(Vec3, f64)>> =
        faces.iter().map(|f| if let Some(Surf::Plane { n, d }) = surf_of(f, tol) { Some((n, d)) } else { None }).collect();
    let mut efaces: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        for e in f.edge_iter() {
            let list = efaces.entry(e.id()).or_default();
            if !list.contains(&i) {
                list.push(i);
            }
        }
    }
    // Every inner loop of a planar face (all but the loop enclosing the most area).
    let mut inner: Vec<(usize, mt::Wire)> = Vec::new();
    for (i, (f, plane)) in faces.iter().zip(&planes).enumerate() {
        let Some((n, _)) = *plane else { continue };
        let wires = f.absolute_boundaries().clone();
        let outer = wires.iter().enumerate().max_by(|a, c| wire_area(a.1, n).abs().total_cmp(&wire_area(c.1, n).abs())).map(|(k, _)| k);
        inner.extend(wires.into_iter().enumerate().filter(|(k, _)| Some(*k) != outer).map(|(_, w)| (i, w)));
    }
    let stops: HashSet<mt::EdgeID> = inner.iter().flat_map(|(_, w)| w.edge_iter().map(|e| e.id())).collect();
    let mut found: Vec<Hole> = Vec::new();
    for (p, w) in &inner {
        let Some((n, d)) = planes.get(*p).copied().flatten() else { continue };
        if found.iter().any(|h| h.rims.iter().any(|(_, r)| r.edge_iter().zip(w.edge_iter()).all(|(x, y)| x.id() == y.id()))) {
            continue;
        }
        // Flood from the faces across the loop.
        let mut part: Vec<usize> = Vec::new();
        let mut todo: Vec<usize> = w.edge_iter().flat_map(|e| efaces.get(&e.id()).into_iter().flatten().copied()).filter(|f| f != p).collect();
        while let Some(f) = todo.pop() {
            if part.contains(&f) || f == *p {
                continue;
            }
            part.push(f);
            let Some(face) = faces.get(f) else { continue };
            for e in face.edge_iter() {
                if stops.contains(&e.id()) {
                    continue;
                }
                todo.extend(efaces.get(&e.id()).into_iter().flatten().copied());
            }
        }
        // Not a hole where the faces across the loop reach every other face (no inner loop
        // bounds them); the side test below tells a hole from a boss.
        if part.is_empty() || part.len() + 1 >= faces.len() {
            continue;
        }
        // Behind the face: every vertex of the hole not on the face's plane lies inside it.
        let size = b.size();
        let behind =
            part.iter().filter_map(|f| faces.get(*f)).flat_map(|f| f.vertex_iter().map(|v| n.dot(from_p3(v.point())) - d)).all(|h| h < size * 1e-6);
        let deep =
            part.iter().filter_map(|f| faces.get(*f)).flat_map(|f| f.vertex_iter().map(|v| n.dot(from_p3(v.point())) - d)).any(|h| h < -size * 1e-6);
        if !(behind && deep) {
            continue;
        }
        // Every loop where the hole opens: the inner loops whose edges border it.
        let rims: Vec<(usize, mt::Wire)> = inner
            .iter()
            .filter(|(_, r)| r.edge_iter().all(|e| efaces.get(&e.id()).is_some_and(|fs| fs.iter().any(|f| part.contains(f)))))
            .cloned()
            .collect();
        found.push(Hole { faces: part, rims });
        if found.len() > MAX_HOLES {
            break;
        }
    }
    found
}

/// Whether a hole of `b` opens within `walls` walls of another loop of the face it opens in,
/// where the ring of face between them may vanish when the shell moves both by the wall (within
/// two, it does wherever the neighbour across that loop stands square to the face).
pub(crate) fn hole_near_rim(b: &Body, thickness: f64, walls: f64) -> bool {
    let faces: Vec<mt::Face> = b.solid.face_iter().cloned().collect();
    holes(b).iter().flat_map(|h| h.rims.iter()).any(|(p, rim)| {
        let Some(face) = faces.get(*p) else { return false };
        let ids: HashSet<mt::EdgeID> = rim.edge_iter().map(|e| e.id()).collect();
        let mine = wire_points(rim);
        // The other loops as polylines (their points closed round), each rim point measured to
        // their segments.
        face.absolute_boundaries().iter().filter(|w| !w.edge_iter().any(|e| ids.contains(&e.id()))).any(|w| {
            let pts = wire_points(w);
            let segs: Vec<(Vec3, Vec3)> = pts.iter().zip(pts.iter().skip(1).chain(pts.first())).map(|(a, c)| (*a, *c)).collect();
            mine.iter().any(|m| segs.iter().any(|(a, c)| segment_dist(*m, *a, *c) < thickness * walls))
        })
    })
}

/// The distance from `p` to the segment from `a` to `c`.
fn segment_dist(p: Vec3, a: Vec3, c: Vec3) -> f64 {
    let d = c - a;
    let t = if d.len2() > 0.0 { ((p - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + d * t)
}

/// The solid a hole removed: its faces turned inside out, capped by its rims on their planes.
fn hole_solid(b: &Body, hole: &Hole) -> Result<Body> {
    let faces: Vec<mt::Face> = b.solid.face_iter().cloned().collect();
    guard("shell", || {
        let mut out: Vec<mt::Face> = hole.faces.iter().filter_map(|i| faces.get(*i)).map(|f| f.inverse()).collect();
        for (p, w) in &hole.rims {
            let plane = faces.get(*p).ok_or_else(|| KernelError::Failed("shell: a hole's face".into()))?;
            let mut cap =
                mt::Face::try_new(vec![w.inverse()], plane.surface()).map_err(|e| KernelError::Failed(format!("shell: a hole's cap: {e}")))?;
            if !plane.orientation() {
                cap.invert();
            }
            out.push(cap);
        }
        let solid = Solid::try_new(vec![out.into()]).map_err(|e| KernelError::Failed(format!("shell: a hole's solid: {e}")))?;
        Body::new(solid)
    })
}

/// Where a refusal says it happened: the last `[x, y, z]` in its message.
pub(crate) fn refused_at(e: &KernelError) -> Option<Vec3> {
    let m = e.to_string();
    let open = m.rfind('[')?;
    let close = open + m.get(open..)?.find(']')?;
    let xs: Vec<f64> = m.get(open + 1..close)?.split(',').map(|x| x.trim().parse::<f64>()).collect::<std::result::Result<_, _>>().ok()?;
    match xs.as_slice() {
        [x, y, z] if x.is_finite() && y.is_finite() && z.is_finite() => Some(Vec3::new(*x, *y, *z)),
        _ => None,
    }
}

/// Shell of `b` (`opened`: its open faces) through its holes filled (see the module comment);
/// `None` where it has none. Open faces move out past the body (by a margin), so the cavity
/// passes through them and the body less the cavity is open there.
pub(crate) fn shell_round_holes(b: &Body, opened: &[usize], thickness: f64, tolerance: f64) -> Result<Option<Body>> {
    let found = holes(b);
    if found.is_empty() || found.len() > MAX_HOLES {
        return Ok(None);
    }
    let lining: Vec<usize> = found.iter().flat_map(|h| h.faces.iter().copied()).collect();
    if opened.iter().any(|i| lining.contains(i)) {
        return Ok(None);
    }
    // A point inside each open face, found again on the filled body.
    let mesh = b.tessellate((b.size() * 2e-3).max(1e-3))?;
    let mut inside: Vec<Vec3> = Vec::new();
    for o in opened {
        let best = mesh
            .triangles
            .iter()
            .zip(&mesh.tri_face)
            .filter(|(_, f)| **f as usize == *o)
            .filter_map(|(t, _)| mesh.tri(t))
            .max_by(|x, y| tri_area(x).total_cmp(&tri_area(y)));
        let [x, y, z] = best.ok_or_else(|| KernelError::Failed("shell: an open face with no mesh".into()))?;
        inside.push((x + y + z) / 3.0);
    }
    let filled = crate::delete_face::delete_faces(b, &lining)?;
    let mut open_filled: Vec<usize> = Vec::new();
    for p in &inside {
        let f = crate::surfaces::nearest_face(&filled, *p)?;
        if !open_filled.contains(&f) {
            open_filled.push(f);
        }
    }
    let margin = thickness + b.size() * 0.05;
    let cavity = crate::offset_free::offset_free(&filled, |i| if open_filled.contains(&i) { margin } else { -thickness }, tolerance)?;
    if let Some(p) = crate::offset_free::too_thin(&filled, &cavity, &open_filled, thickness, tolerance)? {
        return Err(KernelError::Failed(format!(
            "not supported yet: the shell's walls would meet near [{:.2}, {:.2}, {:.2}] (the body is thinner there than twice the wall)",
            p.x, p.y, p.z
        )));
    }
    // The holes grown by the wall, cut from the cavity at once where they lie apart (one
    // Boolean, not one per hole), else one by one.
    let mut grown: Vec<(Body, Aabb3)> = Vec::new();
    for hole in &found {
        let g = crate::offset_free::offset_free(&hole_solid(b, hole)?, |_| thickness, tolerance)?;
        let mut bb = Aabb3::EMPTY;
        for p in g.tessellate((g.size() * 1e-2).max(1e-3))?.positions {
            bb.add(p);
        }
        grown.push((g, bb.inflated(thickness)));
    }
    let overlap = |a: &Aabb3, c: &Aabb3| {
        a.min.x <= c.max.x && c.min.x <= a.max.x && a.min.y <= c.max.y && c.min.y <= a.max.y && a.min.z <= c.max.z && c.min.z <= a.max.z
    };
    let apart = grown.iter().enumerate().all(|(i, (_, a))| grown.iter().skip(i + 1).all(|(_, c)| !overlap(a, c)));
    let mut cavity = cavity;
    if apart && grown.len() > 1 {
        let shells: Vec<mt::Shell> = grown.iter().flat_map(|(g, _)| g.solid.boundaries().iter().cloned()).collect();
        let tool = guard("shell", || Body::new(Solid::new_unchecked(shells)))?;
        cavity =
            crate::ops::boolean(&cavity, &tool, crate::BoolOp::Cut)?.ok_or_else(|| KernelError::Failed("shell: the holes fill the cavity".into()))?;
    } else {
        for (g, _) in &grown {
            cavity =
                crate::ops::boolean(&cavity, g, crate::BoolOp::Cut)?.ok_or_else(|| KernelError::Failed("shell: the holes fill the cavity".into()))?;
        }
    }
    let walls = cavity_history(b, &cavity, thickness)?;
    let r = crate::ops::boolean(b, &cavity, crate::BoolOp::Cut)?.ok_or_else(|| KernelError::Failed("the shell removed everything".into()))?;
    // Over `[b]`: its own faces kept, each wall of the cavity an offset of the face it lines.
    let h = crate::boolean_record::of_pair(&r, &crate::boolean_record::kept(b, 0, 1), &walls);
    Ok(Some(r.with_checked_history(h, &crate::record::counts(&[b]))))
}

/// The cavity's history over `[b]`: each of its faces an offset of the face of `b` it lines, the
/// face a wall out from it (from the middle of its largest triangle, along the triangle's
/// outward normal).
fn cavity_history(b: &Body, cavity: &Body, thickness: f64) -> Result<Option<History>> {
    let mesh = cavity.tessellate((cavity.size() * 2e-3).max(1e-3))?;
    let outer = b.tessellate((b.size() * 2e-3).max(1e-3))?;
    let mut best: HashMap<u32, (f64, Vec3)> = HashMap::new();
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let Some(tri) = mesh.tri(t) else { continue };
        let [x, y, z] = tri;
        let n = (y - x).cross(z - x);
        let area = n.len();
        let Some(n) = n.normalized() else { continue };
        if best.get(f).is_none_or(|(a, _)| area > *a) {
            best.insert(*f, (area, (x + y + z) / 3.0 + n * thickness));
        }
    }
    let mut faces = Vec::with_capacity(cavity.face_count());
    for i in 0..cavity.face_count() {
        let Some((_, p)) = u32::try_from(i).ok().and_then(|k| best.get(&k)) else { return Ok(None) };
        let near = outer
            .triangles
            .iter()
            .zip(&outer.tri_face)
            .filter_map(|(t, f)| outer.tri(t).map(|[x, y, z]| (crate::polyhedron::point_tri(*p, x, y, z), *f)))
            .min_by(|a, c| a.0.total_cmp(&c.0));
        let Some((_, f)) = near else { return Ok(None) };
        faces.push(crate::record::generated(vec![FaceOrigin::Offset { operand: 0, face: f as usize }]));
    }
    Ok(Some(crate::record::from_faces(&cavity.solid, 1, faces)))
}

fn tri_area(t: &[Vec3; 3]) -> f64 {
    let [a, b, c] = *t;
    (b - a).cross(c - a).len()
}
