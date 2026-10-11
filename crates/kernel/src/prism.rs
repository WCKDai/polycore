//! Booleans of prisms in 2D: when the target is a prism (two caps square to one direction,
//! walls of planes and cylinders along it) and the tool a prism along the same direction
//! spanning the target's height, the result is the prism over the 2D boolean of their
//! sections. Plates with holes, slots and patterns of them: exact, and immune to the mesh
//! intersection's trouble with walls meeting along lines.
//! Stepped extrusions (case floors and walls) use sections between every cap level, so a
//! circular boss can meet the rim without shifting either operand off its exact plane.

use solvecraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling as mt;

use crate::body::{Body, from_p3};
use crate::{BoolOp, KernelError, Result};

const MAX_PLANAR_SEGMENTS: usize = 8192;
const MAX_PLANAR_EVENTS: usize = 131_072;
const MAX_PLANAR_PAIRS: usize = 16_000_000;

/// Boolean of planar regions bounded by straight lines and circular arcs. Boundaries stay
/// analytic; disjoint results and retained islands are returned as separate regions.
pub fn planar_regions_boolean(a: &[Region2], b: &[Region2], op: BoolOp, tolerance: f64) -> Result<Vec<Region2>> {
    crate::guard("planar region boolean", || {
        if !tolerance.is_finite() || !(1e-9..=1.0).contains(&tolerance) {
            return Err(KernelError::Invalid("planar boolean tolerance must be finite and between 1e-9 and 1 mm".into()));
        }
        let loops = || a.iter().chain(b).flat_map(|r| std::iter::once(&r.outer).chain(&r.holes));
        let mut edges = 0usize;
        let mut samples = 0usize;
        for lp in loops() {
            edges = edges.saturating_add(lp.segs.len());
            if edges > MAX_PLANAR_SEGMENTS || lp.segs.is_empty() || !lp.is_closed(tolerance * 10.0) {
                return Err(KernelError::Invalid("planar boolean needs closed loops within its geometry budget".into()));
            }
            for s in &lp.segs {
                let finite = |p: Vec2| p.x.is_finite() && p.y.is_finite() && p.x.abs() < 1e9 && p.y.abs() < 1e9;
                let valid = match *s {
                    Seg2::Line { a, b } => finite(a) && finite(b) && a.dist(b) > 1e-12,
                    Seg2::Arc { center, radius, start, sweep } => {
                        finite(center)
                            && radius.is_finite()
                            && radius > 0.0
                            && radius < 1e9
                            && start.is_finite()
                            && sweep.is_finite()
                            && sweep.abs() > 1e-12
                            && sweep.abs() <= std::f64::consts::TAU + 1e-9
                    }
                    _ => false,
                };
                if !valid {
                    return Err(KernelError::Invalid("planar boolean supports finite nondegenerate lines and circular arcs".into()));
                }
                samples = samples.saturating_add(s.polyline(1e-3).len());
                if samples > 65_536 {
                    return Err(KernelError::Invalid("planar boolean sampled geometry exceeds its budget".into()));
                }
            }
        }
        boolean2d(a, b, op, tolerance)
            .ok_or_else(|| KernelError::Failed("planar region arrangement failed or exceeded its bounded processing budget".into()))
    })
}

/// Extrude normalized planar material as one logical compound, retaining every disjoint
/// region and its holes. Planar arrangement certifies separation before shells are joined.
pub fn extrude_planar_regions(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Body> {
    crate::guard("planar material extrusion", || {
        // Validate aggregate inputs first; an arrangement compares opposite operands,
        // so regions in the same operand must be normalized through balanced unions.
        planar_regions_boolean(regions, &[], BoolOp::Union, 1e-7)?;
        let mut groups: Vec<Vec<Region2>> = regions.iter().cloned().map(|r| vec![r]).collect();
        let mut remaining_pairs = 32_000_000usize;
        while groups.len() > 1 {
            let mut next = Vec::new();
            let mut iter = groups.into_iter();
            while let Some(a) = iter.next() {
                if let Some(b) = iter.next() {
                    let count = |rs: &[Region2]| {
                        rs.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).fold(0usize, |n, l| n.saturating_add(l.segs.len()))
                    };
                    remaining_pairs = remaining_pairs
                        .checked_sub(count(&a).saturating_mul(count(&b)))
                        .ok_or_else(|| KernelError::Invalid("planar extrusion arrangement exceeds its work budget".into()))?;
                    next.push(planar_regions_boolean(&a, &b, BoolOp::Union, 1e-7)?);
                } else {
                    next.push(a);
                }
            }
            groups = next;
        }
        let normalized = groups.pop().unwrap_or_default();
        if normalized.is_empty() {
            return Err(KernelError::Invalid("empty planar material extrusion".into()));
        }
        for (i, a) in normalized.iter().enumerate() {
            for b in normalized.iter().skip(i + 1) {
                for s in std::iter::once(&a.outer).chain(&a.holes).flat_map(|l| &l.segs) {
                    for t in std::iter::once(&b.outer).chain(&b.holes).flat_map(|l| &l.segs) {
                        remaining_pairs = remaining_pairs
                            .checked_sub(1)
                            .ok_or_else(|| KernelError::Invalid("planar extrusion separation exceeds its work budget".into()))?;
                        let endpoints_touch = [s.start(), s.end()].iter().any(|p| t.point_at(t.closest_param(*p)).dist(*p) <= 1e-7)
                            || [t.start(), t.end()].iter().any(|p| s.point_at(s.closest_param(*p)).dist(*p) <= 1e-7);
                        if endpoints_touch || !crossings(s, t, 1e-7).is_empty() {
                            return Err(KernelError::Invalid("planar material pieces touch at a boundary".into()));
                        }
                    }
                }
            }
        }
        let pieces = crate::extrude(plane, &normalized, start, end)?;
        crate::ops::join_pieces(pieces)?.ok_or_else(|| KernelError::Failed("empty planar material extrusion".into()))
    })
}

/// A prism: the plane of its bottom cap (normal along the walls), its height and section.
struct Prism {
    lo: f64,
    hi: f64,
    regions: Vec<Region2>,
}

/// Edges of a cap face as 2D segments (lines and arcs only).
fn seg2(e: &mt::Edge, plane: &Plane, tol: f64) -> Option<Vec<Seg2>> {
    use mt::{BoundedCurve, ParametricCurve};
    let c = e.oriented_curve();
    let (t0, t1) = c.range_tuple();
    let pts: Vec<Vec2> = (0..=16).map(|k| plane.to_local(from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 16.0)))).collect();
    let (a, b) = (*pts.first()?, *pts.last()?);
    let closed = a.dist(b) < tol;
    let seg_dist = |p: Vec2| {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.dot(ab).max(1e-300)).clamp(0.0, 1.0);
        (a + ab * t).dist(p)
    };
    if !closed && pts.iter().all(|p| seg_dist(*p) < tol) {
        return Some(vec![Seg2::Line { a, b }]);
    }
    // Circle through three samples.
    let (p, q, r) = (a, *pts.get(5)?, *pts.get(11)?);
    let d = 2.0 * (p.x * (q.y - r.y) + q.x * (r.y - p.y) + r.x * (p.y - q.y));
    if d.abs() < 1e-300 {
        return None;
    }
    let sq = |v: Vec2| v.x * v.x + v.y * v.y;
    let cx = (sq(p) * (q.y - r.y) + sq(q) * (r.y - p.y) + sq(r) * (p.y - q.y)) / d;
    let cy = (sq(p) * (r.x - q.x) + sq(q) * (p.x - r.x) + sq(r) * (q.x - p.x)) / d;
    let center = Vec2::new(cx, cy);
    let radius = center.dist(a);
    if !pts.iter().all(|x| (x.dist(center) - radius).abs() < tol) {
        return None;
    }
    let ang = |v: Vec2| (v - center).angle();
    let start = ang(a);
    // Signed sweep by unwrapping the samples.
    let mut sweep = 0.0;
    let mut prev = start;
    for x in pts.iter().skip(1) {
        let mut da = ang(*x) - prev;
        while da > std::f64::consts::PI {
            da -= std::f64::consts::TAU;
        }
        while da < -std::f64::consts::PI {
            da += std::f64::consts::TAU;
        }
        sweep += da;
        prev = ang(*x);
    }
    if closed {
        let h = sweep / 2.0;
        return Some(vec![Seg2::Arc { center, radius, start, sweep: h }, Seg2::Arc { center, radius, start: start + h, sweep: h }]);
    }
    Some(vec![Seg2::Arc { center, radius, start, sweep }])
}

/// The body as a prism along `d` (unit), seen in `plane` (normal `d`).
fn prism_of(b: &Body, d: Vec3, plane: &Plane, tol: f64) -> Option<Prism> {
    use mt::{ParametricSurface3D, SearchNearestParameter};
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut tops: Vec<&mt::Face> = Vec::new();
    let mut top_h = Vec::new();
    let mut bottom_h = Vec::new();
    for f in b.solid.face_iter() {
        let surf = f.oriented_surface();
        if let mt::Surface::Plane(p) = &surf {
            let n = p.normal();
            let n = Vec3::new(n.x, n.y, n.z).normalized()?;
            let h = plane.height(from_p3(p.origin()));
            if n.dot(d) > 1.0 - 1e-9 {
                tops.push(f);
                top_h.push(h);
                hi = hi.max(h);
                continue;
            }
            if n.dot(d) < -1.0 + 1e-9 {
                bottom_h.push(h);
                lo = lo.min(h);
                continue;
            }
            if n.dot(d).abs() > 1e-9 {
                return None;
            }
            continue;
        }
        // Walls: normals square to d at sampled boundary points.
        for w in f.boundaries() {
            for v in w.vertex_iter() {
                let (u, s) = surf.search_nearest_parameter(v.point(), None, 50)?;
                let n = surf.normal(u, s);
                if Vec3::new(n.x, n.y, n.z).dot(d).abs() > 1e-7 {
                    return None;
                }
            }
        }
    }
    // One height for every top (and bottom) cap.
    if top_h.iter().any(|h| (h - hi).abs() > tol) || bottom_h.iter().any(|h| (h - lo).abs() > tol) || !(lo.is_finite() && hi > lo + tol) {
        return None;
    }
    let mut regions = Vec::new();
    for f in tops {
        let mut loops = Vec::new();
        for w in f.boundaries() {
            let mut segs = Vec::new();
            for e in w.edge_iter() {
                segs.extend(seg2(e, plane, tol)?);
            }
            loops.push(Loop2 { segs });
        }
        let (outer, holes) = split_outer(loops)?;
        regions.push(Region2 { outer, holes });
    }
    Some(Prism { lo, hi, regions })
}

/// The loop with the largest area is the outer one.
fn split_outer(mut loops: Vec<Loop2>) -> Option<(Loop2, Vec<Loop2>)> {
    let i = (0..loops.len()).max_by(|a, b| loops[*a].signed_area().abs().total_cmp(&loops[*b].signed_area().abs()))?;
    let outer = loops.remove(i);
    Some((outer, loops))
}

fn heights_of(b: &Body, plane: &Plane) -> (f64, f64) {
    b.solid.vertex_iter().map(|v| plane.height(from_p3(v.point()))).fold((f64::INFINITY, f64::NEG_INFINITY), |m, h| (m.0.min(h), m.1.max(h)))
}

/// Points where two segments cross (parameters on each), lines and arcs only.
fn crossings(s: &Seg2, t: &Seg2, tol: f64) -> Vec<(f64, f64)> {
    let mut pts: Vec<Vec2> = Vec::new();
    match (*s, *t) {
        (Seg2::Line { a, b }, Seg2::Line { a: c, b: d }) => {
            let (r, q) = (b - a, d - c);
            let den = r.cross(q);
            if den.abs() > 1e-300 {
                let u = (c - a).cross(q) / den;
                pts.push(a + r * u);
            }
        }
        (Seg2::Line { a, b }, Seg2::Arc { center, radius, .. }) | (Seg2::Arc { center, radius, .. }, Seg2::Line { a, b }) => {
            let dir = b - a;
            let f = a - center;
            let (qa, qb, qc) = (dir.dot(dir), 2.0 * f.dot(dir), f.dot(f) - radius * radius);
            let disc = qb * qb - 4.0 * qa * qc;
            if qa > 1e-300 && disc >= -tol * tol * qa {
                let sd = disc.max(0.0).sqrt();
                for u in [(-qb - sd) / (2.0 * qa), (-qb + sd) / (2.0 * qa)] {
                    pts.push(a + dir * u);
                }
            }
        }
        (Seg2::Arc { center: c0, radius: r0, .. }, Seg2::Arc { center: c1, radius: r1, .. }) => {
            let dd = c0.dist(c1);
            if dd > 1e-12 && dd <= r0 + r1 + tol && dd >= (r0 - r1).abs() - tol {
                let a = (r0 * r0 - r1 * r1 + dd * dd) / (2.0 * dd);
                let h = (r0 * r0 - a * a).max(0.0).sqrt();
                let m = c0 + (c1 - c0) * (a / dd);
                let perp = (c1 - c0).perp() * (1.0 / dd);
                pts.push(m + perp * h);
                pts.push(m - perp * h);
            }
        }
        _ => {}
    }
    let mut out = Vec::new();
    for p in pts {
        let (u, v) = (s.closest_param(p), t.closest_param(p));
        if s.point_at(u).dist(p) < tol && t.point_at(v).dist(p) < tol {
            out.push((u, v));
        }
    }
    out
}

/// Ray parity against analytic lines/arcs. Fragment classification must not depend on a
/// display chord tolerance: a tiny tool can sit between a circular boundary and its chords.
fn inside_loop(lp: &Loop2, p: Vec2) -> bool {
    let mut odd = false;
    for seg in &lp.segs {
        match *seg {
            Seg2::Line { a, b } => {
                if (a.y > p.y) != (b.y > p.y) {
                    let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
                    if x > p.x {
                        odd = !odd;
                    }
                }
            }
            Seg2::Arc { center, radius, start, sweep } => {
                let y = (p.y - center.y) / radius;
                if !(-1.0..=1.0).contains(&y) {
                    continue;
                }
                let a = y.asin();
                let root = (1.0 - y * y).max(0.0).sqrt();
                let full = (sweep.abs() - std::f64::consts::TAU).abs() < 1e-12;
                for (i, (angle, x)) in [(a, center.x + radius * root), (std::f64::consts::PI - a, center.x - radius * root)].into_iter().enumerate() {
                    // A tangent has one geometric root. Interior tangencies contribute zero
                    // parity; a minimum at an endpoint follows the same half-open rule as lines.
                    if root == 0.0 && i == 1 {
                        continue;
                    }
                    if x <= p.x {
                        continue;
                    }
                    let mut delta = if sweep > 0.0 { angle - start } else { start - angle };
                    delta = delta.rem_euclid(std::f64::consts::TAU);
                    if (delta - std::f64::consts::TAU).abs() < 1e-12 {
                        delta = 0.0;
                    }
                    if delta > sweep.abs() + 1e-12 {
                        continue;
                    }
                    let dy = angle.cos() * sweep;
                    let tangent = dy.abs() < sweep.abs() * 1e-12;
                    let at_start = delta < 1e-12;
                    let at_end = (delta - sweep.abs()).abs() < 1e-12;
                    let crosses = if full {
                        !tangent
                    } else if at_start {
                        dy > 0.0 && !tangent || tangent && y < 0.0
                    } else if at_end {
                        dy < 0.0 && !tangent || tangent && y < 0.0
                    } else {
                        !tangent
                    };
                    if crosses {
                        odd = !odd;
                    }
                }
            }
            _ => {
                // Private prism callers only supply lines/arcs. Keep a controlled fallback
                // for future internal callers; the public helper rejects other segment kinds.
                let points = seg.polyline(1e-7);
                for pair in points.windows(2) {
                    if let [a, b] = pair
                        && (a.y > p.y) != (b.y > p.y)
                        && a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) > p.x
                    {
                        odd = !odd;
                    }
                }
            }
        }
    }
    odd
}

fn inside(regions: &[Region2], p: Vec2) -> bool {
    regions.iter().any(|r| inside_loop(&r.outer, p) && !r.holes.iter().any(|h| inside_loop(h, p)))
}

fn dist_to(regions: &[Region2], p: Vec2) -> f64 {
    regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .flat_map(|l| l.segs.iter())
        .map(|s| s.point_at(s.closest_param(p)).dist(p))
        .fold(f64::INFINITY, f64::min)
}

/// Direction of the boundary of `regions` through `p` (on it), as the boundary runs.
fn dir_at(regions: &[Region2], p: Vec2, tol: f64) -> Option<Vec2> {
    for r in regions {
        for l in std::iter::once(&r.outer).chain(&r.holes) {
            for s in &l.segs {
                let t = s.closest_param(p);
                if s.point_at(t).dist(p) < tol {
                    return s.derivative(t).normalized();
                }
            }
        }
    }
    None
}

/// Loops oriented: outers counter-clockwise, holes clockwise.
fn oriented(regions: &[Region2]) -> Vec<Region2> {
    regions
        .iter()
        .map(|r| Region2 {
            outer: if r.outer.signed_area() < 0.0 { r.outer.reversed() } else { r.outer.clone() },
            holes: r.holes.iter().map(|h| if h.signed_area() > 0.0 { h.reversed() } else { h.clone() }).collect(),
        })
        .collect()
}

/// 2D boolean of regions bounded by lines and arcs.
fn boolean2d(a: &[Region2], b: &[Region2], op: BoolOp, tol: f64) -> Option<Vec<Region2>> {
    let count =
        |rs: &[Region2]| rs.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).fold(0usize, |n, lp| n.saturating_add(lp.segs.len()));
    let (na, nb) = (count(a), count(b));
    if na.saturating_add(nb) > MAX_PLANAR_SEGMENTS || na.saturating_mul(nb) > MAX_PLANAR_PAIRS {
        return None;
    }
    let (a, b) = (oriented(a), oriented(b));
    let segs =
        |rs: &[Region2]| -> Vec<Seg2> { rs.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).flat_map(|l| l.segs.clone()).collect() };
    let (sa, sb) = (segs(&a), segs(&b));
    if sa.iter().chain(&sb).any(|s| !matches!(s, Seg2::Line { .. } | Seg2::Arc { .. })) {
        return None;
    }
    // Split every segment where it crosses the other side's boundary.
    let mut cut_a: Vec<Vec<f64>> = vec![Vec::new(); sa.len()];
    let mut cut_b: Vec<Vec<f64>> = vec![Vec::new(); sb.len()];
    let mut events = 0usize;
    for (i, s) in sa.iter().enumerate() {
        for (j, t) in sb.iter().enumerate() {
            for (u, v) in crossings(s, t, tol) {
                events += 2;
                if events > MAX_PLANAR_EVENTS {
                    return None;
                }
                cut_a.get_mut(i)?.push(u);
                cut_b.get_mut(j)?.push(v);
            }
        }
    }
    // Ends of the other side's segments lying on a segment split it too (touching corners).
    for (i, s) in sa.iter().enumerate() {
        for t in &sb {
            for p in [t.start(), t.end()] {
                let u = s.closest_param(p);
                if s.point_at(u).dist(p) < tol {
                    events += 1;
                    if events > MAX_PLANAR_EVENTS {
                        return None;
                    }
                    cut_a.get_mut(i)?.push(u);
                }
            }
        }
    }
    for (j, t) in sb.iter().enumerate() {
        for s in &sa {
            for p in [s.start(), s.end()] {
                let v = t.closest_param(p);
                if t.point_at(v).dist(p) < tol {
                    events += 1;
                    if events > MAX_PLANAR_EVENTS {
                        return None;
                    }
                    cut_b.get_mut(j)?.push(v);
                }
            }
        }
    }
    let pieces = |s: &Seg2, cuts: &mut Vec<f64>| -> Vec<Seg2> {
        cuts.sort_by(f64::total_cmp);
        let mut out = Vec::new();
        let mut rest = *s;
        let mut done = 0.0;
        for &c in cuts.iter() {
            if c <= done + 1e-9 || c >= 1.0 - 1e-9 {
                continue;
            }
            let local = (c - done) / (1.0 - done);
            let (x, y) = rest.split_at(local);
            if x.length() > tol {
                out.push(x);
                rest = y;
                done = c;
            }
        }
        out.push(rest);
        out
    };
    let mut kept: Vec<Seg2> = Vec::new();
    // A's pieces.
    for (i, s) in sa.iter().enumerate() {
        if cut_a.get(i)?.len() > MAX_PLANAR_SEGMENTS {
            return None;
        }
        for p in pieces(s, cut_a.get_mut(i)?) {
            let m = p.point_at(0.5);
            let on = dist_to(&b, m) < tol;
            let keep = if on {
                let same = dir_at(&b, m, tol).is_some_and(|d| d.dot(p.derivative(0.5)) > 0.0);
                match op {
                    BoolOp::Union | BoolOp::Intersect => same,
                    BoolOp::Cut => !same,
                }
            } else {
                let ins = inside(&b, m);
                match op {
                    BoolOp::Union | BoolOp::Cut => !ins,
                    BoolOp::Intersect => ins,
                }
            };
            if keep {
                if kept.len() >= MAX_PLANAR_SEGMENTS {
                    return None;
                }
                kept.push(p);
            }
        }
    }
    // B's pieces (coincident ones were decided with A's).
    for (j, t) in sb.iter().enumerate() {
        if cut_b.get(j)?.len() > MAX_PLANAR_SEGMENTS {
            return None;
        }
        for p in pieces(t, cut_b.get_mut(j)?) {
            let m = p.point_at(0.5);
            if dist_to(&a, m) < tol {
                continue;
            }
            let ins = inside(&a, m);
            if kept.len() >= MAX_PLANAR_SEGMENTS {
                return None;
            }
            match op {
                BoolOp::Union if !ins => kept.push(p),
                BoolOp::Intersect if ins => kept.push(p),
                BoolOp::Cut if ins => kept.push(p.reversed()),
                _ => {}
            }
        }
    }
    // Chain into loops.
    let mut loops: Vec<Loop2> = Vec::new();
    let mut used = vec![false; kept.len()];
    for start in 0..kept.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let mut segs = vec![*kept.get(start)?];
        if let Some(u) = used.get_mut(start) {
            *u = true;
        }
        let first = kept.get(start)?.start();
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > kept.len() + 2 {
                return None;
            }
            let end = segs.last()?.end();
            let full_circle =
                segs.first().is_some_and(|s| matches!(s, Seg2::Arc { sweep, .. } if (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9));
            if end.dist(first) < tol * 10.0 && (segs.len() > 1 || full_circle) {
                break;
            }
            let next = (0..kept.len())
                .filter(|k| !used.get(*k).copied().unwrap_or(true))
                .min_by(|x, y| kept[*x].start().dist(end).total_cmp(&kept[*y].start().dist(end)))?;
            if kept.get(next)?.start().dist(end) > tol * 10.0 {
                if end.dist(first) < tol * 10.0 {
                    break;
                }
                return None;
            }
            if let Some(u) = used.get_mut(next) {
                *u = true;
            }
            segs.push(*kept.get(next)?);
        }
        let l = Loop2 { segs };
        if l.signed_area().abs() > tol * tol {
            loops.push(l);
        }
    }
    // Outers (counter-clockwise) take the holes they contain, the smallest first.
    let (mut outers, holes): (Vec<Loop2>, Vec<Loop2>) = loops.into_iter().partition(|l| l.signed_area() > 0.0);
    outers.sort_by(|x, y| x.signed_area().abs().total_cmp(&y.signed_area().abs()));
    let mut regions: Vec<Region2> = outers.into_iter().map(|o| Region2 { outer: o, holes: Vec::new() }).collect();
    for h in holes {
        let p = h.segs.first()?.point_at(0.5);
        let r = regions.iter_mut().find(|r| inside_loop(&r.outer, p) && r.outer.signed_area().abs() > h.signed_area().abs())?;
        r.holes.push(h);
    }
    Some(regions)
}

/// The boolean done on prism sections, or `None` when it doesn't apply.
/// The pieces of a segment between the given points (those lying on it, inside it).
fn split_seg(seg: &Seg2, pts: &[Vec2], tol: f64) -> Option<Vec<Seg2>> {
    if !matches!(seg, Seg2::Line { .. } | Seg2::Arc { .. }) {
        return None;
    }
    let mut ts: Vec<f64> = pts
        .iter()
        .filter_map(|p| {
            let t = seg.closest_param(*p);
            (t > 1e-9 && t < 1.0 - 1e-9 && seg.point_at(t).dist(*p) < tol).then_some(t)
        })
        .collect();
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
    let mut out = Vec::new();
    let mut t0 = 0.0;
    for t1 in ts.into_iter().chain(std::iter::once(1.0)) {
        out.push(match *seg {
            Seg2::Line { .. } => Seg2::Line { a: seg.point_at(t0), b: seg.point_at(t1) },
            Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center, radius, start: start + sweep * t0, sweep: sweep * (t1 - t0) },
            _ => return None,
        });
        t0 = t1;
    }
    Some(out)
}

/// The union of two prisms along `d` stacked end to end (or overlapping) with different
/// sections, sewn from its faces: each slab between the prisms' end levels has walls over its
/// section's outline; each level has a face up over the section below less the one above, and
/// a face down over the one above less the one below. Every outline is split where any other
/// meets it, so the faces share their edges. `None` when a slab is empty or a curve isn't a
/// line or an arc.
fn sew_stack(plane: &Plane, d: Vec3, pa: &Prism, pb: &Prism, tol: f64) -> Option<Result<Body>> {
    let mut levels = vec![pa.lo, pa.hi, pb.lo, pb.hi];
    levels.sort_by(f64::total_cmp);
    levels.dedup_by(|x, y| (*x - *y).abs() < tol);
    let covers = |p: &Prism, z0: f64, z1: f64| p.lo <= z0 + tol && p.hi >= z1 - tol;
    let mut slabs: Vec<Vec<Region2>> = Vec::new();
    for w in levels.windows(2) {
        let (z0, z1) = (*w.first()?, *w.get(1)?);
        let s = match (covers(pa, z0, z1), covers(pb, z0, z1)) {
            (true, true) => boolean2d(&pa.regions, &pb.regions, BoolOp::Union, tol)?,
            (true, false) => pa.regions.clone(),
            (false, true) => pb.regions.clone(),
            (false, false) => return None,
        };
        slabs.push(oriented(&s));
    }
    sew_sections(plane, d, &levels, &slabs, tol)
}

/// Sew the exact sections between successive levels, including empty gaps.
fn sew_sections(plane: &Plane, d: Vec3, levels: &[f64], slabs: &[Vec<Region2>], tol: f64) -> Option<Result<Body>> {
    use crate::sew::{EdgeSpec, FaceSpec, SurfSpec};
    // Faces at each level: up where the slab below sticks out, down where the one above does.
    let diff = |x: &[Region2], y: &[Region2]| -> Option<Vec<Region2>> {
        match (x.is_empty(), y.is_empty()) {
            (true, _) => Some(Vec::new()),
            (false, true) => Some(x.to_vec()),
            (false, false) => boolean2d(x, y, BoolOp::Cut, tol).map(|r| oriented(&r)),
        }
    };
    let mut caps: Vec<(f64, bool, Vec<Region2>)> = Vec::new();
    for (k, z) in levels.iter().enumerate() {
        let below: &[Region2] = if k == 0 { &[] } else { slabs.get(k - 1)? };
        let above: &[Region2] = slabs.get(k).map(Vec::as_slice).unwrap_or(&[]);
        caps.push((*z, true, diff(below, above)?));
        caps.push((*z, false, diff(above, below)?));
    }
    // Every segment end, everywhere: outlines split there.
    let loops_of =
        |rs: &[Region2]| -> Vec<Loop2> { rs.iter().flat_map(|r| std::iter::once(r.outer.clone()).chain(r.holes.iter().cloned())).collect() };
    let mut pts: Vec<Vec2> = Vec::new();
    for lp in slabs.iter().flat_map(|s| loops_of(s)).chain(caps.iter().flat_map(|c| loops_of(&c.2))) {
        for sg in &lp.segs {
            pts.push(sg.start());
            pts.push(sg.end());
        }
    }
    let at = |u: Vec2, z: f64| plane.to_world(u) + d * z;
    let edge = |sg: &Seg2, z: f64| -> EdgeSpec {
        match sg {
            Seg2::Arc { .. } => EdgeSpec::Arc { a: at(sg.start(), z), b: at(sg.end(), z), mid: at(sg.point_at(0.5), z) },
            _ => EdgeSpec::Line { a: at(sg.start(), z), b: at(sg.end(), z) },
        }
    };
    let mut faces: Vec<FaceSpec> = Vec::new();
    for (i, s) in slabs.iter().enumerate() {
        let (z0, z1) = (*levels.get(i)?, *levels.get(i + 1)?);
        for lp in loops_of(s) {
            for sg0 in &lp.segs {
                for sg in split_seg(sg0, &pts, tol)? {
                    let (a, b) = (sg.start(), sg.end());
                    let lp3 = vec![
                        edge(&sg, z0),
                        EdgeSpec::Line { a: at(b, z0), b: at(b, z1) },
                        edge(&sg.reversed(), z1),
                        EdgeSpec::Line { a: at(a, z1), b: at(a, z0) },
                    ];
                    let tangent = plane.dir_to_world(sg.derivative(0.5));
                    let outward = tangent.cross(d).normalized()?;
                    let surface = match sg {
                        Seg2::Arc { .. } => SurfSpec::Extruded { a: at(a, z0), b: at(b, z0), mid: at(sg.point_at(0.5), z0), dir: d * (z1 - z0) },
                        _ => SurfSpec::Plane,
                    };
                    faces.push(FaceSpec { loops: vec![lp3], surface, probe: Some((at(sg.point_at(0.5), (z0 + z1) / 2.0), outward)) });
                }
            }
        }
    }
    for (z, up, rs) in &caps {
        for r in rs {
            let mut loops = Vec::new();
            for lp in std::iter::once(&r.outer).chain(&r.holes) {
                let mut segs: Vec<Seg2> = Vec::new();
                for sg in &lp.segs {
                    segs.extend(split_seg(sg, &pts, tol)?);
                }
                let segs: Vec<Seg2> = if *up { segs } else { segs.iter().rev().map(Seg2::reversed).collect() };
                loops.push(segs.iter().map(|sg| edge(sg, *z)).collect::<Vec<_>>());
            }
            faces.push(FaceSpec { loops, surface: SurfSpec::Plane, probe: None });
        }
    }
    Some(crate::sew::sew_touching(&faces))
}

fn simple_prism_boolean(a: &Body, b: &Body, op: BoolOp) -> Option<Result<Option<Body>>> {
    let size = a.size().max(b.size());
    let tol = (size * 1e-7).max(1e-9) * 10.0;
    // Candidate directions: a's planar face normals (each line once).
    let mut dirs: Vec<Vec3> = Vec::new();
    for f in a.solid.face_iter() {
        if let mt::Surface::Plane(p) = f.oriented_surface() {
            let n = p.normal();
            if let Some(n) = Vec3::new(n.x, n.y, n.z).normalized()
                && !dirs.iter().any(|m| m.dot(n).abs() > 1.0 - 1e-9)
            {
                dirs.push(if n.dot(Vec3::new(1e-3, 1e-6, 1.0)) < 0.0 { -n } else { n });
            }
        }
    }
    let (d, plane, pa, pb) = dirs.into_iter().take(8).find_map(|d| {
        let plane = Plane::from_normal(Vec3::ZERO, d)?;
        let pa = prism_of(a, d, &plane, tol)?;
        let pb = prism_of(b, d, &plane, tol)?;
        Some((d, plane, pa, pb))
    })?;
    let _ = d;
    let (blo, bhi) = heights_of(b, &plane);
    let spans = blo <= pa.lo + tol && bhi >= pa.hi - tol;
    let same = (pb.lo - pa.lo).abs() < tol && (pb.hi - pa.hi).abs() < tol;
    let area =
        |rs: &[Region2]| rs.iter().map(|r| r.outer.signed_area().abs() - r.holes.iter().map(|h| h.signed_area().abs()).sum::<f64>()).sum::<f64>();
    // Prisms of one section overlapping or touching end to end: one longer prism.
    let stacked = op == BoolOp::Union && !same && pb.lo <= pa.hi + tol && pa.lo <= pb.hi + tol;
    let ok = match op {
        BoolOp::Cut | BoolOp::Intersect => spans,
        BoolOp::Union => same || stacked,
    };
    if !ok {
        return None;
    }
    let regions = boolean2d(&pa.regions, &pb.regions, op, tol)?;
    if regions.is_empty() {
        return Some(Ok(None));
    }
    let (lo, hi) = if stacked {
        // Different sections (a boss hanging over a plate's edge): sewn slab by slab.
        let (u, x, y) = (area(&regions), area(&pa.regions), area(&pb.regions));
        if (u - x).abs() > tol * (1.0 + u) || (u - y).abs() > tol * (1.0 + u) {
            return sew_stack(&plane, d, &pa, &pb, tol).map(|r| r.map(Some));
        }
        (pa.lo.min(pb.lo), pa.hi.max(pb.hi))
    } else {
        (pa.lo, pa.hi)
    };
    let base = plane.offset(lo);
    let mut bodies = match crate::extrude(&base, &regions, 0.0, hi - lo) {
        Ok(b) => b,
        Err(e) => return Some(Err(e)),
    };
    let first = bodies.pop()?;
    if !bodies.is_empty() {
        // Several pieces: one body of several lumps.
        let mut shells = first.solid.boundaries().clone();
        for b in bodies {
            shells.extend(b.solid.boundaries().iter().cloned());
        }
        return Some(crate::body::Solid::try_new(shells).map_err(|e| crate::KernelError::Failed(e.to_string())).and_then(Body::new).map(Some));
    }
    Some(Ok(Some(first)))
}

/// A stepped extrusion (case floor plus walls), recovered from its cap events.
/// Downward caps add a section above their level; upward caps remove one.
fn stack_of(b: &Body, d: Vec3, plane: &Plane, tol: f64) -> Option<Vec<Prism>> {
    use mt::{ParametricSurface3D, SearchNearestParameter};
    if let Some(p) = prism_of(b, d, plane, tol) {
        return Some(vec![p]);
    }
    if b.face_count() > 2048 {
        return None;
    }
    let mut events = Vec::new();
    for f in b.solid.face_iter() {
        let surf = f.oriented_surface();
        if let mt::Surface::Plane(p) = &surf {
            let n = p.normal();
            let n = Vec3::new(n.x, n.y, n.z).normalized()?;
            if n.dot(d).abs() > 1.0 - 1e-9 {
                let mut loops = Vec::new();
                for w in f.boundaries() {
                    let mut segs = Vec::new();
                    for e in w.edge_iter() {
                        segs.extend(seg2(e, plane, tol)?);
                    }
                    loops.push(Loop2 { segs });
                }
                let (outer, holes) = split_outer(loops)?;
                let region = Region2 { outer, holes };
                events.push((plane.height(from_p3(p.origin())), n.dot(d) > 0.0, oriented(&[region])));
                continue;
            }
            if n.dot(d).abs() > 1e-9 {
                return None;
            }
        } else {
            // Curved walls must be extruded along d, not slanted or rounded caps.
            for v in f.vertex_iter() {
                let (u, w) = surf.search_nearest_parameter(v.point(), None, 50)?;
                let n = surf.normal(u, w);
                if Vec3::new(n.x, n.y, n.z).dot(d).abs() > 1e-7 {
                    return None;
                }
            }
        }
    }
    let mut levels: Vec<f64> = events.iter().map(|(z, _, _)| *z).collect();
    levels.sort_by(f64::total_cmp);
    levels.dedup_by(|x, y| (*x - *y).abs() < tol);
    if levels.len() < 2 || levels.len() > 64 || levels.iter().any(|z| !z.is_finite()) {
        return None;
    }
    let mut current = Vec::new();
    let mut stack = Vec::new();
    for (i, z) in levels.iter().enumerate() {
        // Remove terminating material before adding any material starting here.
        for up in [true, false] {
            for (_, _, section) in events.iter().filter(|(h, u, _)| (*h - *z).abs() < tol && *u == up) {
                current = match (current.is_empty(), up) {
                    (true, false) => section.clone(),
                    (true, true) => return None,
                    (false, _) => boolean2d(&current, section, if up { BoolOp::Cut } else { BoolOp::Union }, tol)?,
                };
            }
        }
        if let Some(next) = levels.get(i + 1)
            && !current.is_empty()
        {
            stack.push(Prism { lo: *z, hi: *next, regions: oriented(&current) });
        }
    }
    (current.is_empty() && !stack.is_empty()).then_some(stack)
}

fn stack_boolean(a: &Body, b: &Body, op: BoolOp) -> Option<Result<Option<Body>>> {
    // Common-cap joins and partial-depth cuts of aligned stepped prisms can
    // retain their exact section boundaries instead of intersecting meshes.
    // Other extents retain their existing B-rep path.
    if !matches!(op, BoolOp::Union | BoolOp::Cut) {
        return None;
    }
    if op == BoolOp::Cut && a.face_count().saturating_add(b.face_count()) > 2048 {
        return None;
    }
    fn section(ps: &[Prism], lo: f64, hi: f64, tol: f64) -> &[Region2] {
        ps.iter().find(|p| p.lo <= lo + tol && p.hi >= hi - tol).map(|p| p.regions.as_slice()).unwrap_or(&[])
    }
    let tol = (a.size().max(b.size()) * 1e-7).max(1e-9) * 10.0;
    let mut dirs: Vec<Vec3> = Vec::new();
    for f in a.solid.face_iter() {
        if let mt::Surface::Plane(p) = f.oriented_surface() {
            let n = p.normal();
            if let Some(n) = Vec3::new(n.x, n.y, n.z).normalized()
                && !dirs.iter().any(|m| m.dot(n).abs() > 1.0 - 1e-9)
            {
                dirs.push(n);
            }
        }
    }
    let ends = |ps: &[Prism]| ps.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.lo), hi.max(p.hi)));
    let (d, plane, pa, pb) = dirs.into_iter().take(8).find_map(|d| {
        let plane = Plane::from_normal(Vec3::ZERO, d)?;
        let pa = stack_of(a, d, &plane, tol)?;
        let pb = stack_of(b, d, &plane, tol)?;
        let (ea, eb) = (ends(&pa), ends(&pb));
        if (ea.0 - eb.0).abs() >= tol && (ea.1 - eb.1).abs() >= tol {
            return None;
        }
        Some((d, plane, pa, pb))
    })?;
    let mut levels: Vec<f64> = pa.iter().chain(&pb).flat_map(|p| [p.lo, p.hi]).collect();
    levels.sort_by(f64::total_cmp);
    levels.dedup_by(|x, y| (*x - *y).abs() < tol);
    let mut slabs = Vec::new();
    let mut work = 0usize;
    let mut output_edges = 0usize;
    let edges = |regions: &[Region2]| regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).map(|l| l.segs.len()).sum::<usize>();
    for w in levels.windows(2) {
        let (lo, hi) = (*w.first()?, *w.get(1)?);
        let (x, y) = (section(&pa, lo, hi, tol), section(&pb, lo, hi, tol));
        if op == BoolOp::Cut {
            let count = edges(x).saturating_add(edges(y));
            work = work.saturating_add(count.saturating_mul(count));
            if work > 32_000_000 {
                return None;
            }
        }
        let s = match (x.is_empty(), y.is_empty(), op) {
            (true, _, BoolOp::Cut | BoolOp::Intersect) | (_, true, BoolOp::Intersect) => Vec::new(),
            (true, _, BoolOp::Union) => y.to_vec(),
            (_, true, _) => x.to_vec(),
            _ => boolean2d(x, y, op, tol)?,
        };
        if op == BoolOp::Cut {
            output_edges = output_edges.saturating_add(edges(&s));
            if output_edges > 8192 {
                return None;
            }
        }
        slabs.push(oriented(&s));
    }
    if slabs.iter().all(Vec::is_empty) {
        return Some(Ok(None));
    }
    sew_sections(&plane, d, &levels, &slabs, tol).map(|r| r.map(Some))
}

pub(crate) fn prism_boolean(a: &Body, b: &Body, op: BoolOp) -> Option<Result<Option<Body>>> {
    crate::guard("prism boolean", || Ok(simple_prism_boolean(a, b, op).or_else(|| stack_boolean(a, b, op)))).ok().flatten()
}

#[cfg(test)]
mod planar_tests {
    use super::*;
    fn rectangle(a: Vec2, b: Vec2) -> Region2 {
        Region2 { outer: Loop2::polygon(&[a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)]), holes: vec![] }
    }
    #[test]
    fn partial_depth_cuts_touching_a_through_hole_use_exact_stacked_sections() {
        let plane = Plane::named("XY").unwrap();
        let mut plate = rectangle(Vec2::ZERO, Vec2::new(100., 60.));
        plate.holes.push(rectangle(Vec2::new(25., 20.), Vec2::new(30., 30.)).outer);
        let stock = crate::extrude(&plane, &[plate], 0., 2.5).unwrap().pop().unwrap();
        let low = crate::extrude(&plane, &[rectangle(Vec2::new(20., 20.), Vec2::new(30., 30.))], 0., 1.5).unwrap().pop().unwrap();
        let high = crate::extrude(&plane, &[rectangle(Vec2::new(25., 20.), Vec2::new(35., 30.))], 1., 2.5).unwrap().pop().unwrap();
        for tools in [[&low, &high], [&high, &low]] {
            let mut result = stock.clone();
            for tool in tools {
                result = stack_boolean(&result, tool, BoolOp::Cut).expect("exact stack path").unwrap().unwrap();
            }
            assert!(result.validity().is_empty());
            assert_eq!(result.shell_count(), 1);
            assert!((crate::measure(&result).unwrap().volume - 14_725.).abs() < 1e-6);
            let mesh = result.tessellate(1e-4).unwrap();
            for (point, material) in [
                (Vec3::new(22.5, 25., 0.5), false),
                (Vec3::new(22.5, 25., 2.), true),
                (Vec3::new(32.5, 25., 0.5), true),
                (Vec3::new(32.5, 25., 2.), false),
                (Vec3::new(27.5, 25., 1.25), false),
                (Vec3::new(10., 10., 1.25), true),
            ] {
                assert_eq!(mesh.contains(point), material, "{point:?}");
            }
            let imported = crate::step_import(&crate::step_export(&[&result], "layered cut").unwrap()).unwrap();
            assert!(imported.warnings.is_empty());
            assert!((crate::measure(&imported.bodies.first().unwrap().body).unwrap().volume - 14_725.).abs() < 1e-6);
            let next = crate::extrude(&plane, &[rectangle(Vec2::new(20., 20.), Vec2::new(25., 30.))], 1.5, 2.5).unwrap().pop().unwrap();
            let opened = crate::boolean(&imported.bodies.first().unwrap().body, &next, BoolOp::Cut).unwrap().unwrap();
            assert!((crate::measure(&opened).unwrap().volume - 14_675.).abs() < 1e-6);
            assert!(opened.validity().is_empty());
        }
    }
    #[test]
    fn compound_planar_extrusion_normalizes_overlap_and_retains_islands() {
        let plane = Plane::named("XY").unwrap();
        let a = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        let b = rectangle(Vec2::new(5.0, 0.0), Vec2::new(15.0, 10.0));
        let body = extrude_planar_regions(&plane, &[a.clone(), b], 0.0, 2.0).unwrap();
        assert!((crate::measure(&body).unwrap().volume - 300.0).abs() < 1e-6);
        let inner = Region2 { outer: Loop2::circle(Vec2::new(5.0, 5.0), 2.0), holes: vec![] };
        let body = extrude_planar_regions(&plane, &[a.clone(), inner.clone()], 0.0, 2.0).unwrap();
        assert!((crate::measure(&body).unwrap().volume - 200.0).abs() < 1e-6);
        let annulus = Region2 { outer: a.outer, holes: vec![Loop2::circle(Vec2::new(5.0, 5.0), 3.0)] };
        let body = extrude_planar_regions(&plane, &[annulus, inner], 0.0, 2.0).unwrap();
        let expected = (100.0 - std::f64::consts::PI * 5.0) * 2.0;
        assert!((crate::measure(&body).unwrap().volume - expected).abs() < 0.1);
        assert_eq!(body.solid.boundaries().len(), 2);
        let a = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        let b = rectangle(Vec2::new(10.0, 10.0), Vec2::new(20.0, 20.0));
        assert!(extrude_planar_regions(&plane, &[a, b], 0.0, 2.0).is_err());
        let circles = [Vec2::ZERO, Vec2::new(10.0, 0.0)].map(|center| Region2 { outer: Loop2::circle(center, 5.0), holes: vec![] });
        assert!(extrude_planar_regions(&plane, &circles, 0.0, 2.0).is_err());
    }

    #[test]
    fn analytic_circle_classification_respects_signed_sweeps_and_seams() {
        for seam in [0.0, 0.13, std::f64::consts::FRAC_PI_2, -std::f64::consts::FRAC_PI_2] {
            for reversed in [false, true] {
                let lp = Loop2::circle_from(Vec2::ZERO, 10.0, seam);
                let lp = if reversed { lp.reversed() } else { lp };
                for p in [Vec2::ZERO, Vec2::new(9.9998, 0.0), Vec2::new(0.0, 9.9998), Vec2::new(0.0, -9.9998)] {
                    assert!(inside_loop(&lp, p), "{seam} {reversed} {p:?}");
                }
                for p in [Vec2::new(11.0, 0.0), Vec2::new(0.0, 10.0), Vec2::new(0.0, -10.0)] {
                    assert!(!inside_loop(&lp, p), "{seam} {reversed} {p:?}");
                }
            }
        }
        let half = Loop2 {
            segs: vec![
                Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: -std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::PI },
                Seg2::Line { a: Vec2::new(0.0, 10.0), b: Vec2::new(0.0, -10.0) },
            ],
        };
        for lp in [half.clone(), half.reversed()] {
            assert!(inside_loop(&lp, Vec2::new(5.0, 0.0)));
            assert!(!inside_loop(&lp, Vec2::new(-5.0, 0.0)));
            assert!(!inside_loop(&lp, Vec2::new(-5.0, -10.0)));
        }
    }
    #[test]
    fn tiny_tool_between_circle_and_display_chord_is_not_lost() {
        let circle = Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![] };
        let sampled = circle.outer.polyline(1e-3);
        let direction = (sampled[0] + sampled[1]).normalized().unwrap();
        let at = direction * 9.9998;
        let half = Vec2::new(0.000025, 0.000025);
        let square = rectangle(at - half, at + half);
        assert!(square.outer.segs.iter().all(|s| s.start().len() < 10.0));
        assert!(!circle.contains(at), "fixture must lie outside the display chord");
        let cut = planar_regions_boolean(std::slice::from_ref(&circle), std::slice::from_ref(&square), BoolOp::Cut, 1e-7).unwrap();
        assert_eq!(cut.len(), 1);
        assert_eq!(cut[0].holes.len(), 1);
        assert!((cut[0].holes[0].signed_area().abs() - 2.5e-9).abs() < 1e-13);
        let intersect = planar_regions_boolean(&[circle], &[square], BoolOp::Intersect, 1e-7).unwrap();
        assert_eq!(intersect.len(), 1);
        assert!((intersect[0].area() - 2.5e-9).abs() < 1e-13);
    }
    #[test]
    fn planar_region_api_rejects_hostile_values_and_budgets() {
        let plate = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        for tolerance in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            assert!(planar_regions_boolean(std::slice::from_ref(&plate), &[], BoolOp::Union, tolerance).is_err());
        }
        let mut bad = plate.clone();
        bad.outer.segs[0] = Seg2::Line { a: Vec2::new(f64::NAN, 0.0), b: Vec2::X };
        assert!(planar_regions_boolean(&[bad], &[], BoolOp::Union, 1e-7).is_err());
        let many = vec![plate; MAX_PLANAR_SEGMENTS / 4 + 1];
        assert!(planar_regions_boolean(&many, &[], BoolOp::Union, 1e-7).is_err());
    }
}
