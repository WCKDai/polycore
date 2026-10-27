//! SolidCraft: face pairs whose intersection has a closed form (#125), intersected exactly rather
//! than by colliding their meshes and projecting the collision's points back onto both surfaces.
//!
//! So far: two planar faces bounded by straight edges. Their intersection is the planes' line,
//! inside both faces: each face's boundary crosses the other plane at points along the line, and
//! between alternate crossings the line runs inside the face (a vertex on the plane decides its
//! side once, so a corner the plane only touches is crossed twice at one point or not at all).
//! The pieces inside both faces are the segments the mesh collision finds (a planar face's mesh
//! is the face itself), exactly. A pair this cannot decide (a face of another surface or with
//! another edge, parallel planes that coincide, an edge lying in the other plane) is `None`, and
//! the mesh collision takes it as before.

use std::any::Any;
use truck_base::{cgmath64::*, tolerance::*};
use truck_meshalgo::prelude::PolylineCurve;
use truck_topology::Face;

#[cfg(any(test, feature = "test-hooks"))]
thread_local! {
    /// Set by the test hooks: every pair goes to the mesh collision, as before.
    pub(super) static MESH_ONLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// How many pairs were intersected exactly (for the test hooks to see that they were).
    pub(super) static EXACT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Whether exact intersections are taken (always, but in the test hooks' comparisons).
fn enabled() -> bool {
    #[cfg(any(test, feature = "test-hooks"))]
    {
        !MESH_ONLY.with(|c| c.get())
    }
    #[cfg(not(any(test, feature = "test-hooks")))]
    {
        true
    }
}

/// The plane of a planar surface: a point on it and its unit normal.
fn plane_of<S: 'static>(s: &S) -> Option<(Point3, Vector3)> {
    let any: &dyn Any = s;
    match any.downcast_ref::<truck_modeling::Surface>()? {
        truck_modeling::Surface::Plane(p) => {
            let n = p.normal();
            (n.magnitude2().is_finite() && n.magnitude2() > 0.5).then(|| (p.origin(), n.normalize()))
        }
        _ => None,
    }
}

/// Whether an edge curve is a straight segment between its ends: a line, or a B-spline whose
/// control points all lie on one line (its points lie in their hull).
fn straight<C: 'static>(c: &C) -> bool {
    let any: &dyn Any = c;
    match any.downcast_ref::<truck_modeling::Curve>() {
        Some(truck_modeling::Curve::Line(_)) => true,
        Some(truck_modeling::Curve::BSplineCurve(b)) => {
            let pts = b.control_points();
            match (pts.first(), pts.last()) {
                (Some(a), Some(z)) if !a.near(z) => {
                    let d = (z - a).normalize();
                    pts.iter().all(|p| (p - a).cross(d).magnitude() <= TOLERANCE)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// Where the line `o + t d` runs inside `face` (on a plane through the line), as sorted parameter
/// intervals: from the face's straight edges' crossings of the plane `(p, n)` the face's own
/// plane meets it in. `None` when an edge is not straight, lies in that plane, or the crossings
/// don't pair up.
fn inside_intervals<C: Clone + 'static, S>(face: &Face<Point3, C, S>, (p, n): (Point3, Vector3), o: Point3, d: Vector3) -> Option<Vec<(f64, f64)>> {
    let mut ts = Vec::new();
    for wire in face.boundaries() {
        for e in wire.edge_iter() {
            if !straight(&e.curve()) {
                return None;
            }
            let (a, b) = (e.front().point(), e.back().point());
            let (ga, gb) = ((a - p).dot(n), (b - p).dot(n));
            if !ga.is_finite() || !gb.is_finite() || (ga.abs() <= TOLERANCE && gb.abs() <= TOLERANCE) {
                return None;
            }
            if (ga >= 0.0) != (gb >= 0.0) {
                let q = a + (b - a) * (ga / (ga - gb)).clamp(0.0, 1.0);
                ts.push((q - o).dot(d));
            }
        }
    }
    if ts.len() % 2 != 0 {
        return None;
    }
    ts.sort_by(f64::total_cmp);
    Some(ts.chunks(2).filter_map(|c| match c {
        [a, b] => Some((*a, *b)),
        _ => None,
    }).collect())
}

/// Below this |n0 × n1| (the sine of the planes' angle) a plane pair is left to the mesh collision.
const NEAR_PARALLEL: f64 = 1e-6;

/// The exact intersection of two faces as the polylines the mesh collision would chain (each a
/// straight segment here), or `None` for a pair it leaves to the mesh collision.
pub(super) fn exact_intersection<C: Clone + 'static, S: 'static>(face0: &Face<Point3, C, S>, face1: &Face<Point3, C, S>) -> Option<Vec<PolylineCurve<Point3>>>
where S: Clone {
    if !enabled() {
        return None;
    }
    let (p0, n0) = plane_of(&face0.surface())?;
    let (p1, n1) = plane_of(&face1.surface())?;
    let d = n0.cross(n1);
    // Parallel or nearly so (their line far off or ill-conditioned): the mesh collision's, which
    // finds what meets within the faces either way.
    if d.magnitude() < NEAR_PARALLEL {
        return None;
    }
    let d = d.normalize();
    // A point of the line, solved relative to p0 so faces far from the origin keep their digits:
    // o = p0 + a n0 + b n1 with n0·(o - p0) = 0 and n1·(o - p1) = 0.
    let c1 = n1.dot(p1 - p0);
    let n01 = n0.dot(n1);
    let det = 1.0 - n01 * n01;
    let (a, b) = ((-c1 * n01) / det, c1 / det);
    let o = p0 + n0 * a + n1 * b;
    if !o.to_vec().magnitude2().is_finite() {
        return None;
    }
    let in0 = inside_intervals(face0, (p1, n1), o, d)?;
    let in1 = inside_intervals(face1, (p0, n0), o, d)?;
    // Where both run inside, segments touching end to start taken as one (the collision chains them).
    let mut segments: Vec<(f64, f64)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while let (Some(&(a0, a1)), Some(&(b0, b1))) = (in0.get(i), in1.get(j)) {
        let (lo, hi) = (a0.max(b0), a1.min(b1));
        if hi - lo > TOLERANCE {
            match segments.last_mut() {
                Some(last) if (lo - last.1).abs() <= TOLERANCE => last.1 = hi,
                _ => segments.push((lo, hi)),
            }
        }
        if a1 < b1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    #[cfg(any(test, feature = "test-hooks"))]
    EXACT.with(|c| c.set(c.get() + 1));
    Some(segments.into_iter().map(|(lo, hi)| PolylineCurve(vec![o + d * lo, o + d * hi])).collect())
}
