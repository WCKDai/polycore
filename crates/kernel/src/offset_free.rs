//! Topology-preserving offsets of bodies with free-form faces (lofts, sweeps, revolved splines,
//! patches), and the shells made from them. Each face moves along its outward normal by its own
//! distance: planes, cylinders, cones, spheres and tori exactly (as in `offset`), any other face
//! onto a B-spline surface fitted to its true offset within a tolerance (`offset_fit`).
//!
//! Every vertex and edge goes to where its faces' moved surfaces meet. Faces that meet
//! smoothly (G1: their normals agree) are one constraint there, so a point on their shared edge
//! moves along their common normal: the offset of the edge itself, with no ill-conditioned
//! intersection of two nearly coincident surfaces. Faces meeting at an angle meet where their
//! moved surfaces cross (Newton on the moved surfaces' distance functions, a free-form face's
//! measured from the nearest point of its own surface, read past its range where the moved
//! edge has passed it). Lines between planes and arcs between analytic faces stay exact; every
//! other edge is a cubic through moved points, refined until it is within the tolerance.
//!
//! Anything the topology can't keep (a round thinner than the move, faces that cross over, a
//! face that would fold over itself) is refused with a message, never built.

use std::collections::HashMap;

use mt::{BoundedCurve, ParametricCurve, SearchNearestParameter};
use solidcraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::body::{Body, Solid, from_p3, p3};
use crate::offset::{Surf, arc_mid, moved_surface, span_step, surf_of};
use crate::offset_fit::{Extended, fit_curve, fit_offset};
use crate::{KernelError, Result, guard};

fn vec3(v: mt::Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn fail(m: &str) -> KernelError {
    KernelError::Failed(format!("offset: {m}"))
}

/// Normals at a point whose `1 − cos` of the angle between them is below this (about 1.1°)
/// are always faces meeting smoothly there; up to a hundred times this (about 11°) where the
/// move is short enough for the tolerance.
const SMOOTH: f64 = 2e-4;
/// Most faces a body may have for the free-form offset.
const MAX_FACES: usize = 512;
/// Seeds for finding the nearest point of a free-form face, each way.
const SEEDS: usize = 17;

/// A free-form face: its surface (read past its range), which way its outward normal points,
/// and the parameters it spans, widened where its moved edges may reach past it.
struct Free {
    base: Extended,
    sign: f64,
    /// The face's own span on its surface: [u0, u1, v0, v1].
    own: [f64; 4],
    /// The span read and fitted: `own` widened by `reach` across each open side.
    span: [f64; 4],
    seeds: Vec<((f64, f64), Vec3)>,
}

impl Free {
    /// `reach`: how far (mm) a moved edge may run past the face's own edges.
    fn new(f: &mt::Face, reach: f64, size: f64) -> Result<Free> {
        let s = f.surface();
        let (mut u0, mut u1, mut v0, mut v1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for e in f.absolute_boundaries().iter().flat_map(|w| w.edge_iter()) {
            let c = e.curve();
            let (t0, t1) = c.range_tuple();
            for k in 0..=16 {
                if let Some((u, v)) = s.search_nearest_parameter(c.subs(t0 + (t1 - t0) * k as f64 / 16.0), None, 100) {
                    (u0, u1, v0, v1) = (u0.min(u), u1.max(u), v0.min(v), v1.max(v));
                }
            }
        }
        if !(u0.is_finite() && u1 > u0 && v0.is_finite() && v1 > v0) {
            return Err(fail("a free-form face's span on its surface"));
        }
        let base = Extended::new(s);
        // Widen across each side where the face is open there (not closed round onto itself),
        // by `reach` measured along the surface.
        let widen = |along_u: bool| -> f64 {
            let (a, b, c0, c1) = if along_u { (u0, u1, v0, v1) } else { (v0, v1, u0, u1) };
            let mut closed = true;
            let mut slowest = f64::INFINITY;
            for k in 0..=8 {
                let c = c0 + (c1 - c0) * k as f64 / 8.0;
                let (ja, jb) = if along_u { (base.jet(a, c), base.jet(b, c)) } else { (base.jet(c, a), base.jet(c, b)) };
                closed &= ja.p.dist(jb.p) < size * 1e-6;
                let speed = |j: &crate::offset_fit::Jet| if along_u { j.su.len() } else { j.sv.len() };
                slowest = slowest.min(speed(&ja)).min(speed(&jb));
            }
            if closed {
                0.0
            } else if slowest > 1e-12 {
                (reach / slowest).min((b - a) * 0.5)
            } else {
                (b - a) * 0.5
            }
        };
        let (eu, ev) = (widen(true), widen(false));
        let span = [u0 - eu, u1 + eu, v0 - ev, v1 + ev];
        let mut seeds = Vec::with_capacity(SEEDS * SEEDS);
        for i in 0..SEEDS {
            for j in 0..SEEDS {
                let (u, v) =
                    (span[0] + (span[1] - span[0]) * i as f64 / (SEEDS - 1) as f64, span[2] + (span[3] - span[2]) * j as f64 / (SEEDS - 1) as f64);
                seeds.push(((u, v), base.jet(u, v).p));
            }
        }
        Ok(Free { base, sign: if f.orientation() { 1.0 } else { -1.0 }, own: [u0, u1, v0, v1], span, seeds })
    }

    /// The parameters of the point of the face's surface nearest `q`, within `span`.
    fn foot(&self, q: Vec3, span: [f64; 4]) -> Option<(f64, f64)> {
        let inside = |(u, v): (f64, f64)| u >= span[0] && u <= span[1] && v >= span[2] && v <= span[3];
        let start = self.seeds.iter().filter(|(uv, _)| inside(*uv)).min_by(|a, b| a.1.dist(q).total_cmp(&b.1.dist(q)))?.0;
        self.base.foot(q, start, span)
    }

    /// The face's outward normal at the point nearest `p`.
    fn normal(&self, p: Vec3) -> Option<Vec3> {
        let (u, v) = self.foot(p, self.span)?;
        self.base.jet(u, v).normal().map(|n| n * self.sign)
    }

    /// The signed distance of `q` from the face moved by `s` (positive outside), and its gradient.
    fn level(&self, q: Vec3, s: f64) -> Option<(f64, Vec3)> {
        let (u, v) = self.foot(q, self.span)?;
        let j = self.base.jet(u, v);
        let n = j.normal()? * self.sign;
        Some(((q - j.p).dot(n) - s, n))
    }
}

/// How one face moves.
enum Mover {
    Exact { old: Surf, new: Surf },
    Free(Box<Free>),
}

struct Moved {
    movers: Vec<Mover>,
    shifts: Vec<f64>,
    tol: f64,
}

impl Moved {
    fn normal(&self, i: usize, p: Vec3) -> Option<Vec3> {
        match self.movers.get(i)? {
            Mover::Exact { old, .. } => old.normal(p),
            Mover::Free(f) => f.normal(p),
        }
    }

    fn level(&self, i: usize, q: Vec3) -> Option<(f64, Vec3)> {
        match self.movers.get(i)? {
            Mover::Exact { new, .. } => Some((new.dist(q), new.grad(q)?)),
            Mover::Free(f) => f.level(q, *self.shifts.get(i)?),
        }
    }

    /// Where the point `p`, on each of the faces `fs`, goes: along the faces' common normal
    /// where they meet smoothly, else to where their moved surfaces cross.
    fn point(&self, p: Vec3, fs: &[usize]) -> Option<Vec3> {
        // (normal, shift, representative face) per group of faces meeting smoothly at p.
        let groups = self.groups(p, fs)?;
        let (ns, ss): (Vec<Vec3>, Vec<f64>) = groups.iter().map(|g| (g.0, g.1)).unzip();
        let mut q = p + span_step(&ns, &ss)?;
        if groups.len() == 1 {
            return Some(q);
        }
        for _ in 0..40 {
            let lv: Vec<(f64, Vec3)> = groups.iter().map(|g| self.level(g.2, q)).collect::<Option<_>>()?;
            if lv.iter().all(|(d, _)| d.abs() < self.tol * 1e-3) {
                return Some(q);
            }
            let (gs, rhs): (Vec<Vec3>, Vec<f64>) = lv.iter().map(|(d, g)| (*g, -*d)).unzip();
            q += span_step(&gs, &rhs)?;
        }
        let lv: Vec<(f64, Vec3)> = groups.iter().map(|g| self.level(g.2, q)).collect::<Option<_>>()?;
        lv.iter().all(|(d, _)| d.abs() < self.tol * 0.1).then_some(q)
    }

    /// The faces `fs` at `p` in groups meeting smoothly there: (normal, shift, a face of the
    /// group). Faces are one group where moving along their mean normal keeps the point within
    /// a quarter of the tolerance of each moved face; `None` where smooth neighbours move by
    /// different distances (they part, with no common point).
    fn groups(&self, p: Vec3, fs: &[usize]) -> Option<Vec<(Vec3, f64, usize)>> {
        let mut groups: Vec<(Vec3, f64, usize)> = Vec::new();
        for &i in fs {
            let (n, s) = (self.normal(i, p)?, *self.shifts.get(i)?);
            // Moved along the mean of normals α apart, a point is off each moved face by about
            // |s|·α²/8 = |s|·(1 − cos α)/4: at most a quarter of the tolerance.
            let smooth = (self.tol / s.abs().max(1e-12)).clamp(SMOOTH, SMOOTH * 100.0);
            match groups.iter().find(|g| g.0.dot(n) > 1.0 - smooth) {
                // Smooth neighbours moving by different distances part: no common point.
                Some(g) if (g.1 - s).abs() > 1e-9 => return None,
                Some(_) => {}
                None => groups.push((n, s, i)),
            }
        }
        Some(groups)
    }

    /// The point at `t` of edge curve `c` (t over [0, 1] for the curve itself, continued along
    /// its end tangents past them), moved with its faces `fs`.
    fn along(&self, c: &mt::Curve, fs: &[usize], t: f64) -> Option<Vec3> {
        let (t0, t1) = c.range_tuple();
        let x = t0 + (t1 - t0) * t;
        let p = if t < 0.0 {
            from_p3(c.subs(t0)) + vec3(c.der(t0)) * (x - t0)
        } else if t > 1.0 {
            from_p3(c.subs(t1)) + vec3(c.der(t1)) * (x - t1)
        } else {
            from_p3(c.subs(x))
        };
        self.point(p, fs)
    }

    /// Where along the moved edge curve (see [`Moved::along`]) it passes nearest `q`, searched
    /// within a quarter of the edge each way of `around`: (t, how far).
    fn nearest_along(&self, c: &mt::Curve, fs: &[usize], q: Vec3, around: f64) -> Option<(f64, f64)> {
        let d = |t: f64| self.along(c, fs, t).map_or(f64::INFINITY, |p| p.dist(q));
        let n = 100;
        let (mut best, mut bd) = (around, d(around));
        for k in 0..=n {
            let t = around - 0.25 + 0.5 * k as f64 / n as f64;
            let x = d(t);
            if x < bd {
                (best, bd) = (t, x);
            }
        }
        // Golden section about the best sample.
        let g = (5f64.sqrt() - 1.0) / 2.0;
        let (mut a, mut b) = (best - 0.5 / n as f64, best + 0.5 / n as f64);
        let (mut x1, mut x2) = (b - g * (b - a), a + g * (b - a));
        let (mut f1, mut f2) = (d(x1), d(x2));
        for _ in 0..60 {
            if f1 < f2 {
                (b, x2, f2) = (x2, x1, f1);
                x1 = b - g * (b - a);
                f1 = d(x1);
            } else {
                (a, x1, f1) = (x1, x2, f2);
                x2 = a + g * (b - a);
                f2 = d(x2);
            }
        }
        let t = (a + b) / 2.0;
        let dt = d(t);
        let found = if dt <= bd { (t, dt) } else { (best, bd) };
        found.1.is_finite().then_some(found)
    }

    /// A corner whose faces fall into two smooth groups fixes only a curve; it goes where that
    /// curve meets a smooth edge's offset: along the offset of edge `c` (its faces `fs`, all in
    /// one group, the corner at its end `end`), to where it meets the other group's moved face
    /// `other`.
    fn corner_on_edge(&self, c: &mt::Curve, fs: &[usize], end: f64, other: usize) -> Option<Vec3> {
        let f = |t: f64| self.along(c, fs, t).and_then(|p| self.level(other, p)).map(|x| x.0);
        let (mut a, mut b) = (end, end + if end == 0.0 { 0.01 } else { -0.01 });
        let (mut fa, mut fb) = (f(a)?, f(b)?);
        for _ in 0..60 {
            if fb.abs() < self.tol * 1e-3 {
                break;
            }
            if (fb - fa).abs() < 1e-300 {
                return None;
            }
            let next = (b - (fb * (b - a)) / (fb - fa)).clamp(-0.5, 1.5);
            (a, fa) = (b, fb);
            b = next;
            fb = f(b)?;
        }
        (fb.abs() < self.tol * 0.1).then(|| self.along(c, fs, b)).flatten()
    }
}

/// Whether any face of `b` is free-form (not a plane, cylinder, cone, sphere or torus).
pub(crate) fn has_free_faces(b: &Body) -> bool {
    let tol = (b.size() * 1e-7).max(1e-9) * 100.0;
    b.solid.face_iter().any(|f| surf_of(f, tol).is_none())
}

/// The body with each face moved along its outward normal by `shift(face index)`, keeping the
/// topology (faces in the same order); free-form faces onto surfaces fitted within `tolerance`.
/// The result's history is over `[b]`: each face is the face it was moved from, each edge the
/// edge it was moved from.
pub(crate) fn offset_free(b: &Body, shift: impl Fn(usize) -> f64, tolerance: f64) -> Result<Body> {
    let faces: Vec<mt::Face> = b.solid.face_iter().cloned().collect();
    if faces.len() > MAX_FACES {
        return Err(fail(&format!("not supported yet: free-form offsets of bodies with more than {MAX_FACES} faces")));
    }
    let size = b.size();
    let rec_tol = (size * 1e-7).max(1e-9) * 100.0;
    let shifts: Vec<f64> = (0..faces.len()).map(&shift).collect();
    if shifts.iter().any(|s| !s.is_finite()) {
        return Err(KernelError::Invalid("the offset distance must be finite".into()));
    }
    let reach = shifts.iter().fold(0.0f64, |m, s| m.max(s.abs())) * 4.0 + size * 1e-3;
    let mut movers = Vec::with_capacity(faces.len());
    for (f, s) in faces.iter().zip(&shifts) {
        movers.push(match surf_of(f, rec_tol) {
            Some(old) => {
                let new = old.moved(*s).ok_or_else(|| fail("not supported yet: a round thinner than the move (it would vanish)"))?;
                Mover::Exact { old, new }
            }
            None => Mover::Free(Box::new(Free::new(f, reach, size)?)),
        });
    }
    // A free-form face moved deeper than its tightest radius on that side folds over: refused
    // before anything is moved, naming the radius.
    for (mv, s) in movers.iter().zip(&shifts) {
        if let Mover::Free(fr) = mv
            && *s != 0.0
        {
            crate::offset_fit::fold_check(&fr.base, fr.own, fr.sign * s)?;
        }
    }
    let m = Moved { movers, shifts, tol: tolerance };
    let mut vfaces: HashMap<mt::VertexID, Vec<usize>> = HashMap::new();
    let mut efaces: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        for v in f.vertex_iter() {
            let e = vfaces.entry(v.id()).or_default();
            if !e.contains(&i) {
                e.push(i);
            }
        }
        for ed in f.edge_iter() {
            let e = efaces.entry(ed.id()).or_default();
            if !e.contains(&i) {
                e.push(i);
            }
        }
    }
    // The edges at each corner.
    let mut vedges: HashMap<mt::VertexID, Vec<mt::Edge>> = HashMap::new();
    for e in b.solid.edge_iter() {
        let (f0, f1) = (e.absolute_front().id(), e.absolute_back().id());
        if f0 == f1 {
            continue;
        }
        for v in [f0, f1] {
            let list = vedges.entry(v).or_default();
            if !list.iter().any(|x| x.id() == e.id()) {
                list.push(e.clone());
            }
        }
    }
    let mut newpos: HashMap<mt::VertexID, Vec3> = HashMap::new();
    for v in b.solid.vertex_iter() {
        let fs = vfaces.get(&v.id()).ok_or_else(|| fail("vertex"))?;
        let p = from_p3(v.point());
        let corner = || fail(&format!("not supported yet: the faces at the corner [{:.2}, {:.2}, {:.2}] don't meet after the move", p.x, p.y, p.z));
        let groups = m.groups(p, fs).ok_or_else(corner)?;
        // Two groups fix a curve, not a point: the corner is where a smooth edge's offset
        // meets it, so that edge's moved curve ends there.
        let on_edge = (groups.len() == 2).then(|| {
            vedges.get(&v.id()).into_iter().flatten().find_map(|e| {
                let efs = efaces.get(&e.id())?;
                let gs = m.groups(p, efs)?;
                let [(gn, ..)] = gs.as_slice() else { return None };
                let other = groups.iter().find(|g| g.0.dot(*gn) < 0.999)?.2;
                let end = if e.absolute_front().id() == v.id() { 0.0 } else { 1.0 };
                m.corner_on_edge(&e.curve(), efs, end, other)
            })
        });
        let q = match on_edge.flatten() {
            Some(q) => q,
            None => m.point(p, fs).ok_or_else(corner)?,
        };
        newpos.insert(v.id(), q);
    }
    // An edge turned round: faces crossed over (a wall thinner than the move).
    for e in b.solid.edge_iter() {
        let (f0, f1) = (e.absolute_front(), e.absolute_back());
        let (Some(q0), Some(q1)) = (newpos.get(&f0.id()), newpos.get(&f1.id())) else { continue };
        let old_v = from_p3(f1.point()) - from_p3(f0.point());
        if old_v.len() > size * 1e-6 && old_v.dot(*q1 - *q0) < 0.0 {
            return Err(fail("not supported yet: the move is larger than a wall or step is thick (faces would cross)"));
        }
    }
    let planes = |fs: &[usize]| fs.iter().all(|i| matches!(m.movers.get(*i), Some(Mover::Exact { old: Surf::Plane { .. }, .. })));
    let exact = |fs: &[usize]| fs.iter().all(|i| matches!(m.movers.get(*i), Some(Mover::Exact { .. })));
    guard("offset", || {
        let verts: HashMap<mt::VertexID, mt::Vertex> = newpos.iter().map(|(id, p)| (*id, builder::vertex(p3(*p)))).collect();
        let mut edges: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
        let mut rec = crate::record::Rebuild::default();
        for e in b.solid.edge_iter() {
            if edges.contains_key(&e.id()) {
                continue;
            }
            let (Some(a), Some(c)) = (verts.get(&e.absolute_front().id()), verts.get(&e.absolute_back().id())) else { return Err(fail("edge")) };
            let fs = efaces.get(&e.id()).ok_or_else(|| fail("edge faces"))?;
            let (pa, pc) = (from_p3(a.point()), from_p3(c.point()));
            let closed = e.absolute_front().id() == e.absolute_back().id();
            let kind = if exact(fs) && !closed { arc_mid(&e, size * 1e-6) } else { None };
            let ne = match kind {
                Some(None) if planes(fs) => builder::line(a, c),
                Some(Some(mid)) => {
                    let m2 = m.point(mid, fs).ok_or_else(|| fail("an edge whose faces don't meet the same way after the move"))?;
                    builder::circle_arc(a, c, p3(m2))
                }
                _ => {
                    // The moved curve between the moved ends, which may have slid along it
                    // (a wall's edge moved in past the floor moved up): where it passes them.
                    let cv = e.curve();
                    let bad = || fail("an edge whose faces don't meet the same way after the move");
                    let (ta, tc) = if closed {
                        (0.0, 1.0)
                    } else {
                        let (Some((ta, da)), Some((tc, dc))) = (m.nearest_along(&cv, fs, pa, 0.0), m.nearest_along(&cv, fs, pc, 1.0)) else {
                            return Err(bad());
                        };
                        if da > tolerance || dc > tolerance || tc <= ta {
                            return Err(bad());
                        }
                        (ta, tc)
                    };
                    let at = |t: f64| -> Result<Vec3> {
                        if t <= 0.0 {
                            return Ok(pa);
                        }
                        if t >= 1.0 {
                            return Ok(pc);
                        }
                        m.along(&cv, fs, ta + (tc - ta) * t).ok_or_else(bad)
                    };
                    mt::Edge::new(a, c, mt::Curve::BSplineCurve(fit_curve(at, tolerance)?))
                }
            };
            rec.piece(&ne, &e);
            edges.insert(e.id(), ne);
        }
        let mut out = Vec::with_capacity(faces.len());
        for ((f, mv), s) in faces.iter().zip(&m.movers).zip(&m.shifts) {
            // The moved surface; a fitted one over the span its moved edges reach, within the
            // tolerance (refused where it folds over).
            let surface = match mv {
                Mover::Exact { old, new } => moved_surface(f, old, new)?,
                Mover::Free(_) if *s == 0.0 => f.surface(),
                Mover::Free(fr) => mt::Surface::BSplineSurface(fit_offset(&fr.base, fit_span(fr, f, &edges)?, fr.sign * s, tolerance)?),
            };
            let wires: Vec<mt::Wire> = f
                .absolute_boundaries()
                .iter()
                .map(|w| {
                    w.edge_iter()
                        .map(|e| {
                            let ne = edges.get(&e.id()).ok_or_else(|| fail("edge"))?;
                            Ok(if e.front() == e.absolute_front() { ne.clone() } else { ne.inverse() })
                        })
                        .collect::<Result<Vec<_>>>()
                        .map(mt::Wire::from)
                })
                .collect::<Result<_>>()?;
            let mut nf = mt::Face::try_new(wires, surface).map_err(|e| fail(&e.to_string()))?;
            if !f.orientation() {
                nf.invert();
            }
            rec.face(&nf, f);
            out.push(nf);
        }
        let mut shells: Vec<mt::Shell> = Vec::new();
        let mut it = out.into_iter();
        for sh in b.solid.boundaries() {
            shells.push(it.by_ref().take(sh.len()).collect::<Vec<_>>().into());
        }
        let solid = Solid::try_new(shells).map_err(|e| fail(&e.to_string()))?;
        crate::record::rebuilt(solid, &[&b.solid], &rec)
    })
}

/// The span of a free-form face's surface that its moved edges (`edges`, by the old edge) reach:
/// their feet on it, padded by 2 % of the face's own span, within the span it is read on.
fn fit_span(fr: &Free, f: &mt::Face, edges: &HashMap<mt::EdgeID, mt::Edge>) -> Result<[f64; 4]> {
    let (mut lo, mut hi) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
    for e in f.edge_iter() {
        let ne = edges.get(&e.id()).ok_or_else(|| fail("edge"))?;
        let c = ne.curve();
        let (t0, t1) = c.range_tuple();
        for k in 0..=16 {
            let q = from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 16.0));
            let (u, v) = fr.foot(q, fr.span).ok_or_else(|| fail("a moved edge off its face's surface"))?;
            lo = (lo.0.min(u), lo.1.min(v));
            hi = (hi.0.max(u), hi.1.max(v));
        }
    }
    let [u0, u1, v0, v1] = fr.own;
    let (pu, pv) = ((u1 - u0) * 0.02, (v1 - v0) * 0.02);
    let [s0, s1, s2, s3] = fr.span;
    let span = [(lo.0 - pu).max(s0), (hi.0 + pu).min(s1), (lo.1 - pv).max(s2), (hi.1 + pv).min(s3)];
    if !(span[1] > span[0] && span[3] > span[2]) {
        return Err(fail("a moved face with no span on its surface"));
    }
    Ok(span)
}

/// How far a shell's free-form inner walls may stray from the true offset of the outer ones:
/// 0.01 mm, or 1e-3 of the body's size for a body under 10 mm.
pub(crate) fn shell_tolerance(b: &Body) -> f64 {
    (b.size() * 1e-3).clamp(1e-4, 0.01)
}

/// Shell of a body with free-form faces: every face but the open ones moved in by
/// `thickness` (the open ones stay, so the cavity's copies of them lie on them), sewn to the
/// body (see `polyhedron::sew_cavity`). `None` for a body without free-form faces, which the
/// analytic shell takes. Refused where an inner wall comes closer than the wall to the outer
/// free-form faces (a part thinner than twice the wall).
pub(crate) fn shell_free(b: &Body, opened: &[usize], thickness: f64) -> Result<Option<Body>> {
    if b.solid.boundaries().len() != 1 || !has_free_faces(b) {
        return Ok(None);
    }
    let tolerance = shell_tolerance(b);
    let cavity = offset_free(b, |i| if opened.contains(&i) { 0.0 } else { -thickness }, tolerance)?;
    if let Some(p) = too_thin(b, &cavity, opened, thickness, tolerance) {
        return Err(KernelError::Failed(format!(
            "not supported yet: the shell's walls would meet near [{:.2}, {:.2}, {:.2}] (the body is thinner there than twice the wall)",
            p.x, p.y, p.z
        )));
    }
    crate::polyhedron::sew_cavity(b, &cavity, opened).map(Some)
}

/// A point of the cavity's walls closer than the wall to one of the body's free-form faces
/// (other than the open ones), sampled over each wall's span.
fn too_thin(b: &Body, cavity: &Body, opened: &[usize], thickness: f64, tolerance: f64) -> Option<Vec3> {
    let size = b.size();
    let outer: Vec<Free> = b
        .solid
        .face_iter()
        .enumerate()
        .filter(|(i, f)| !opened.contains(i) && surf_of(f, (size * 1e-7).max(1e-9) * 100.0).is_none())
        .filter_map(|(_, f)| Free::new(f, 0.0, size).ok())
        .collect();
    let limit = thickness - (tolerance * 10.0).max(thickness * 0.01);
    let rec_tol = (size * 1e-7).max(1e-9) * 100.0;
    let free: Vec<bool> = b.solid.face_iter().map(|f| surf_of(f, rec_tol).is_none()).collect();
    for (i, f) in cavity.solid.face_iter().enumerate() {
        // The inner walls of free-form faces (their spans are the walls; a plane's would reach
        // past its trimmed region).
        if opened.contains(&i) || !free.get(i).copied().unwrap_or(false) {
            continue;
        }
        let s = f.surface();
        let samples: Vec<Vec3> = match Free::new(f, 0.0, size) {
            Ok(fr) => {
                let [u0, u1, v0, v1] = fr.own;
                (0..=6)
                    .flat_map(|k| (0..=6).map(move |l| (u0 + (u1 - u0) * k as f64 / 6.0, v0 + (v1 - v0) * l as f64 / 6.0)))
                    .map(|(u, v)| from_p3(mt::ParametricSurface::subs(&s, u, v)))
                    .collect()
            }
            Err(_) => continue,
        };
        for q in samples {
            for o in &outer {
                let Some((u, v)) = o.foot(q, o.own) else { continue };
                if o.base.jet(u, v).p.dist(q) < limit {
                    return Some(q);
                }
            }
        }
    }
    None
}
