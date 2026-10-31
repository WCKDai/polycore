use truck_modeling as mt;
use truck_stepio::out;

use crate::body::Body;
use crate::{KernelError, Result, guard};

/// Intersection-curve edges (from booleans) as cubic B-splines through the exact curve: STEP
/// readers get plain 3D geometry that lies on both faces, and the file does not repeat the two
/// surfaces for every such edge. Revolutions whose placement no longer preserves that
/// surface type become exact rational sheets on this copy only.
fn plain_curves(b: &Body) -> Result<mt::Solid> {
    let s = crate::ops::placed_topology(&b.solid, mt::Matrix4::from_scale(1.0))?;
    let size = b.size();
    let mut seen = std::collections::HashSet::new();
    for e in s.edge_iter() {
        if !seen.insert(e.id()) {
            continue;
        }
        if let mt::Curve::IntersectionCurve(ic) = e.curve()
            && let Some(c) = hermite_through(&ic, size * 1e-4)
        {
            e.set_curve(mt::Curve::BSplineCurve(c));
        }
    }
    for face in s.face_iter() {
        if let mt::Surface::RevolutedCurve(processor) = face.surface()
            && !out::preserves_revolution(&processor)
        {
            face.set_surface(affine_revolution(&processor)?);
        }
    }
    Ok(s)
}

/// Tensor the exact homogeneous profile with a rational quadratic circle, then apply
/// the affine placement to the homogeneous net. Unlike revolving a transformed profile,
/// this also preserves non-uniform radial scales, shears and float32-noisy rotations.
fn affine_revolution(processor: &mt::Processor<mt::RevolutedCurve<mt::Curve>, mt::Matrix4>) -> Result<mt::Surface> {
    use mt::{EuclideanSpace, InnerSpace, Invertible};
    let revolution = processor.entity();
    let profile = match revolution.entity_curve() {
        mt::Curve::Line(curve) => mt::Curve::BSplineCurve(curve.to_bspline()).lift_up(),
        mt::Curve::BSplineCurve(curve) => mt::Curve::BSplineCurve(curve.clone()).lift_up(),
        mt::Curve::NurbsCurve(curve) => curve.non_rationalized().clone(),
        mt::Curve::IntersectionCurve(_) => {
            return Err(KernelError::Failed("not supported yet: STEP export of an intersection-curve revolution".into()));
        }
    };
    // Conversion expands each profile control point into nine controls, never an
    // input-selected angular sampling count. Bound the allocation before building it.
    if profile.control_points().len() > 100_000 {
        return Err(KernelError::TooLarge("STEP revolution profile exceeds 100000 control points".into()));
    }
    let axis = revolution.axis().normalize();
    let origin = revolution.origin().to_vec();
    let q = std::f64::consts::FRAC_1_SQRT_2;
    let circle = [(1., 0., 1.), (q, q, q), (0., 1., 1.), (-q, q, q), (-1., 0., 1.), (-q, -q, q), (0., -1., 1.), (q, -q, q), (1., 0., 1.)];
    let mut net = Vec::with_capacity(profile.control_points().len());
    for point in profile.control_points() {
        let offset = point.truncate() - origin * point.w;
        let centre = origin * point.w + axis * offset.dot(axis);
        let radial = point.truncate() - centre;
        let tangent = axis.cross(radial);
        let mut row = Vec::with_capacity(circle.len());
        for (cos, sin, weight) in circle {
            let h = processor.transform() * (centre * weight + radial * cos + tangent * sin).extend(point.w * weight);
            if !(h.x.is_finite() && h.y.is_finite() && h.z.is_finite() && h.w.is_finite() && h.w != 0.0) {
                return Err(KernelError::Failed("STEP revolution produced a nonfinite or zero-weight control point".into()));
            }
            row.push(h);
        }
        net.push(row);
    }
    let angular = mt::KnotVec::from(vec![0., 0., 0., 1., 1., 2., 2., 3., 3., 4., 4., 4.]);
    let mut surface = crate::spline_conv::nurbs_surface((profile.knot_vec().clone(), angular), net)?;
    if !processor.orientation() {
        surface.invert();
    }
    Ok(surface)
}

/// A C¹ cubic B-spline through exact points of `c` spaced so that chords deviate at most `tol`
/// (Catmull–Rom tangents over chord length; the cubic is far closer than the chords).
fn hermite_through<C>(c: &C, tol: f64) -> Option<mt::BSplineCurve<mt::Point3>>
where
    C: mt::ParametricCurve<Point = mt::Point3, Vector = mt::Vector3> + mt::BoundedCurve + mt::ParameterDivision1D<Point = mt::Point3>,
{
    use mt::{InnerSpace, MetricSpace};
    let (_, pts) = c.parameter_division(c.range_tuple(), tol);
    let pts: Vec<mt::Point3> = pts.into_iter().fold(Vec::new(), |mut v: Vec<mt::Point3>, p| {
        if v.last().is_none_or(|q| q.distance(p) > tol * 1e-3) {
            v.push(p);
        }
        v
    });
    let n = pts.len().checked_sub(1).filter(|n| *n >= 1 && *n < 1_000_000)?;
    let mut u = vec![0.0; n + 1];
    for i in 1..=n {
        u[i] = u[i - 1] + pts[i].distance(pts[i - 1]);
    }
    let tangent = |i: usize| {
        let (a, b) = (i.saturating_sub(1), (i + 1).min(n));
        (pts[b] - pts[a]) / (u[b] - u[a])
    };
    let mut ctrl = vec![pts[0]];
    let mut knots = vec![u[0]; 4];
    for i in 0..n {
        let h = u[i + 1] - u[i];
        ctrl.push(pts[i] + tangent(i) * (h / 3.0));
        ctrl.push(pts[i + 1] - tangent(i + 1) * (h / 3.0));
        ctrl.push(pts[i + 1]);
        knots.extend(std::iter::repeat_n(u[i + 1], if i + 1 == n { 4 } else { 3 }));
    }
    if ctrl.iter().any(|p| !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite())) || tangent(0).magnitude() == 0.0 {
        return None;
    }
    mt::BSplineCurve::try_new(mt::KnotVec::from(knots), ctrl).ok()
}

/// truck's STEP text for the given bodies (geometry and topology; product data is ours).
pub(crate) fn truck_step(bodies: &[&Body], system: &str) -> Result<String> {
    if bodies.is_empty() {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    for b in bodies {
        b.require_brep("STEP export")?;
    }
    guard("step export", || {
        let cs: Vec<_> = bodies.iter().map(|b| plain_curves(b).map(|solid| solid.compress())).collect::<Result<_>>()?;
        let models: out::StepModels<_, _, _> = cs.iter().collect();
        let header = out::StepHeaderDescriptor { organization_system: system.to_owned(), ..Default::default() };
        written(&out::CompleteStepDisplay::new(models, header))
    })
}

/// The text of truck's STEP writer, or an error. The writer refuses some geometry by returning
/// `fmt::Error` after a line saying why; `to_string` would turn that into a panic (caught by
/// `guard` on the desktop, but the browser build aborts on any panic).
fn written(display: &impl std::fmt::Display) -> Result<String> {
    use std::fmt::Write;
    let mut text = String::new();
    match write!(text, "{display}") {
        Ok(()) => Ok(text),
        Err(_) => {
            let why = text.lines().last().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).unwrap_or("unsupported geometry");
            Err(KernelError::Failed(format!("not supported yet: STEP export of this body ({why})")))
        }
    }
}

/// STEP (ISO 10303-21, AP242) text for the given bodies as one part (`Body1`…, with the
/// bodies' colours).
pub fn step_export(bodies: &[&Body], system: &str) -> Result<String> {
    let list: Vec<crate::ExportBody> =
        bodies.iter().enumerate().map(|(i, b)| crate::ExportBody { name: format!("Body{}", i + 1), body: b, color: b.color() }).collect();
    let header = crate::StepHeader { organization: system.to_string(), ..Default::default() };
    crate::step_out::step_export_bodies(&list, &header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt::{BoundedCurve, InnerSpace, Invertible, MetricSpace, ParametricSurface, ParametricSurface3D, Transformed};

    #[test]
    fn affine_revolution_net_preserves_points_and_normals() {
        let ball = crate::sphere(polycore_geom::Vec3::new(2., -3., 4.), 5.).unwrap();
        let mut checked = 0;
        for face in ball.solid.face_iter() {
            let mt::Surface::RevolutedCurve(source) = face.surface() else { continue };
            checked += 1;
            for sign in [1., -1.] {
                let mut processor = source.clone();
                processor.transform_by(mt::Matrix4::new(sign, 0., 0., 0., 0.2, 1.5, 0., 0., 0., 0.1, 0.6, 0., 10., -20., 30., 1.));
                for inverted in [false, true] {
                    if inverted {
                        processor.invert();
                    }
                    let converted = affine_revolution(&processor).unwrap();
                    let (lo, hi) = processor.entity().entity_curve().range_tuple();
                    for fraction in [0.1, 0.4, 0.8] {
                        let u = lo + fraction * (hi - lo);
                        for quarter in 0..4 {
                            for t in [0.1, 0.5, 0.9] {
                                let a = (1. - t) * (1. - t);
                                let b = std::f64::consts::SQRT_2 * t * (1. - t);
                                let c = t * t;
                                let angle = (b + c).atan2(a + b) + quarter as f64 * std::f64::consts::FRAC_PI_2;
                                let v = quarter as f64 + t;
                                let (old, new, n0, n1) = if processor.orientation() {
                                    (processor.subs(u, angle), converted.subs(u, v), processor.normal(u, angle), converted.normal(u, v))
                                } else {
                                    (processor.subs(angle, u), converted.subs(v, u), processor.normal(angle, u), converted.normal(v, u))
                                };
                                assert!(old.distance(new) < 1e-10, "{old:?} vs {new:?}");
                                assert!(n0.dot(n1) > 1. - 1e-10, "{n0:?} vs {n1:?}");
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 0);
    }

    #[test]
    fn affine_revolution_rejects_nonfinite_controls() {
        let profile = mt::Curve::Line(mt::Line(mt::Point3::new(1., 0., 0.), mt::Point3::new(1., 0., 1.)));
        let mut processor = mt::Processor::new(mt::RevolutedCurve::by_revolution(profile, mt::Point3::new(0., 0., 0.), mt::Vector3::unit_z()));
        processor.transform_by(mt::Matrix4::from_scale(f64::NAN));
        assert!(affine_revolution(&processor).is_err());
    }
}
