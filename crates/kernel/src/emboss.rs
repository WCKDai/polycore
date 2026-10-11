//! Exact radial embossing of orthogonal profiles on complete cylindrical walls.
use crate::body::{Body, Solid, from_p3, p3};
use crate::offset::{Surf, surf_of};
use crate::{KernelError, Result, guard};
use solvecraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling::{self as mt, Transformed, builder};

fn fail(s: &str) -> KernelError {
    KernelError::Invalid(format!("cylinder emboss: {s}"))
}
fn angle(p: Vec3, o: Vec3, axis: Vec3, radial: Vec3, tangent: Vec3) -> f64 {
    let w = p - o;
    let w = w - axis * w.dot(axis);
    w.dot(tangent).atan2(w.dot(radial))
}
fn unwrap(a: f64, around: f64) -> f64 {
    around + (a - around + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

/// Certify a tessellated pick and project it onto the cylinder used by emboss.
pub fn cylinder_emboss_anchor(body: &Body, face: usize, at: Vec3) -> Result<Vec3> {
    if !at.is_finite() || at.len() > 1e9 {
        return Err(fail("invalid selected face point"));
    }
    guard("cylinder emboss anchor", || {
        let f = body.solid.face_iter().nth(face).ok_or_else(|| fail("selected face"))?;
        let tol = (body.size() * 1e-8).max(1e-8);
        let Some(Surf::Cylinder { o, a, r, convex: true }) = surf_of(f, tol) else {
            return Err(fail("selected face is not an exterior cylinder"));
        };
        let w = at - o;
        let radial = w - a * w.dot(a);
        if (radial.len() - r).abs() > (body.size() * 1e-3).max(1e-3).min(r * 0.05) {
            return Err(fail("selected point is too far from cylinder"));
        }
        Ok(o + a * w.dot(a) + radial.normalized().ok_or_else(|| fail("radial point"))? * r)
    })
}

/// Wrap sketch arc length around an exterior cylinder, retaining exact radial depth.
/// Profiles must contain orthogonal lines and lie inside one uninterrupted wall face.
pub fn emboss_cylinder(body: &Body, plane: &Plane, regions: &[Region2], depth: f64) -> Result<Body> {
    emboss_impl(body, None, plane, regions, depth)
}

/// As [`emboss_cylinder`], requiring the saved selected point to identify that wall.
pub fn emboss_cylinder_at(body: &Body, at: Vec3, plane: &Plane, regions: &[Region2], depth: f64) -> Result<Body> {
    if !at.is_finite() || at.len() > 1e9 {
        return Err(fail("invalid selected face point"));
    }
    emboss_impl(body, Some(at), plane, regions, depth)
}

fn emboss_impl(body: &Body, anchor: Option<Vec3>, plane: &Plane, regions: &[Region2], depth: f64) -> Result<Body> {
    body.require_brep("cylinder emboss")?;
    if !depth.is_finite() || depth.abs() < 1e-6 || depth.abs() > 1e6 || regions.is_empty() || regions.len() > 64 {
        return Err(fail("needs finite nonzero depth and 1..64 profiles"));
    }
    if !plane.origin.is_finite()
        || !plane.x.is_finite()
        || !plane.y.is_finite()
        || (plane.x.len() - 1.0).abs() > 1e-8
        || (plane.y.len() - 1.0).abs() > 1e-8
        || plane.x.dot(plane.y).abs() > 1e-8
    {
        return Err(fail("invalid sketch frame"));
    }
    guard("cylinder emboss", || {
        if body.solid.boundaries().len() != 1 || body.solid.face_iter().take(17).count() > 16 || body.solid.edge_iter().take(257).count() > 256 {
            return Err(fail("requires a complete cylinder or coaxial sleeve"));
        }
        let faces: Vec<_> = body.solid.face_iter().cloned().collect();
        let tol = (body.size() * 1e-8).max(1e-8);
        let surfaces: Vec<_> =
            faces.iter().map(|f| surf_of(f, tol).ok_or_else(|| fail("requires only cylindrical walls and planar endcaps"))).collect::<Result<_>>()?;
        let mut chosen = None;
        for (fi, s) in surfaces.iter().enumerate() {
            let Surf::Cylinder { o, a, r, convex: true } = *s else { continue };
            let w = plane.origin - o;
            let radial = w - a * w.dot(a);
            if (radial.len() - r).abs() > tol * 10.0 || plane.normal().dot(radial / r) < 1.0 - 1e-8 {
                continue;
            }
            let radial = radial.normalized().ok_or_else(|| fail("radial frame"))?;
            let tangent = a.cross(radial);
            let f = faces.get(fi).ok_or_else(|| fail("face"))?;
            if f.boundaries().len() != 1 || f.edge_iter().count() != 4 {
                continue;
            }
            let mut angular = None;
            for e in f.edge_iter() {
                if let Some(Some(mid)) = crate::offset::arc_mid(&e, tol * 10.0) {
                    let am = angle(mid, o, a, radial, tangent);
                    let aa = unwrap(angle(from_p3(e.front().point()), o, a, radial, tangent), am);
                    let ab = unwrap(angle(from_p3(e.back().point()), o, a, radial, tangent), am);
                    angular = Some((aa.min(ab), aa.max(ab)));
                    break;
                }
            }
            let Some((alo, ahi)) = angular else { continue };
            if alo >= -tol / r || ahi <= tol / r {
                continue;
            }
            if chosen.is_some() {
                return Err(fail("ambiguous cylindrical face"));
            }
            chosen = Some((fi, o, a, r, radial, tangent, alo, ahi));
        }
        let (fi, o, axis, r, radial, tangent, alo, ahi) =
            chosen.ok_or_else(|| fail("sketch must be tangent to one complete exterior cylindrical wall, clear of its seam"))?;
        let mut caps = Vec::new();
        let mut inner: f64 = 0.0;
        for s in &surfaces {
            match *s {
                Surf::Plane { n, d } if n.dot(axis).abs() > 1.0 - 1e-8 => caps.push((d - n.dot(o)) / n.dot(axis)),
                Surf::Cylinder { o: co, a: ca, r: cr, convex }
                    if ca.dot(axis).abs() > 1.0 - 1e-8 && (co - o - axis * (co - o).dot(axis)).len() < tol * 10.0 =>
                {
                    if convex && (cr - r).abs() > tol * 10.0 {
                        return Err(fail("interrupted or stepped cylinder"));
                    }
                    if !convex {
                        inner = inner.max(cr);
                    }
                }
                _ => return Err(fail("requires a complete cylinder or coaxial sleeve")),
            }
        }
        if caps.len() != 2 {
            return Err(fail("requires two planar endcaps"));
        }
        let lo = caps.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = caps.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let rr = r + depth;
        if rr <= inner + tol * 10.0 {
            return Err(fail("depth reaches the axis or inner wall"));
        }
        if let Some(at) = anchor {
            let h = (at - o).dot(axis);
            let v = at - o - axis * h;
            let theta = angle(at, o, axis, radial, tangent);
            if (v.len() - r).abs() > tol * 10.0 || theta <= alo + tol / r || theta >= ahi - tol / r || h <= lo + tol || h >= hi - tol {
                return Err(fail("saved selection no longer identifies the sketch's cylindrical face"));
            }
        }
        let z0 = (plane.origin - o).dot(axis);
        let map2 = |p: Vec2| Vec2::new(plane.dir_to_world(p).dot(tangent), z0 + plane.dir_to_world(p).dot(axis));
        let mut count = 0usize;
        let mut mapped = Vec::new();
        for reg in regions {
            let mut loops = Vec::new();
            for lp in std::iter::once(&reg.outer).chain(&reg.holes) {
                count = count.saturating_add(lp.segs.len());
                if count > 4096 || lp.segs.len() < 4 || !lp.is_closed(tol) {
                    return Err(fail("profile geometry budget or open loop"));
                }
                let mut segs = Vec::new();
                for s in &lp.segs {
                    let Seg2::Line { a, b } = *s else {
                        return Err(fail("only orthogonal line profiles are supported on cylinders"));
                    };
                    if !a.is_finite() || !b.is_finite() || a.len() > 1e7 || b.len() > 1e7 {
                        return Err(fail("nonfinite or excessive profile coordinates"));
                    }
                    let (a, b) = (map2(a), map2(b));
                    let d = b - a;
                    if d.len() < tol || (d.x.abs() > tol && d.y.abs() > tol) {
                        return Err(fail("profile edges must follow the cylinder axis or circumference"));
                    }
                    for p in [a, b] {
                        if p.x / r <= alo + tol / r || p.x / r >= ahi - tol / r || p.y <= lo + tol || p.y >= hi - tol {
                            return Err(fail("profile crosses a cylinder seam or endcap boundary"));
                        }
                    }
                    segs.push(Seg2::Line { a, b });
                }
                loops.push(Loop2 { segs });
            }
            let outer = loops.first().ok_or_else(|| fail("profile"))?.ccw();
            let holes = loops.iter().skip(1).map(|l| l.ccw().reversed()).collect();
            mapped.push(Region2 { outer, holes });
        }
        validate_profiles(&mapped, tol)?;
        let at = |p: Vec2, radius: f64| o + axis * p.y + (radial * (p.x / r).cos() + tangent * (p.x / r).sin()) * radius;
        let selected = faces.get(fi).ok_or_else(|| fail("face"))?;
        let mut base_patches = vec![selected.absolute_boundaries().clone()];
        let mut hole_patches = Vec::new();
        let mut footprints = Vec::new();
        let mut added = Vec::new();
        let k = rr / r;
        let coord = |v: Vec3| [v.x, v.y, v.z];
        let av = coord(axis);
        let ov = coord(o);
        let mut m = [[0.0; 4]; 4];
        for (j, col) in m.iter_mut().take(3).enumerate() {
            for (i, slot) in col.iter_mut().take(3).enumerate() {
                *slot = if i == j { k } else { 0.0 } + (1.0 - k) * av.get(i).copied().unwrap_or(0.0) * av.get(j).copied().unwrap_or(0.0);
            }
        }
        for (i, oi) in ov.iter().enumerate() {
            let translation =
                *oi - (0..3).map(|j| m.get(j).and_then(|row| row.get(i)).copied().unwrap_or(0.0) * ov.get(j).copied().unwrap_or(0.0)).sum::<f64>();
            if let Some(slot) = m.get_mut(3).and_then(|row| row.get_mut(i)) {
                *slot = translation;
            }
        }
        if let Some(slot) = m.get_mut(3).and_then(|row| row.get_mut(3)) {
            *slot = 1.0;
        }
        let [c0, c1, c2, c3] = m;
        let [a, b, c, d] = c0;
        let [e, f, g, h] = c1;
        let [i, j, l, n] = c2;
        let [p, q, s, t] = c3;
        let mat = mt::Matrix4::new(a, b, c, d, e, f, g, h, i, j, l, n, p, q, s, t);
        let raised = selected.oriented_surface().transformed(mat);
        for reg in &mapped {
            let mut cap = Vec::new();
            for (li, lp) in std::iter::once(&reg.outer).chain(&reg.holes).enumerate() {
                let (base, top, walls) = wrapped_loop(lp, r, rr, &at)?;
                if li == 0 {
                    footprints.push((lp, base.inverse()));
                } else {
                    let index = base_patches.len();
                    base_patches.push(vec![base.inverse()]);
                    hole_patches.push((lp, index));
                }
                cap.push(top);
                added.extend(walls);
            }
            added.push(mt::Face::try_new(cap, raised.clone()).map_err(|e| fail(&e.to_string()))?);
        }
        // An island inside a profile hole cuts that hole's original-radius patch,
        // rather than adding another hole to the already-cut exterior patch.
        for (lp, wire) in footprints {
            let probe = lp.segs.first().ok_or_else(|| fail("empty profile"))?.start();
            let owner = hole_patches
                .iter()
                .filter(|(hole, _)| hole.contains(probe))
                .min_by(|(a, _), (b, _)| a.signed_area().abs().total_cmp(&b.signed_area().abs()))
                .map(|(_, index)| *index)
                .unwrap_or(0);
            base_patches.get_mut(owner).ok_or_else(|| fail("profile ownership"))?.push(wire);
        }
        let mut all: Vec<_> = faces.iter().enumerate().filter(|(i, _)| *i != fi).map(|(_, f)| f.clone()).collect();
        for patch in base_patches {
            all.push(mt::Face::try_new(patch, selected.oriented_surface()).map_err(|e| fail(&e.to_string()))?);
        }
        all.extend(added);
        let solid = Solid::try_new(vec![all.into()]).map_err(|e| fail(&e.to_string()))?;
        let result = Body::new(solid)?;
        if !result.validity().is_empty() {
            return Err(fail("rebuilt cylinder is not a closed manifold"));
        }
        Ok(result)
    })
}

fn wrapped_loop(lp: &Loop2, r: f64, rr: f64, at: &impl Fn(Vec2, f64) -> Vec3) -> Result<(mt::Wire, mt::Wire, Vec<mt::Face>)> {
    let base: Vec<_> = lp.segs.iter().map(|s| builder::vertex(p3(at(s.start(), r)))).collect();
    let top: Vec<_> = lp.segs.iter().map(|s| builder::vertex(p3(at(s.start(), rr)))).collect();
    let radial: Vec<_> = base.iter().zip(&top).map(|(a, b)| builder::line(a, b)).collect();
    let mut be = Vec::new();
    let mut te = Vec::new();
    let mut walls = Vec::new();
    for (i, s) in lp.segs.iter().enumerate() {
        let next = (i + 1) % lp.segs.len();
        let (Some(a), Some(b), Some(c), Some(d), Some(ra), Some(rb)) =
            (base.get(i), base.get(next), top.get(i), top.get(next), radial.get(i), radial.get(next))
        else {
            return Err(fail("loop topology"));
        };
        let circular = (s.end().x - s.start().x).abs() > 1e-10;
        let eb = if circular { builder::circle_arc(a, b, p3(at(s.mid(), r))) } else { builder::line(a, b) };
        let et = if circular { builder::circle_arc(c, d, p3(at(s.mid(), rr))) } else { builder::line(c, d) };
        let w: mt::Wire = vec![eb.clone(), rb.clone(), et.inverse(), ra.inverse()].into();
        walls.push(builder::try_attach_plane(&[w]).map_err(|e| fail(&e.to_string()))?);
        be.push(eb);
        te.push(et);
    }
    Ok((be.into(), te.into(), walls))
}

fn validate_profiles(regions: &[Region2], tol: f64) -> Result<()> {
    let loops: Vec<_> = regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).collect();
    let edges: Vec<_> = loops.iter().enumerate().flat_map(|(l, lp)| lp.segs.iter().enumerate().map(move |(i, s)| (l, i, *s))).collect();
    for (k, (l, i, s)) in edges.iter().enumerate() {
        for (ll, j, t) in edges.iter().skip(k + 1) {
            let len = loops.get(*l).map(|lp| lp.segs.len()).unwrap_or(0);
            if l == ll && ((*i + 1) % len == *j || (*j + 1) % len == *i) {
                continue;
            }
            let (a, b, c, d) = (s.start(), s.end(), t.start(), t.end());
            let ab = b - a;
            let cd = d - c;
            let cross = ab.cross(cd);
            let intersects = if cross.abs() > tol * tol {
                let u = (c - a).cross(cd) / cross;
                let v = (c - a).cross(ab) / cross;
                (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)
            } else {
                false
            };
            if intersects
                || [a, b].iter().any(|p| t.point_at(t.closest_param(*p)).dist(*p) < tol)
                || [c, d].iter().any(|p| s.point_at(s.closest_param(*p)).dist(*p) < tol)
            {
                return Err(fail("profile boundaries intersect or touch"));
            }
        }
    }
    for (i, r) in regions.iter().enumerate() {
        if r.area() <= tol * tol {
            return Err(fail("profile has no area"));
        }
        for (h, lp) in r.holes.iter().enumerate() {
            if !r.outer.contains(lp.segs.first().ok_or_else(|| fail("empty hole"))?.start()) {
                return Err(fail("hole outside profile"));
            }
            for other in r.holes.iter().skip(h + 1) {
                if lp.contains(other.segs.first().ok_or_else(|| fail("empty hole"))?.start())
                    || other.contains(lp.segs.first().ok_or_else(|| fail("empty hole"))?.start())
                {
                    return Err(fail("nested profile holes"));
                }
            }
        }
        for other in regions.iter().skip(i + 1) {
            if r.contains(other.outer.segs.first().ok_or_else(|| fail("empty profile"))?.start())
                || other.contains(r.outer.segs.first().ok_or_else(|| fail("empty profile"))?.start())
            {
                return Err(fail("overlapping profiles"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> Region2 {
        Region2 { outer: Loop2::polygon(&[Vec2::new(-2., 5.), Vec2::new(2., 5.), Vec2::new(2., 10.), Vec2::new(-2., 10.)]), holes: vec![] }
    }
    fn plane() -> Plane {
        Plane::new(Vec3::Y * 10., -Vec3::X, Vec3::Z).unwrap()
    }
    #[test]
    fn wrapped_nested_island_uses_its_enclosing_hole_patch() {
        let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
        let mut ring = profile();
        ring.holes.push(Loop2::polygon(&[Vec2::new(-1., 6.), Vec2::new(1., 6.), Vec2::new(1., 9.), Vec2::new(-1., 9.)]));
        let island =
            Region2 { outer: Loop2::polygon(&[Vec2::new(-0.5, 6.5), Vec2::new(0.5, 6.5), Vec2::new(0.5, 8.5), Vec2::new(-0.5, 8.5)]), holes: vec![] };
        let before = body.tessellate_with(1e-4, true).unwrap().measure().volume;
        for depth in [1., -1.] {
            for regions in [vec![ring.clone(), island.clone()], vec![island.clone(), ring.clone()]] {
                let result = emboss_cylinder(&body, &plane(), &regions, depth).unwrap();
                assert_eq!(result.shell_count(), 1);
                assert!(result.validity().is_empty());
                let after = result.tessellate_with(1e-4, true).unwrap().measure().volume;
                let expected = 16. * (depth + depth * depth / 20.);
                assert!((after - before - expected).abs() < 0.02, "{} vs {expected}", after - before);
                let mesh = result.tessellate_with(0.001, true).unwrap();
                for (s, raised) in [(0., true), (0.75, false), (1.5, true)] {
                    let radial = -Vec3::X * (s / 10.0_f64).sin() + Vec3::Y * (s / 10.0_f64).cos();
                    let radius = if depth > 0. { 10.5 } else { 9.5 };
                    assert_eq!(mesh.contains(radial * radius + Vec3::Z * 7.5), if depth > 0. { raised } else { !raised });
                }
            }
        }
    }

    #[test]
    fn exact_wrapped_rectangle_is_connected_and_has_radial_depth() {
        let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
        for depth in [1., -1.] {
            let result = emboss_cylinder(&body, &plane(), &[profile()], depth).unwrap();
            assert_eq!(result.shell_count(), 1);
            assert!(result.validity().is_empty());
            let before = body.tessellate_with(1e-4, true).unwrap().measure().volume;
            let after = result.tessellate_with(1e-4, true).unwrap().measure().volume;
            let expected = if depth > 0. { 21. } else { -19. };
            assert!((after - before - expected).abs() < 0.02, "{} vs {expected}", after - before);
            let mesh = result.tessellate_with(0.001, true).unwrap();
            for s in [-1.5, 0., 1.5] {
                let radial = -Vec3::X * (s / 10.0_f64).sin() + Vec3::Y * (s / 10.0_f64).cos();
                assert_eq!(mesh.contains(radial * (if depth > 0. { 10.5 } else { 9.5 }) + Vec3::Z * 7.5), depth > 0.);
            }
        }
    }

    #[test]
    fn wrapped_holes_and_concave_profiles_preserve_material_on_rotated_axes() {
        let origin = Vec3::new(123.0, -456.0, 789.0);
        let axis = Vec3::X;
        let radial = Vec3::Y;
        let tangent = Vec3::Z;
        let body = crate::cylinder(origin, axis, 10.0, 20.0).unwrap();
        let plane = Plane::new(origin + radial * 10.0, tangent, axis).unwrap();
        let mut holed = profile();
        holed.holes.push(Loop2::polygon(&[Vec2::new(-1., 6.), Vec2::new(1., 6.), Vec2::new(1., 9.), Vec2::new(-1., 9.)]));
        let concave = Region2 {
            outer: Loop2::polygon(&[
                Vec2::new(-2., 5.),
                Vec2::new(2., 5.),
                Vec2::new(2., 7.),
                Vec2::new(0., 7.),
                Vec2::new(0., 10.),
                Vec2::new(-2., 10.),
            ]),
            holes: vec![],
        };
        for profile in [holed, concave] {
            for depth in [1., -1.] {
                let result = emboss_cylinder_at(&body, origin + radial * 10.0 + axis * 7.5, &plane, std::slice::from_ref(&profile), depth).unwrap();
                assert_eq!(result.shell_count(), 1);
                let before = body.tessellate_with(1e-4, true).unwrap().measure().volume;
                let after = result.tessellate_with(1e-4, true).unwrap().measure().volume;
                let want = profile.area().abs() * (depth + depth * depth / 20.0);
                assert!((after - before - want).abs() < 0.02, "{} vs {want}", after - before);
                let point = |s: f64, z: f64, r: f64| origin + axis * z + (radial * (s / 10.0).cos() + tangent * (s / 10.0).sin()) * r;
                let mesh = result.tessellate_with(0.001, true).unwrap();
                for (s, z) in [(-1.5, 7.5), (1.5, 7.5), (0.5, 6.5)] {
                    let material = profile.contains(Vec2::new(s, z));
                    assert_eq!(mesh.contains(point(s, z, if depth > 0. { 10.5 } else { 9.5 })), if depth > 0. { material } else { !material });
                }
                let imported = crate::step_import(&crate::step_export(&[&result], "wrapped cylinder").unwrap()).unwrap();
                assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
                assert_eq!(imported.bodies.len(), 1);
                let round = imported.bodies.first().unwrap();
                assert!(round.body.validity().is_empty());
                let roundmesh = round.body.tessellate_with(0.001, true).unwrap();
                assert_eq!(roundmesh.contains(point(-1.5, 7.5, if depth > 0. { 10.5 } else { 9.5 })), depth > 0.);
                let cut = crate::boolean(
                    &round.body,
                    &crate::box_solid(origin + Vec3::new(5., -20., -20.), origin + Vec3::new(10., 20., 20.)).unwrap(),
                    crate::BoolOp::Cut,
                )
                .unwrap()
                .unwrap();
                assert!(!cut.tessellate(0.01).unwrap().contains(point(-1.5, 7.5, 9.0)));
            }
        }
    }

    #[test]
    fn sleeve_depth_and_hostile_profiles_refuse_changes_atomically() {
        let outer = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
        let inner = crate::cylinder(-Vec3::Z, Vec3::Z, 8., 22.).unwrap();
        let sleeve = crate::boolean(&outer, &inner, crate::BoolOp::Cut).unwrap().unwrap();
        let good = emboss_cylinder(&sleeve, &plane(), &[profile()], -1.).unwrap();
        assert_eq!(good.shell_count(), 1);
        let before = crate::step_export(&[&sleeve], "unchanged").unwrap();
        let mut cap = profile();
        cap.outer = Loop2::polygon(&[Vec2::new(-2., 19.), Vec2::new(2., 19.), Vec2::new(2., 21.), Vec2::new(-2., 21.)]);
        let circle = Region2 { outer: Loop2::circle(Vec2::new(0., 7.5), 1.), holes: vec![] };
        let mut crossing = profile();
        crossing.outer = Loop2::polygon(&[Vec2::new(-2., 5.), Vec2::new(2., 10.), Vec2::new(-2., 10.), Vec2::new(2., 5.)]);
        for (profiles, d) in [
            (vec![profile()], -2.),
            (vec![cap], 1.),
            (vec![circle], 1.),
            (vec![crossing], 1.),
            (vec![profile()], f64::NAN),
            (vec![profile(); 65], 1.),
        ] {
            assert!(emboss_cylinder(&sleeve, &plane(), &profiles, d).is_err());
        }
        assert!(emboss_cylinder_at(&sleeve, -Vec3::Y * 10. + Vec3::Z * 7.5, &plane(), &[profile()], 1.).is_err());
        assert_eq!(crate::step_export(&[&sleeve], "unchanged").unwrap(), before);
    }
}
