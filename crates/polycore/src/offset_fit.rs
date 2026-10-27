//! Offsets of free-form surfaces (B-spline, NURBS, revolved curves, …) as B-spline surfaces
//! fitted within an explicit tolerance. The true offset `S(u, v) + d·N(u, v)` is sampled on a
//! grid over the face's parameters and interpolated at the same parameters, so the fitted
//! surface keeps the base surface's parametrisation; the grid is refined where the fit strays
//! (where the surface bends most) until every check point between the samples is within the
//! tolerance. An offset that reaches a centre of curvature (the distance as deep as the
//! smallest radius on that side) or whose fitted normals turn round folds over itself and is
//! refused, never fitted.
//!
//! A base surface can be read a little past its own parameter range (`Extended`): continued
//! along its tangents at the boundary, so the offset of a face can reach a neighbour that its
//! moved edge has passed (the side of a loft moved in under its cap).

use mt::ParametricSurface;
use solidcraft_geom::Vec3;
use truck_modeling as mt;

use crate::body::from_p3;
use crate::{KernelError, Result};

/// Sample lines each way a fit starts with, and the most it is refined to.
const START_LINES: usize = 9;
const MAX_LINES: usize = 513;
/// Most samples along a fitted edge.
const MAX_EDGE_SAMPLES: usize = 1024;

fn vec(x: mt::Vector3) -> Vec3 {
    Vec3::new(x.x, x.y, x.z)
}

/// A surface read on and a little past its own parameter range.
#[derive(Clone)]
pub(crate) struct Extended {
    pub(crate) surface: mt::Surface,
    /// The surface's own range each way, `None` where it is unbounded or periodic.
    range: [Option<(f64, f64)>; 2],
}

/// A point of a surface with its first derivatives.
#[derive(Clone, Copy)]
pub(crate) struct Jet {
    pub(crate) p: Vec3,
    pub(crate) su: Vec3,
    pub(crate) sv: Vec3,
}

impl Jet {
    /// The unit normal `su × sv`, `None` where the surface has none (a pole, a crease).
    pub(crate) fn normal(&self) -> Option<Vec3> {
        // Against the product of the two lengths, so a parameter scaled however it is (an
        // imported surface's knots) doesn't make a regular point look singular.
        let n = self.su.cross(self.sv);
        (n.len() > 1e-12 * (self.su.len() * self.sv.len()).max(1e-300)).then(|| n.normalized()).flatten()
    }
}

impl Extended {
    pub(crate) fn new(surface: mt::Surface) -> Extended {
        use std::ops::Bound::Included;
        let (ru, rv) = surface.parameter_range();
        let bounded = |r: (std::ops::Bound<f64>, std::ops::Bound<f64>), periodic: bool| match r {
            (Included(a), Included(b)) if !periodic && a.is_finite() && b.is_finite() && b > a => Some((a, b)),
            _ => None,
        };
        let range = [bounded(ru, surface.u_period().is_some()), bounded(rv, surface.v_period().is_some())];
        Extended { surface, range }
    }

    /// The parameters clamped into the surface's range, and how far past it they were.
    fn clamp(&self, u: f64, v: f64) -> ((f64, f64), (f64, f64)) {
        let c = |x: f64, r: Option<(f64, f64)>| match r {
            Some((a, b)) => {
                let y = x.clamp(a, b);
                (y, x - y)
            }
            None => (x, 0.0),
        };
        let ((cu, du), (cv, dv)) = (c(u, self.range[0]), c(v, self.range[1]));
        ((cu, cv), (du, dv))
    }

    /// The surface's point and derivatives at (u, v), continued past its range along its
    /// tangents (to first order across each side it is read past, so the continuation adds no
    /// bend of its own: it is exact for a ruled direction such as a loft's between sections).
    pub(crate) fn jet(&self, u: f64, v: f64) -> Jet {
        let s = &self.surface;
        let ((cu, cv), (du, dv)) = self.clamp(u, v);
        let (p, su, sv) = (from_p3(s.subs(cu, cv)), vec(s.uder(cu, cv)), vec(s.vder(cu, cv)));
        if du == 0.0 && dv == 0.0 {
            return Jet { p, su, sv };
        }
        let suv = vec(s.uvder(cu, cv));
        Jet { p: p + su * du + sv * dv + suv * (du * dv), su: su + suv * dv, sv: sv + suv * du }
    }

    /// The second derivatives at (u, v); past the range, those of the continuation (no bend
    /// along a direction it is read past in).
    pub(crate) fn second(&self, u: f64, v: f64) -> (Vec3, Vec3, Vec3) {
        let s = &self.surface;
        let ((cu, cv), (du, dv)) = self.clamp(u, v);
        let (suu, suv, svv) = (vec(s.uuder(cu, cv)), vec(s.uvder(cu, cv)), vec(s.vvder(cu, cv)));
        (if du == 0.0 { suu } else { Vec3::ZERO }, suv, if dv == 0.0 { svv } else { Vec3::ZERO })
    }

    /// Where the surface may bend abruptly, each way: its distinct knots (none for a surface
    /// that has none) and the ends of its range.
    fn breaks(&self) -> (Vec<f64>, Vec<f64>) {
        let distinct = |kv: &mt::KnotVec| {
            let mut k: Vec<f64> = kv.iter().copied().collect();
            k.dedup();
            k
        };
        let (mut ku, mut kv) = match &self.surface {
            mt::Surface::BSplineSurface(b) => (distinct(b.uknot_vec()), distinct(b.vknot_vec())),
            mt::Surface::NurbsSurface(n) => (distinct(n.uknot_vec()), distinct(n.vknot_vec())),
            _ => (Vec::new(), Vec::new()),
        };
        ku.extend(self.range[0].iter().flat_map(|(a, b)| [*a, *b]));
        kv.extend(self.range[1].iter().flat_map(|(a, b)| [*a, *b]));
        (ku, kv)
    }

    /// The principal curvatures at (u, v), signed: positive where the surface bends toward its
    /// normal `su × sv`. `None` where it has no normal.
    pub(crate) fn curvatures(&self, u: f64, v: f64) -> Option<(f64, f64)> {
        let j = self.jet(u, v);
        let n = j.normal()?;
        let (suu, suv, svv) = self.second(u, v);
        let (e, f, g) = (j.su.dot(j.su), j.su.dot(j.sv), j.sv.dot(j.sv));
        let (l, m, nn) = (suu.dot(n), suv.dot(n), svv.dot(n));
        let det = e * g - f * f;
        if det.abs() < 1e-18 {
            return None;
        }
        let k = (l * nn - m * m) / det;
        let h = (e * nn - 2.0 * f * m + g * l) / (2.0 * det);
        let disc = (h * h - k).max(0.0).sqrt();
        Some((h + disc, h - disc))
    }

    /// The parameters of the point of the surface nearest `q`, from `start` (Newton on the
    /// squared distance; Gauss–Newton where that is not convex), kept inside `within`.
    pub(crate) fn foot(&self, q: Vec3, start: (f64, f64), within: [f64; 4]) -> Option<(f64, f64)> {
        let [u0, u1, v0, v1] = within;
        let (mut u, mut v) = start;
        for _ in 0..40 {
            let j = self.jet(u, v);
            let r = q - j.p;
            let (suu, suv, svv) = self.second(u, v);
            let g = [r.dot(j.su), r.dot(j.sv)];
            let mut h = [[j.su.dot(j.su) - r.dot(suu), j.su.dot(j.sv) - r.dot(suv)], [0.0, j.sv.dot(j.sv) - r.dot(svv)]];
            h[1][0] = h[0][1];
            let mut det = h[0][0] * h[1][1] - h[0][1] * h[1][0];
            if !(det > 0.0 && h[0][0] > 0.0) {
                h = [[j.su.dot(j.su), j.su.dot(j.sv)], [j.su.dot(j.sv), j.sv.dot(j.sv)]];
                det = h[0][0] * h[1][1] - h[0][1] * h[1][0];
            }
            if !(det.abs() > 1e-300) {
                return None;
            }
            let du = (h[1][1] * g[0] - h[0][1] * g[1]) / det;
            let dv = (h[0][0] * g[1] - h[1][0] * g[0]) / det;
            let (nu, nv) = ((u + du).clamp(u0, u1), (v + dv).clamp(v0, v1));
            // The step measured on the surface.
            let moved = (j.su * (nu - u) + j.sv * (nv - v)).len();
            (u, v) = (nu, nv);
            if !(u.is_finite() && v.is_finite()) {
                return None;
            }
            if moved < 1e-12 * (1.0 + q.len()) {
                break;
            }
        }
        Some((u, v))
    }
}

/// Refused where moving `base` by `d` along its normal `su × sv` over `span` would fold it:
/// that shrinks lengths along a principal direction by 1 − d·k, and at zero the offset reaches
/// the centre of curvature. Checked on a grid with a margin, naming the tightest radius on
/// that side and where it is.
pub(crate) fn fold_check(base: &Extended, span: [f64; 4], d: f64) -> Result<()> {
    let [u0, u1, v0, v1] = span;
    let lerp = |a: f64, b: f64, n: usize| -> Vec<f64> { (0..n).map(|i| a + (b - a) * i as f64 / (n - 1) as f64).collect() };
    let mut tightest: Option<(f64, Vec3)> = None;
    for u in lerp(u0, u1, 2 * START_LINES - 1) {
        for v in lerp(v0, v1, 2 * START_LINES - 1) {
            let Some((k1, k2)) = base.curvatures(u, v) else { continue };
            for k in [k1, k2] {
                if d * k > 0.0 && 1.0 - d * k <= 0.05 {
                    let r = 1.0 / k.abs();
                    if tightest.is_none_or(|t| r < t.0) {
                        tightest = Some((r, base.jet(u, v).p));
                    }
                }
            }
        }
    }
    if let Some((r, p)) = tightest {
        return Err(KernelError::Invalid(format!(
            "the offset is as deep as the face's smallest concave radius on that side ({r:.4} mm, near [{:.2}, {:.2}, {:.2}]): it would fold the face over",
            p.x, p.y, p.z
        )));
    }
    Ok(())
}

/// The offset of `base` by `d` along its normal `su × sv` over `span` = [u0, u1, v0, v1] (which
/// may reach past the surface's own range), fitted within `tolerance`: a cubic B-spline surface
/// on the same parameters. Refused where the offset would fold over.
pub(crate) fn fit_offset(base: &Extended, span: [f64; 4], d: f64, tolerance: f64) -> Result<mt::BSplineSurface<mt::Point3>> {
    let [u0, u1, v0, v1] = span;
    if !(span.iter().all(|x| x.is_finite()) && u1 > u0 && v1 > v0 && d.is_finite() && tolerance > 0.0) {
        return Err(KernelError::Failed("offset: the face's span on its surface".into()));
    }
    let at = |u: f64, v: f64| -> Result<(Vec3, Vec3)> {
        let j = base.jet(u, v);
        let n =
            j.normal().ok_or_else(|| KernelError::Invalid("the face has a point with no normal (a pole or a crease): not supported yet".into()))?;
        Ok((j.p + n * d, n))
    };
    // The base surface's own knots and the ends of its range (where it is read on past them)
    // break the fit into pieces, joined with triple knots: across them the base's curvature
    // jumps, and a cubic through samples on both sides would converge slowly there. Within a
    // piece the offset is smooth.
    let pieces = |a: f64, b: f64, cuts: Vec<f64>| -> (Vec<f64>, Vec<f64>) {
        let gap = (b - a) * 1e-3;
        let mut ends: Vec<f64> = cuts.into_iter().filter(|k| *k > a + gap && *k < b - gap).collect();
        ends.sort_by(f64::total_cmp);
        ends.dedup_by(|x, y| (*x - *y).abs() < gap);
        let mut all = vec![a];
        all.extend(ends.iter().copied());
        all.push(b);
        let per = (START_LINES - 1).div_ceil(all.len() - 1).max(3);
        let mut lines = vec![a];
        for w in all.windows(2) {
            let (Some(&x), Some(&y)) = (w.first(), w.get(1)) else { continue };
            lines.extend((1..=per).map(|k| x + (y - x) * k as f64 / per as f64));
        }
        (lines, ends)
    };
    let (ku, kv) = base.breaks();
    let ((mut us, bu), (mut vs, bv)) = (pieces(u0, u1, ku), pieces(v0, v1, kv));
    if us.len() > MAX_LINES || vs.len() > MAX_LINES {
        return Err(KernelError::TooLarge(format!(
            "offset: a surface with too many knots to fit its offset ({} × {} sample lines, at most {MAX_LINES})",
            us.len(),
            vs.len()
        )));
    }
    fold_check(base, span, d)?;
    loop {
        let rows: Vec<Vec<Vec3>> = us.iter().map(|u| vs.iter().map(|v| at(*u, *v).map(|x| x.0)).collect::<Result<_>>()).collect::<Result<_>>()?;
        let surface = interpolate_net_at(&rows, (&us, &bu), (&vs, &bv)).ok_or_else(|| KernelError::Failed("offset: fitting the surface".into()))?;
        // Checked between the samples (cell middles and edge middles), where a fit strays most;
        // a sample interval is split wherever one of its checks is off.
        let (mut bad_u, mut bad_v) = (vec![false; us.len() - 1], vec![false; vs.len() - 1]);
        let (mut worst, mut at_worst) = (0.0f64, Vec3::ZERO);
        for i in 0..us.len() {
            for j in 0..vs.len() {
                let (Some(&u), Some(&v)) = (us.get(i), vs.get(j)) else { continue };
                let (un, vn) = (us.get(i + 1).copied(), vs.get(j + 1).copied());
                let checks = [
                    un.zip(vn).map(|(a, b)| ((u + a) / 2.0, (v + b) / 2.0, true, true)),
                    un.map(|a| ((u + a) / 2.0, v, true, false)),
                    vn.map(|b| (u, (v + b) / 2.0, false, true)),
                ];
                for (cu, cv, in_u, in_v) in checks.into_iter().flatten() {
                    let (want, n) = at(cu, cv)?;
                    let got = from_p3(mt::ParametricSurface::subs(&surface, cu, cv));
                    let err = got.dist(want);
                    // The fitted surface's normal turned round: the offset folds over itself.
                    let fitted = vec(surface.uder(cu, cv)).cross(vec(surface.vder(cu, cv)));
                    if fitted.dot(n) <= 0.0 {
                        return Err(KernelError::Invalid(format!(
                            "the offset folds over itself near [{:.2}, {:.2}, {:.2}] (its normals turn round): try a smaller distance",
                            want.x, want.y, want.z
                        )));
                    }
                    if err > worst {
                        (worst, at_worst) = (err, want);
                    }
                    if err > tolerance {
                        if in_u && let Some(b) = bad_u.get_mut(i) {
                            *b = true;
                        }
                        if in_v && let Some(b) = bad_v.get_mut(j) {
                            *b = true;
                        }
                    }
                }
            }
        }
        if !bad_u.contains(&true) && !bad_v.contains(&true) {
            return Ok(surface);
        }
        let split = |xs: &[f64], bad: &[bool]| -> Vec<f64> {
            let mut out = Vec::with_capacity(xs.len() * 2);
            for (k, w) in xs.windows(2).enumerate() {
                let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
                out.push(a);
                if bad.get(k).copied().unwrap_or(false) {
                    out.push((a + b) / 2.0);
                }
            }
            out.extend(xs.last());
            out
        };
        let (nu, nv) = (split(&us, &bad_u), split(&vs, &bad_v));
        if nu.len() > MAX_LINES || nv.len() > MAX_LINES {
            return Err(KernelError::Invalid(format!(
                "the offset can't be fitted within {tolerance} mm (off by up to {worst:.4} mm near [{:.2}, {:.2}, {:.2}], {} × {} samples): try a larger tolerance",
                at_worst.x,
                at_worst.y,
                at_worst.z,
                us.len(),
                vs.len()
            )));
        }
        (us, vs) = (nu, nv);
    }
}

/// A cubic through `at(t)` for t over [0, 1] (uniform parameters), its samples doubled until
/// every point of `at` between them is within `tolerance` of it (measured to the curve's
/// nearest point, so a curve whose own parameter runs unevenly, such as an intersection
/// curve's, still converges).
pub(crate) fn fit_curve(at: impl Fn(f64) -> Result<Vec3>, tolerance: f64) -> Result<mt::BSplineCurve<mt::Point3>> {
    use mt::ParametricCurve;
    let mut n = 16usize;
    loop {
        let pts: Vec<Vec3> = (0..=n).map(|k| at(k as f64 / n as f64)).collect::<Result<_>>()?;
        let ts: Vec<f64> = (0..=n).map(|k| k as f64 / n as f64).collect();
        let c = interp_cubic(&pts, &ts).ok_or_else(|| KernelError::Failed("offset edge".into()))?;
        let (mut worst, mut at_worst) = (0.0f64, Vec3::ZERO);
        for k in 0..n {
            let t = (k as f64 + 0.5) / n as f64;
            let want = at(t)?;
            // The nearest point of the curve, from the same parameter.
            let mut s = t;
            for _ in 0..8 {
                let (p, d1, d2) = (from_p3(c.subs(s)), vec(c.der(s)), vec(c.der2(s)));
                let r = p - want;
                let den = d1.dot(d1) + r.dot(d2);
                if !(den > 1e-300) {
                    break;
                }
                s = (s - r.dot(d1) / den).clamp(0.0, 1.0);
            }
            let off = from_p3(c.subs(s)).dist(want);
            if off > worst {
                (worst, at_worst) = (off, want);
            }
        }
        if worst <= tolerance {
            return Ok(c);
        }
        if n >= MAX_EDGE_SAMPLES {
            let p = at_worst;
            return Err(KernelError::Invalid(format!(
                "an offset edge can't be fitted within {tolerance} mm (off by up to {worst:.4} mm near [{:.2}, {:.2}, {:.2}])",
                p.x, p.y, p.z
            )));
        }
        n *= 2;
    }
}

/// The cubic B-spline through `pts` at parameters `t` (rising from 0 to 1), clamped, its knots
/// by averaging. The collocation matrix is banded and totally positive, so it is solved by
/// elimination within the band, without pivoting.
fn interp_cubic(pts: &[Vec3], t: &[f64]) -> Option<mt::BSplineCurve<mt::Point3>> {
    const BAND: usize = 6;
    let m = pts.len();
    if m < 4 || t.len() != m || t.windows(2).any(|w| w.first() >= w.get(1)) {
        return None;
    }
    let mut knots = vec![0.0; 4];
    for j in 1..(m - 3) {
        knots.push(t.get(j..j + 3)?.iter().sum::<f64>() / 3.0);
    }
    knots.extend([1.0; 4]);
    let mut a: Vec<Vec<f64>> = t.iter().map(|ti| crate::build::cubic_basis(&knots, m, *ti)).collect();
    if a.iter().enumerate().any(|(i, row)| row.len() != m || row.iter().enumerate().any(|(j, x)| *x != 0.0 && i.abs_diff(j) >= BAND)) {
        return None;
    }
    let mut q: Vec<Vec3> = pts.to_vec();
    for col in 0..m {
        let piv = *a.get(col)?.get(col)?;
        if piv.abs() < 1e-14 {
            return None;
        }
        for r in col + 1..(col + BAND).min(m) {
            let f = *a.get(r)?.get(col)? / piv;
            if f == 0.0 {
                continue;
            }
            for c in col..(col + BAND).min(m) {
                let x = *a.get(col)?.get(c)?;
                *a.get_mut(r)?.get_mut(c)? -= f * x;
            }
            let qc = *q.get(col)?;
            *q.get_mut(r)? -= qc * f;
        }
    }
    let mut x = vec![Vec3::ZERO; m];
    for i in (0..m).rev() {
        let mut acc = *q.get(i)?;
        for c in i + 1..(i + BAND).min(m) {
            acc -= *x.get(c)? * *a.get(i)?.get(c)?;
        }
        *x.get_mut(i)? = acc * (1.0 / *a.get(i)?.get(i)?);
    }
    let cps: Vec<mt::Point3> = x.into_iter().map(crate::body::p3).collect();
    mt::BSplineCurve::try_new(mt::KnotVec::from(knots), cps).ok()
}

/// The cubic through `pts` at parameters `t` (rising from 0 to 1) in pieces that end at the
/// parameters `cuts` (each one of `t`): each piece a clamped cubic through its own points,
/// joined end to end with triple knots (C0 there, through the shared point).
fn interp_pieces(pts: &[Vec3], t: &[f64], cuts: &[f64]) -> Option<mt::BSplineCurve<mt::Point3>> {
    let mut at: Vec<usize> = vec![0];
    at.extend(t.iter().enumerate().filter(|(_, x)| cuts.iter().any(|c| (*c - **x).abs() < 1e-12)).map(|(i, _)| i));
    at.push(t.len().checked_sub(1)?);
    at.dedup();
    let (mut knots, mut cps): (Vec<f64>, Vec<mt::Point3>) = (Vec::new(), Vec::new());
    for w in at.windows(2) {
        let (&i, &j) = (w.first()?, w.get(1)?);
        let (a, b) = (*t.get(i)?, *t.get(j)?);
        let local: Vec<f64> = t.get(i..=j)?.iter().map(|x| (x - a) / (b - a)).collect();
        let c = interp_cubic(pts.get(i..=j)?, &local)?;
        let ks: Vec<f64> = c.knot_vec().iter().map(|k| a + (b - a) * k).collect();
        // The first piece keeps its four start knots; every piece its interior ones, and three
        // at its end (the join, or with one more the curve's end).
        let interior = ks.get(4..ks.len().checked_sub(4)?)?;
        if knots.is_empty() {
            knots.extend([a; 4]);
            cps.extend(c.control_points().iter().copied());
        } else {
            cps.extend(c.control_points().iter().skip(1).copied());
        }
        knots.extend(interior.iter().copied());
        knots.extend([b; 3]);
    }
    let end = *knots.last()?;
    knots.push(end);
    mt::BSplineCurve::try_new(mt::KnotVec::from(knots), cps).ok()
}

/// The cubic B-spline surface through `rows[i][j]` at parameters (`us[i]`, `vs[j]`), on
/// [us₀, usₙ] × [vs₀, vsₙ], in pieces between the cuts each way (see [`interp_pieces`]):
/// cubics across v through each row, then across u through their control points (the
/// parameters of each direction rising).
fn interpolate_net_at(rows: &[Vec<Vec3>], (us, cu): (&[f64], &[f64]), (vs, cv): (&[f64], &[f64])) -> Option<mt::BSplineSurface<mt::Point3>> {
    let unit = |xs: &[f64], cuts: &[f64]| -> Option<(Vec<f64>, Vec<f64>)> {
        let (a, b) = (*xs.first()?, *xs.last()?);
        (b > a).then(|| (xs.iter().map(|x| (x - a) / (b - a)).collect(), cuts.iter().map(|x| (x - a) / (b - a)).collect()))
    };
    let ((tu, ctu), (tv, ctv)) = (unit(us, cu)?, unit(vs, cv)?);
    let across_v: Vec<mt::BSplineCurve<mt::Point3>> = rows.iter().map(|r| interp_pieces(r, &tv, &ctv)).collect::<Option<_>>()?;
    let n = across_v.first()?.control_points().len();
    let mut net: Vec<Vec<mt::Point3>> = vec![Vec::with_capacity(rows.len()); n];
    let mut ku = None;
    for (j, col) in net.iter_mut().enumerate() {
        let pts: Vec<Vec3> = across_v.iter().map(|c| c.control_points().get(j).map(|p| from_p3(*p))).collect::<Option<_>>()?;
        let c = interp_pieces(&pts, &tu, &ctu)?;
        ku = Some(c.knot_vec().clone());
        col.extend(c.control_points().iter().copied());
    }
    // `net[j][i]`: control point i across u of column j; the surface wants rows along u.
    let m = net.first()?.len();
    let grid: Vec<Vec<mt::Point3>> = (0..m).map(|i| net.iter().map(|col| col.get(i).copied()).collect::<Option<_>>()).collect::<Option<_>>()?;
    let stretch = |kv: &mt::KnotVec, a: f64, b: f64| mt::KnotVec::from(kv.iter().map(|k| a + (b - a) * k).collect::<Vec<_>>());
    let ku = stretch(&ku?, *us.first()?, *us.last()?);
    let kv = stretch(across_v.first()?.knot_vec(), *vs.first()?, *vs.last()?);
    mt::BSplineSurface::try_new((ku, kv), grid).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::p3;

    /// A doubly curved B-spline patch (a bump) offset both ways: the fit is within the
    /// tolerance of the true offset at points off the sample grid, and the refinement leaves
    /// more sample lines where the patch bends than across its flat run.
    #[test]
    fn a_bump_is_fitted_within_the_tolerance_off_the_grid() {
        let pts: Vec<Vec<mt::Point3>> = (0..5)
            .map(|i| (0..5).map(|j| p3(Vec3::new(i as f64 * 10.0, j as f64 * 10.0, if i == 2 && j == 2 { 12.0 } else { 0.0 }))).collect())
            .collect();
        let knots = mt::KnotVec::bezier_knot(4);
        let s = mt::Surface::BSplineSurface(mt::BSplineSurface::new((knots.clone(), knots), pts));
        let base = Extended::new(s);
        let tol = 0.005;
        for d in [3.0, -3.0] {
            let f = fit_offset(&base, [0.0, 1.0, 0.0, 1.0], d, tol).unwrap();
            for k in 0..=40 {
                for l in 0..=40 {
                    let (u, v) = (0.0125 + 0.975 * k as f64 / 40.0, 0.0125 + 0.975 * l as f64 / 40.0);
                    let j = base.jet(u, v);
                    let want = j.p + j.normal().unwrap() * d;
                    let got = from_p3(mt::ParametricSurface::subs(&f, u, v));
                    assert!(got.dist(want) <= tol, "{} at ({u}, {v})", got.dist(want));
                }
            }
        }
    }

    /// A cylinder of radius 10 (a revolved line) offset in by 9.6 passes the margin before its
    /// axis: refused, naming the radius; offset out, the fit is a cylinder of radius 13.
    #[test]
    fn an_offset_past_the_radius_is_refused() {
        let line = mt::Curve::Line(mt::Line(p3(Vec3::new(10.0, 0.0, 0.0)), p3(Vec3::new(10.0, 0.0, 20.0))));
        let rc = mt::RevolutedCurve::by_revolution(line, p3(Vec3::ZERO), mt::Vector3::unit_z());
        let base = Extended::new(mt::Surface::RevolutedCurve(mt::Processor::new(rc)));
        let j = base.jet(0.5, 1.0);
        let flat = |p: Vec3| (p.x * p.x + p.y * p.y).sqrt();
        // Which way the normal points decides which offset closes in on the axis.
        let inward = if flat(j.p + j.normal().unwrap()) < 10.0 { 1.0 } else { -1.0 };
        let e = fit_offset(&base, [0.0, 1.0, 0.0, 6.0], 9.6 * inward, 0.01).unwrap_err().to_string();
        assert!(e.contains("10.0000") && e.contains("fold"), "{e}");
        let f = fit_offset(&base, [0.0, 1.0, 0.0, 6.0], -3.0 * inward, 0.01).unwrap();
        for k in 0..=20 {
            let p = from_p3(mt::ParametricSurface::subs(&f, 0.3, 6.0 * k as f64 / 20.0));
            assert!((flat(p) - 13.0).abs() < 0.01, "{p:?}");
        }
    }

    /// Past its range a surface goes on along its tangents: a plane's extension is the plane,
    /// and a foot found there lands on it.
    #[test]
    fn a_surface_is_read_past_its_range() {
        let pts = vec![vec![p3(Vec3::ZERO), p3(Vec3::new(0.0, 10.0, 0.0))], vec![p3(Vec3::new(10.0, 0.0, 0.0)), p3(Vec3::new(10.0, 10.0, 0.0))]];
        let k = mt::KnotVec::bezier_knot(1);
        let base = Extended::new(mt::Surface::BSplineSurface(mt::BSplineSurface::new((k.clone(), k), pts)));
        let j = base.jet(1.5, -0.2);
        assert!(j.p.dist(Vec3::new(15.0, -2.0, 0.0)) < 1e-12, "{:?}", j.p);
        let (u, v) = base.foot(Vec3::new(13.0, 4.0, 3.0), (0.5, 0.5), [-1.0, 2.0, -1.0, 2.0]).unwrap();
        assert!((u - 1.3).abs() < 1e-9 && (v - 0.4).abs() < 1e-9, "{u} {v}");
    }

    /// A plane whose u runs over 1e-4 and v over 1e8 (derivatives 1e5 and 1e-7 long): its
    /// normal is regular whatever the scaling, and its offset fits.
    #[test]
    fn a_normal_does_not_depend_on_parameter_scaling() {
        let pts = vec![vec![p3(Vec3::ZERO), p3(Vec3::new(0.0, 10.0, 0.0))], vec![p3(Vec3::new(10.0, 0.0, 0.0)), p3(Vec3::new(10.0, 10.0, 0.0))]];
        let (ku, kv) = (mt::KnotVec::from(vec![0.0, 0.0, 1e-4, 1e-4]), mt::KnotVec::from(vec![0.0, 0.0, 1e8, 1e8]));
        let base = Extended::new(mt::Surface::BSplineSurface(mt::BSplineSurface::new((ku, kv), pts)));
        let n = base.jet(0.5e-4, 0.5e8).normal().unwrap();
        assert!((n.z.abs() - 1.0).abs() < 1e-12, "{n:?}");
        let f = fit_offset(&base, [0.0, 1e-4, 0.0, 1e8], 2.0, 0.01).unwrap();
        assert!((from_p3(mt::ParametricSurface::subs(&f, 0.3e-4, 0.7e8)).z.abs() - 2.0).abs() < 1e-9);
    }

    /// A surface with more knots than an offset fit takes sample lines (400 spans each way) is
    /// refused before anything is sampled.
    #[test]
    fn a_surface_with_too_many_knots_is_refused_up_front() {
        let n = 400;
        let mut knots = vec![0.0, 0.0];
        knots.extend((1..n).map(|i| i as f64 / n as f64));
        knots.extend([1.0, 1.0]);
        let kv = mt::KnotVec::from(knots);
        let pts: Vec<Vec<mt::Point3>> =
            (0..=n).map(|i| (0..=n).map(|j| p3(Vec3::new(i as f64, j as f64, ((i + j) % 2) as f64 * 0.01))).collect()).collect();
        let base = Extended::new(mt::Surface::BSplineSurface(mt::BSplineSurface::new((kv.clone(), kv), pts)));
        let e = fit_offset(&base, [0.0, 1.0, 0.0, 1.0], 0.1, 0.01).unwrap_err();
        assert!(matches!(e, KernelError::TooLarge(_)), "{e}");
    }
}
