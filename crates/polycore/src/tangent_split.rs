//! A ball touching the target along a circle tangent to both, which the boolean cannot trace:
//! a sphere capping a cylinder of its own radius (a capsule, a rounded toe or finger).
//!
//! The circle lies in a plane of the target through the ball's centre (the cylinder's end
//! face). Split along that plane, the outer half meets the target face to face, which the
//! boolean takes, and the inner half lies inside it. So when a union with a ball has failed,
//! it is tried once more with the ball cut by each such plane, the halves joined one after the
//! other. Nothing else takes this path: other tools, and planes not through the centre, are
//! left to the boolean's own refusal.

use solidcraft_geom::{Plane, Vec3};

use crate::body::Body;
use crate::{BoolOp, FaceSurface, Result, face_surface};

/// Most planes through the ball's centre tried.
const MAX_PLANES: usize = 4;

thread_local! {
    /// Set while the halves are joined (no second round of splitting).
    static SPLITTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The union of `a` and `b` where one is a ball whose centre lies in a planar face of the
/// other, with the ball split along that face's plane. `None` when that is not the case or
/// no plane gives a valid result of plausible volume.
pub(crate) fn union(a: &Body, b: &Body, boolean: &dyn Fn(&Body, &Body, BoolOp) -> Result<Option<Body>>) -> Option<Body> {
    if SPLITTING.with(|s| s.replace(true)) {
        return None;
    }
    let found = [(a, b), (b, a)].into_iter().find_map(|(target, ball)| by_planes(target, ball, a, b, boolean));
    SPLITTING.with(|s| s.set(false));
    found
}

/// The centre and radius of a body all of whose faces lie on one sphere.
fn ball(b: &Body) -> Option<(Vec3, f64)> {
    let n = b.face_count();
    if n == 0 || n > 8 {
        return None;
    }
    let mut found: Option<(Vec3, f64)> = None;
    for i in 0..n {
        let Some(FaceSurface::Sphere { center, radius }) = face_surface(b, i) else { return None };
        match found {
            None => found = Some((center, radius)),
            Some((c, r)) if c.dist(center) <= r * 1e-9 && (r - radius).abs() <= r * 1e-9 => {}
            Some(_) => return None,
        }
    }
    found
}

fn by_planes(target: &Body, tool: &Body, a: &Body, b: &Body, boolean: &dyn Fn(&Body, &Body, BoolOp) -> Result<Option<Body>>) -> Option<Body> {
    let (center, radius) = ball(tool)?;
    let tol = radius * 1e-7;
    let volume = |x: &Body| crate::measure(x).ok().map(|m| m.volume);
    let (va, vb) = (volume(a)?, volume(b)?);
    let mut planes: Vec<Plane> = Vec::new();
    for i in 0..target.face_count().min(1_000) {
        let Some(p) = target.planar_face(i) else { continue };
        let n = p.normal();
        if (n.dot(center - p.origin)).abs() > tol {
            continue;
        }
        if planes.iter().any(|q| q.normal().dot(n).abs() > 1.0 - 1e-9) {
            continue;
        }
        planes.push(p);
        if planes.len() >= MAX_PLANES {
            break;
        }
    }
    planes.iter().find_map(|plane| {
        let halves = crate::ops::split_by_plane(tool, plane).ok()?;
        if halves.len() != 2 {
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
