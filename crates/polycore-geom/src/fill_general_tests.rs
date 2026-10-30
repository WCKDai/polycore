use super::*;
use std::f64::consts::{PI, TAU};

const TOL: f64 = 1e-5;
const ANGLE: f64 = 0.05 * PI / 180.0;

/// The surface at fractions `(x, y)` of its domain.
fn at(s: &NurbsSurface, x: f64, y: f64) -> Vec3 {
    let [du, dv] = s.domain();
    s.eval((du[0] + (du[1] - du[0]) * x).min(du[1]), (dv[0] + (dv[1] - dv[0]) * y).min(dv[1])).unwrap()
}

fn spec(points: &[Vec3]) -> FillSpec<'_> {
    FillSpec { tol: TOL, angle_tol: ANGLE, crown: 0.0, points, speed: LEGACY_FILL_SPEED }
}

/// The radial and tangential unit vectors of a cylinder about z at angle `a`.
fn radial(a: f64) -> Vec3 {
    Vec3::new(a.cos(), a.sin(), 0.0)
}
fn around(a: f64) -> Vec3 {
    Vec3::new(-a.sin(), a.cos(), 0.0)
}

/// The open end of a tube about z (radius `r`, its wall below z = 0), one side: G1 (or G2) to
/// the wall, which leaves straight up.
fn tube_end(r: f64, g2: bool) -> impl FnMut(usize, &[f64]) -> Result<Vec<Station>, String> {
    move |_, ts| {
        Ok(ts
            .iter()
            .map(|t| {
                let a = t * TAU;
                let across = Across { leave: Vec3::Z, normal: radial(a), bend: g2.then_some(Vec3::ZERO) };
                Station { p: radial(a) * r, up: around(a).cross(Vec3::Z), across: Some(across), corner: false, clash: false }
            })
            .collect())
    }
}

/// A window in the wall of a cylinder about z (radius `r`): `a` either side of the x axis, `h`
/// high, its four sides G1 to the wall around it (the same cylinder).
fn window(r: f64, a: f64, h: f64) -> impl FnMut(usize, &[f64]) -> Result<Vec<Station>, String> {
    move |k, ts| {
        Ok(ts
            .iter()
            .map(|&t| {
                let (angle, z, tangent, leave) = match k {
                    0 => (-a + 2.0 * a * t, 0.0, around(-a + 2.0 * a * t), Vec3::Z),
                    1 => (a, h * t, Vec3::Z, around(a) * -1.0),
                    2 => (a - 2.0 * a * t, h, around(a - 2.0 * a * t) * -1.0, Vec3::Z * -1.0),
                    _ => (-a, h - h * t, Vec3::Z * -1.0, around(-a)),
                };
                let p = radial(angle) * r + Vec3::Z * z;
                let up = tangent.cross(leave);
                Station { p, up, across: Some(Across { leave, normal: radial(angle), bend: None }), corner: t == 0.0, clash: false }
            })
            .collect())
    }
}

#[test]
fn a_tube_end_capped_g1_meets_the_wall_and_rises_over_it() {
    for g2 in [false, true] {
        let f = fill_general(1, &mut tube_end(10.0, g2), &spec(&[])).unwrap();
        eprintln!(
            "tube end (g2 {g2}): {:?} spans, {} stations, off the rim {:e}, normals within {:.4}°, curvature mismatch {:e}",
            f.spans,
            f.stations,
            f.deviation,
            f.normal_deviation.to_degrees(),
            f.curvature_mismatch
        );
        assert!(f.deviation <= TOL && f.normal_deviation <= ANGLE);
        // The middle rises over the rim (the cap leaves the wall upwards).
        let mid = at(&f.surface, 0.5, 0.5);
        eprintln!("the cap's middle: {mid:?}");
        assert!(mid.z > 2.0 && mid.x.hypot(mid.y) < 1.0, "{mid:?}");
    }
}

#[test]
fn a_window_in_a_cylinder_wrapping_past_half_way_fills_close_to_the_cylinder() {
    // 240° around: no plane sees it as a height field.
    let (r, a, h) = (10.0, 2.0 * PI / 3.0, 10.0);
    let f = fill_general(4, &mut window(r, a, h), &spec(&[])).unwrap();
    eprintln!("window: {:?} spans, off {:e}, normals within {:.4}°", f.spans, f.deviation, f.normal_deviation.to_degrees());
    assert!(f.deviation <= TOL && f.normal_deviation <= ANGLE);
    // Inside, the fill stays near the cylinder it was cut from (it is the least-bending surface
    // with the cylinder's boundary data, not the cylinder itself).
    let mut worst = 0.0f64;
    for k in 1..20 {
        for j in 1..20 {
            let (ang, z) = (-a + 2.0 * a * k as f64 / 20.0, h * j as f64 / 20.0);
            // The fill's point nearest this cylinder point, by a coarse search.
            let target = radial(ang) * r + Vec3::Z * z;
            let mut best = f64::INFINITY;
            for u in 0..=120 {
                for v in 0..=120 {
                    best = best.min(at(&f.surface, u as f64 / 120.0, v as f64 / 120.0).dist(target));
                }
            }
            worst = worst.max(best);
        }
    }
    eprintln!("window: the cylinder is within {worst} of the fill");
    assert!(worst < 0.25, "{worst}");
}

#[test]
fn a_planar_polygon_is_its_plane_and_a_point_lifts_it() {
    // An L-shaped hexagon (one reflex corner) in z = 0, its neighbours standing up around it.
    let c = [[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0], [10.0, 20.0], [0.0, 20.0]];
    let mut stations = |k: usize, ts: &[f64]| -> Result<Vec<Station>, String> {
        let (Some(a), Some(b)) = (c.get(k), c.get((k + 1) % 6)) else { return Err("no side".into()) };
        let tangent = Vec3::new(b[0] - a[0], b[1] - a[1], 0.0).normalized().ok_or("no tangent")?;
        let inward = Vec3::Z.cross(tangent);
        Ok(ts
            .iter()
            .map(|t| {
                let p = Vec3::new(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, 0.0);
                Station { p, up: Vec3::Z, across: Some(Across { leave: inward, normal: Vec3::Z, bend: None }), corner: *t == 0.0, clash: false }
            })
            .collect())
    };
    let f = fill_general(6, &mut stations, &spec(&[])).unwrap();
    let mut off = 0.0f64;
    for i in 0..=20 {
        for j in 0..=20 {
            off = off.max(at(&f.surface, i as f64 / 20.0, j as f64 / 20.0).z.abs());
        }
    }
    eprintln!("L: {:?} spans, the plane within {off:e}", f.spans);
    assert!(off < 1e-8, "{off}");
    // G0 with a point 3 above the inside of the L: the surface passes through it.
    let mut g0 = |k: usize, ts: &[f64]| stations(k, ts).map(|v| v.into_iter().map(|s| Station { across: None, ..s }).collect());
    let point = [Vec3::new(5.0, 5.0, 3.0)];
    let f = fill_general(6, &mut g0, &spec(&point)).unwrap();
    eprintln!("L through a point: {:?} spans, off the loop {:e}, off the point {:e}", f.spans, f.deviation, f.point_deviation);
    assert!(f.deviation <= TOL && f.point_deviation <= TOL);
}

#[test]
fn hostile_loops_and_specs_are_refused() {
    let ok = |s: &FillSpec| fill_general(1, &mut tube_end(10.0, false), s);
    for (tol, angle, crown) in
        [(0.0, ANGLE, 0.0), (f64::NAN, ANGLE, 0.0), (TOL, -1.0, 0.0), (TOL, ANGLE, f64::NAN), (TOL, ANGLE, -0.95), (TOL, ANGLE, 1e300)]
    {
        assert!(ok(&FillSpec { tol, angle_tol: angle, crown, points: &[], speed: LEGACY_FILL_SPEED }).is_err(), "{tol} {angle} {crown}");
    }
    assert!(ok(&spec(&[Vec3::new(f64::NAN, 0.0, 0.0)])).is_err());
    assert!(ok(&spec(&vec![Vec3::ZERO; MAX_FILL_POINTS + 1])).is_err());
    // A point not over the inside of the loop.
    assert!(ok(&spec(&[Vec3::new(30.0, 0.0, 0.0)])).is_err());
    assert!(fill_general(0, &mut tube_end(10.0, false), &spec(&[])).is_err());
    assert!(fill_general(10_000, &mut tube_end(10.0, false), &spec(&[])).is_err());
    assert!(fill_general(1, &mut |_, _| Err("no".into()), &spec(&[])).is_err());
    assert!(
        fill_general(
            1,
            &mut |_, ts| Ok(vec![Station { p: Vec3::ZERO, up: Vec3::Z, across: None, corner: false, clash: false }; ts.len()]),
            &spec(&[])
        )
        .is_err()
    );
    // A loop that runs out and straight back.
    let mut spike = |_: usize, ts: &[f64]| -> Result<Vec<Station>, String> {
        Ok(ts
            .iter()
            .map(|t| Station { p: Vec3::X * (1.0 - (2.0 * t - 1.0f64).abs()) * 10.0, up: Vec3::Z, across: None, corner: false, clash: false })
            .collect())
    };
    assert!(fill_general(1, &mut spike, &spec(&[])).is_err());
    // A figure eight: its development crosses itself.
    let mut eight = |_: usize, ts: &[f64]| -> Result<Vec<Station>, String> {
        Ok(ts
            .iter()
            .map(|t| {
                let a = t * TAU;
                Station { p: Vec3::new(10.0 * a.sin(), 5.0 * (2.0 * a).sin(), 0.0), up: Vec3::Z, across: None, corner: false, clash: false }
            })
            .collect())
    };
    assert!(fill_general(1, &mut eight, &spec(&[])).is_err());
    // Huge and tiny loops do not panic.
    for scale in [1e300, 1e-300] {
        let mut s = |_: usize, ts: &[f64]| -> Result<Vec<Station>, String> {
            Ok(ts.iter().map(|t| Station { p: radial(t * TAU) * scale, up: Vec3::Z, across: None, corner: false, clash: false }).collect())
        };
        let _ = fill_general(1, &mut s, &spec(&[]));
    }
}

/// The smallest radius of curvature over the surface, by finite differences of its first and
/// second derivatives on a 61 × 61 grid just inside its domain.
fn min_radius(s: &NurbsSurface) -> f64 {
    let [du, dv] = s.domain();
    let (hu, hv) = ((du[1] - du[0]) * 1e-4, (dv[1] - dv[0]) * 1e-4);
    let e = |a: f64, b: f64| s.eval(a, b).unwrap();
    let mut least = f64::INFINITY;
    for i in 0..=60 {
        for j in 0..=60 {
            let u = du[0] + (du[1] - du[0]) * (0.002 + 0.996 * i as f64 / 60.0);
            let v = dv[0] + (dv[1] - dv[0]) * (0.002 + 0.996 * j as f64 / 60.0);
            let p = e(u, v);
            let su = (e(u + hu, v) - e(u - hu, v)) * (0.5 / hu);
            let sv = (e(u, v + hv) - e(u, v - hv)) * (0.5 / hv);
            let suu = (e(u + hu, v) - p * 2.0 + e(u - hu, v)) * (1.0 / (hu * hu));
            let svv = (e(u, v + hv) - p * 2.0 + e(u, v - hv)) * (1.0 / (hv * hv));
            let suv = (e(u + hu, v + hv) - e(u + hu, v - hv) - e(u - hu, v + hv) + e(u - hu, v - hv)) * (0.25 / (hu * hv));
            let Some(n) = su.cross(sv).normalized() else { continue };
            let (ee, ff, gg) = (su.dot(su), su.dot(sv), sv.dot(sv));
            let (l, m, nn) = (suu.dot(n), suv.dot(n), svv.dot(n));
            let det = ee * gg - ff * ff;
            let (h, k) = ((ee * nn - 2.0 * ff * m + gg * l) / (2.0 * det), (l * nn - m * m) / det);
            let most = h.abs() + (h * h - k).max(0.0).sqrt();
            if most > 0.0 {
                least = least.min(1.0 / most);
            }
        }
    }
    least
}

/// The open end of an elliptical tube about z (half-axes `a`, `b`, its wall below z = 0): G1 to
/// the wall, which leaves straight up. Its rim is tightest at the ends of the long axis, b²/a.
fn ellipse_end(a: f64, b: f64) -> impl FnMut(usize, &[f64]) -> Result<Vec<Station>, String> {
    move |_, ts| {
        Ok(ts
            .iter()
            .map(|t| {
                let th = t * TAU;
                let tangent = Vec3::new(-a * th.sin(), b * th.cos(), 0.0).normalized().unwrap_or(Vec3::X);
                let out = tangent.cross(Vec3::Z);
                let across = Across { leave: Vec3::Z, normal: out, bend: None };
                Station { p: Vec3::new(a * th.cos(), b * th.sin(), 0.0), up: out, across: Some(across), corner: false, clash: false }
            })
            .collect())
    }
}

#[test]
fn a_cap_leaving_at_twice_the_rims_speed_is_fair() {
    // A G1 cap on a tube of radius 10: at the boundary's own speed (patches saved before the
    // speed was) it rises only 5, half a hemisphere, and curls to a radius of 2.6 at the rim; at
    // twice that speed (new patches) it rises 10 and its tightest radius is 7 (a hemisphere's is
    // 10, which a bicubic does not reach).
    let legacy = fill_general(1, &mut tube_end(10.0, false), &spec(&[])).unwrap();
    let fair = fill_general(1, &mut tube_end(10.0, false), &FillSpec { speed: FILL_SPEED, ..spec(&[]) }).unwrap();
    let (r0, r1) = (min_radius(&legacy.surface), min_radius(&fair.surface));
    let (z0, z1) = (at(&legacy.surface, 0.5, 0.5).z, at(&fair.surface, 0.5, 0.5).z);
    eprintln!("tube cap: legacy rises {z0:.3}, smallest radius {r0:.3}; fair rises {z1:.3}, smallest radius {r1:.3}");
    assert!((z0 - 5.0).abs() < 0.2 && r0 < 3.0, "{z0} {r0}");
    assert!((z1 - 10.0).abs() < 0.5 && r1 > 6.5, "{z1} {r1}");
    assert!(fair.deviation <= TOL && fair.normal_deviation <= ANGLE);
    // An elliptical rim 20 × 10, its ends' radius 2.5 (a snout's tight corner): the legacy cap
    // curls to 0.92 there, the fair one to 1.63.
    let loose = |speed| FillSpec { tol: 1e-4, speed, ..spec(&[]) };
    let legacy = fill_general(1, &mut ellipse_end(10.0, 5.0), &loose(LEGACY_FILL_SPEED)).unwrap();
    let fair = fill_general(1, &mut ellipse_end(10.0, 5.0), &loose(FILL_SPEED)).unwrap();
    let (r0, r1) = (min_radius(&legacy.surface), min_radius(&fair.surface));
    eprintln!("ellipse cap: smallest radius legacy {r0:.3}, fair {r1:.3}");
    assert!(r0 < 1.0 && r1 > 1.5, "{r0} {r1}");
    // Speeds out of range are refused.
    for bad in [0.0, 5.0, f64::NAN] {
        assert!(fill_general(1, &mut tube_end(10.0, false), &FillSpec { speed: bad, ..spec(&[]) }).is_err(), "{bad}");
    }
}
