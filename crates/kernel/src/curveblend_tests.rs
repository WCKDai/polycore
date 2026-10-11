//! Independent analytic reference: the centre of a ball tangent to two perpendicular
//! cylinders lies on the intersection of their radius-offset cylinders.

use mt::{BoundedCurve, ParametricCurve, ParametricSurface, ParametricSurface3D, SearchNearestParameter};

use super::*;
use crate::{Body, BoolOp, boolean, cylinder, fillet};

fn tee(scale: f64, origin: Vec3) -> Body {
    let main = cylinder(origin + Vec3::new(-40.0, 0.0, 0.0) * scale, Vec3::X, 15.0 * scale, 80.0 * scale).unwrap();
    let branch = cylinder(origin, Vec3::Z, 10.0 * scale, 35.0 * scale).unwrap();
    boolean(&main, &branch, BoolOp::Union).unwrap().unwrap()
}

fn analytic_section(theta: f64, scale: f64, origin: Vec3) -> (Vec3, Vec3, Vec3) {
    let centre = Vec3::new(13.0 * theta.cos(), 13.0 * theta.sin(), (324.0 - 169.0 * theta.sin().powi(2)).sqrt());
    let main = Vec3::new(centre.x, centre.y * 15.0 / 18.0, centre.z * 15.0 / 18.0);
    let branch = Vec3::new(centre.x * 10.0 / 13.0, centre.y * 10.0 / 13.0, centre.z);
    (origin + centre * scale, origin + main * scale, origin + branch * scale)
}

fn nearest_blend(surfaces: &[mt::Surface], point: Vec3) -> (f64, Vec3) {
    surfaces
        .iter()
        .filter_map(|s| {
            let (u, v) = s.search_nearest_parameter(p3(point), None, 100)?;
            let actual = from_p3(s.subs(u, v));
            let n = s.normal(u, v);
            Some((actual.dist(point), Vec3::new(n.x, n.y, n.z).normalized()?))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap()
}

fn distance_to_analytic_arc(theta: f64, point: Vec3) -> f64 {
    let (centre, main, branch) = analytic_section(theta, 1.0, Vec3::ZERO);
    let a = (main - centre).normalized().unwrap();
    let b = (branch - centre).normalized().unwrap();
    let plane = a.cross(b).normalized().unwrap();
    let radial = point - centre;
    let projected = radial - plane * radial.dot(plane);
    let direction = projected.normalized().unwrap();
    let angle = a.dot(b).clamp(-1.0, 1.0).acos();
    let from_main = a.dot(direction).clamp(-1.0, 1.0).acos();
    let from_branch = b.dot(direction).clamp(-1.0, 1.0).acos();
    if (from_main + from_branch - angle).abs() < 1e-7 { point.dist(centre + direction * 3.0) } else { point.dist(main).min(point.dist(branch)) }
}

fn distance_to_analytic_band(point: Vec3) -> f64 {
    // The arc's XY azimuth stays between its two contacts' azimuths. Their difference
    // for this tee is less than 0.1 rad; bracket generously around the query azimuth.
    let azimuth = point.y.atan2(point.x);
    let (mut lo, mut hi) = (azimuth - 0.3, azimuth + 0.3);
    for _ in 0..55 {
        let a = lo + (hi - lo) * 0.38196601125;
        let b = lo + (hi - lo) * 0.61803398875;
        if distance_to_analytic_arc(a, point) < distance_to_analytic_arc(b, point) {
            hi = b;
        } else {
            lo = a;
        }
    }
    distance_to_analytic_arc((lo + hi) * 0.5, point)
}

#[test]
fn fitted_contact_edges_coincide_with_their_surface_boundaries() {
    let sections: Vec<Vec<Section>> = (0..4)
        .map(|k| {
            (0..SAMPLES)
                .map(|i| {
                    let theta = std::f64::consts::FRAC_PI_2 * (k as f64 + i as f64 / (SAMPLES - 1) as f64);
                    let (c, q1, q2) = analytic_section(theta, 1.0, Vec3::ZERO);
                    Section { c, q1, q2 }
                })
                .collect()
        })
        .collect();
    for k in 0..sections.len() {
        let fit = crate::guard("contact consistency", || fit_sections(&sections, k, true, true)).unwrap();
        for (side, curve) in fit.contacts.iter().enumerate() {
            let (lo, hi) = curve.range_tuple();
            for i in 0..=200 {
                let parameter = lo + (hi - lo) * i as f64 / 200.0;
                let edge = from_p3(curve.subs(parameter));
                let surface = from_p3(fit.surface.subs(side as f64, parameter));
                assert!(edge.dist(surface) < 1e-9, "contact-to-surface gap {}", edge.dist(surface));
            }
        }
    }
}

#[test]
fn rolling_ball_matches_analytic_tee_and_contacts() {
    for (scale, origin) in [(1.0, Vec3::ZERO), (0.25, Vec3::new(23.0, -17.0, 9.0))] {
        let body = tee(scale, origin);
        let point = origin + Vec3::new(0.0, 10.0, 125f64.sqrt()) * scale;
        let blend = fillet(&body, &[point], 3.0 * scale).unwrap();
        assert!(blend.validity().is_empty(), "{:?}", blend.validity());
        let surfaces: Vec<_> = blend.solid.face_iter().map(|f| f.oriented_surface()).filter(|s| matches!(s, mt::Surface::NurbsSurface(_))).collect();
        assert!(!surfaces.is_empty());
        let mut distance: f64 = 0.0;
        let mut tangent_angle: f64 = 0.0;
        for i in 0..96 {
            // Offset the angular samples from patch seams; contact endpoints are tested below.
            let theta = std::f64::consts::TAU * (i as f64 + 0.37) / 96.0;
            let (centre, main, branch) = analytic_section(theta, scale, origin);
            let a = (main - centre).normalized().unwrap();
            let b = (branch - centre).normalized().unwrap();
            let angle = a.dot(b).clamp(-1.0, 1.0).acos();
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                // Spherical linear interpolation of the radial unit vectors, independent of
                // the production rational control-point construction and fitting parameters.
                let radial = (a * ((1.0 - fraction) * angle).sin() + b * (fraction * angle).sin()) / angle.sin();
                let exact = centre + radial * (3.0 * scale);
                let (error, normal) = nearest_blend(&surfaces, exact);
                distance = distance.max(error);
                if fraction == 0.0 || fraction == 1.0 {
                    let want = (centre - exact).normalized().unwrap();
                    tangent_angle = tangent_angle.max(normal.dot(want).abs().min(1.0).acos().to_degrees());
                }
            }
        }
        assert!(distance <= 0.005, "scale {scale}: analytic surface deviation {distance} mm");
        assert!(tangent_angle <= 0.5, "scale {scale}: contact normals differ by {tangent_angle} degrees");

        // The reverse direction uses points on the actual trimmed face mesh, so an
        // untrimmed neighbouring surface cannot conceal an overshooting blend patch.
        let mesh = blend.tessellate_with(0.002, true).unwrap();
        let mut reverse_distance: f64 = 0.0;
        let mut probes = 0;
        for point in mesh.positions.iter().step_by(19) {
            let p = (*point - origin) / scale;
            if (p.x * p.x + p.y * p.y).sqrt() <= 10.000001
                || (p.y * p.y + p.z * p.z).sqrt() <= 15.000001
                || !(10.0..20.0).contains(&p.z)
                || p.x.abs() > 16.0
            {
                continue;
            }
            probes += 1;
            reverse_distance = reverse_distance.max(distance_to_analytic_band(p) * scale);
        }
        assert!(probes > 300, "only {probes} trimmed surface points checked");
        assert!(reverse_distance <= 0.005, "scale {scale}: reverse analytic deviation {reverse_distance} mm");
        eprintln!("scale {scale}: surface {distance:.9} mm, reverse {reverse_distance:.9} mm, contact normals {tangent_angle:.6} degrees");

        let mut contact_count = 0;
        for edge in Body::unique_edges(&blend.solid) {
            let curve = edge.oriented_curve();
            let (lo, hi) = curve.range_tuple();
            let endpoints = [from_p3(curve.subs(lo)), from_p3(curve.subs(hi))].map(|p| (p - origin) / scale);
            if endpoints.iter().any(|p| !(9.0..20.0).contains(&p.z) || p.x.abs() > 16.0) {
                continue;
            }
            let branch = endpoints.iter().all(|p| ((p.x * p.x + p.y * p.y).sqrt() - 10.0).abs() < 1e-5);
            let main = endpoints.iter().all(|p| ((p.y * p.y + p.z * p.z).sqrt() - 15.0).abs() < 1e-5);
            if !branch && !main {
                continue;
            }
            contact_count += 1;
            for i in 0..=200 {
                let p = (from_p3(curve.subs(lo + (hi - lo) * i as f64 / 200.0)) - origin) / scale;
                let radius_error = if branch { ((p.x * p.x + p.y * p.y).sqrt() - 10.0).abs() } else { ((p.y * p.y + p.z * p.z).sqrt() - 15.0).abs() };
                assert!(radius_error * scale <= 0.002, "supporting cylinder deviation {} mm", radius_error * scale);
            }
        }
        assert!(contact_count >= 4, "only {contact_count} contacts checked");
    }
}

#[test]
fn rolling_ball_refinement_cap_returns_an_error_and_preserves_input() {
    let body = tee(1.0, Vec3::ZERO);
    let solid = body.deep_copy();
    let ids: Vec<_> = Body::unique_edges(&solid)
        .iter()
        .filter(|edge| {
            let curve = edge.oriented_curve();
            let (lo, hi) = curve.range_tuple();
            let p = from_p3(curve.subs((lo + hi) * 0.5));
            p.z > 10.0 && ((p.x * p.x + p.y * p.y).sqrt() - 10.0).abs() < 0.01 && ((p.y * p.y + p.z * p.z).sqrt() - 15.0).abs() < 0.01
        })
        .map(mt::Edge::id)
        .collect();
    assert!(!ids.is_empty());
    let before = crate::step_export(&[&body], "unchanged").unwrap();
    let input_before = crate::step_export(&[&Body::new(solid.clone()).unwrap()], "unchanged").unwrap();
    // Four sections deliberately undersample the saddle; production starts at 24.
    let error = crate::guard("limited rolling ball", || curve_blend_with_limit(&solid, &ids, body.size(), 3.0, true, 4)).unwrap_err();
    assert!(error.to_string().contains("accuracy and section limits"), "{error}");
    assert_eq!(crate::step_export(&[&body], "unchanged").unwrap(), before);
    assert_eq!(crate::step_export(&[&Body::new(solid.clone()).unwrap()], "unchanged").unwrap(), input_before);
}
