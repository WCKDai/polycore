//! A tool touching the target along a curve tangent to both, which the boolean cannot trace:
//! a sphere capping a cylinder of its own radius (a capsule, a rounded toe), a ball centred on
//! a flat face it meets along its equator.
//!
//! The contact curve lies in a plane of the target. Split along that plane, each half of the
//! tool meets the target either face to face (in the plane), which the boolean takes, or only
//! along the curve from inside, where the union changes nothing. So when a union has failed,
//! it is tried once more with the tool cut by each plane of the target that runs through it,
//! the halves joined one after the other. The result's volume must still be plausible.

use solidcraft_geom::Plane;

use crate::body::Body;
use crate::{BoolOp, Result};

/// Most planes of the target tried.
const MAX_PLANES: usize = 32;

thread_local! {
    /// Set while the halves are joined (no second round of splitting).
    static SPLITTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The union of `a` and `b` with `b` split by a plane of `a` (or the other way round), if one
/// gives a result of plausible volume. `None` when no plane helps.
pub(crate) fn union(a: &Body, b: &Body, boolean: &dyn Fn(&Body, &Body, BoolOp) -> Result<Option<Body>>) -> Option<Body> {
    if SPLITTING.with(|s| s.replace(true)) {
        return None;
    }
    let found = [(a, b), (b, a)].into_iter().find_map(|(target, tool)| by_planes(target, tool, a, b, boolean));
    SPLITTING.with(|s| s.set(false));
    found
}

fn by_planes(target: &Body, tool: &Body, a: &Body, b: &Body, boolean: &dyn Fn(&Body, &Body, BoolOp) -> Result<Option<Body>>) -> Option<Body> {
    let size = a.size().max(b.size());
    let tol = size * 1e-6;
    let mesh = tool.tessellate_with(size * 1e-3, true).ok()?;
    let volume = |x: &Body| crate::measure(x).ok().map(|m| m.volume);
    let (va, vb) = (volume(a)?, volume(b)?);
    let mut planes: Vec<Plane> = Vec::new();
    for i in 0..target.face_count().min(1_000) {
        let Some(p) = target.planar_face(i) else { continue };
        let n = p.normal();
        let d = n.dot(p.origin);
        // One try per plane (a box's opposite faces are two).
        if planes.iter().any(|q| (q.normal().dot(n).abs() > 1.0 - 1e-9) && (q.normal().dot(n) * q.normal().dot(q.origin) - d).abs() < tol) {
            continue;
        }
        // Only a plane the tool lies on both sides of.
        let (lo, hi) = mesh.positions.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
            let s = n.dot(*x) - d;
            (lo.min(s), hi.max(s))
        });
        if lo < -tol * 10.0 && hi > tol * 10.0 {
            planes.push(p);
        }
        if planes.len() >= MAX_PLANES {
            break;
        }
    }
    planes.iter().find_map(|plane| {
        let halves = crate::ops::split_by_plane(tool, plane).ok()?;
        if halves.len() < 2 {
            return None;
        }
        let mut acc = target.clone();
        for h in &halves {
            acc = boolean(&acc, h, BoolOp::Union).ok()??;
        }
        // A union holds at least the larger operand and at most both.
        let v = volume(&acc)?;
        let slack = 2e-3 * (va + vb);
        (v >= va.max(vb) - slack && v <= va + vb + slack && acc.validity().is_empty()).then(|| acc.without_history())
    })
}

#[cfg(test)]
mod tests {
    use solidcraft_geom::Vec3;

    use crate::{BoolOp, boolean, cylinder, measure, sphere};

    fn sound(b: &crate::Body, want: f64) {
        assert!(b.validity().is_empty(), "{:?}", b.validity());
        let v = measure(b).unwrap().volume;
        assert!((v - want).abs() / want < 1e-3, "{v} vs {want}");
        let mesh = b.tessellate_with(0.01, true).unwrap();
        assert_eq!(crate::curveblend::tests::open_and_over_shared(&mesh, 1e-4), (0, 0), "open and over-shared mesh sides");
    }

    #[test]
    fn a_ball_caps_a_rod_of_its_radius() {
        // TinyRex's toe: a 3.5 mm rod 28 mm long, a 3.5 mm ball on its end.
        let pi = std::f64::consts::PI;
        let (r, l, o) = (3.5, 28.0, Vec3::new(32.0, 35.0, -121.655));
        let rod = cylinder(o, Vec3::X, r, l).unwrap();
        let want = pi * r * r * l + 2.0 / 3.0 * pi * r * r * r;
        for end in [o + Vec3::X * l, o] {
            let capsule = boolean(&rod, &sphere(end, r).unwrap(), BoolOp::Union).unwrap().unwrap();
            sound(&capsule, want);
        }
    }

    #[test]
    fn a_link_rounded_at_both_ends() {
        let pi = std::f64::consts::PI;
        let (r, l) = (5.0, 40.0);
        let rod = cylinder(Vec3::ZERO, Vec3::Z, r, l).unwrap();
        let one = boolean(&rod, &sphere(Vec3::ZERO, r).unwrap(), BoolOp::Union).unwrap().unwrap();
        let both = boolean(&one, &sphere(Vec3::new(0.0, 0.0, l), r).unwrap(), BoolOp::Union).unwrap().unwrap();
        sound(&both, pi * r * r * l + 4.0 / 3.0 * pi * r * r * r);
    }
}
