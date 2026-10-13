//! Display tessellation: each face's triangulation in its own parameters, with every edge
//! sampled once so neighbouring faces share their boundary points exactly, handed over as a
//! [`TessPlan`] for the CPU or GPU evaluator.
//!
//! Per face, cheapest first:
//! - planar faces: a constrained Delaunay triangulation of the edge nodes alone (a plane needs
//!   no interior points);
//! - faces whose boundary runs along their parameter box (cylinder halves, sphere and torus
//!   patches, fillets): a structured grid zipped to the edge nodes;
//! - four-cornered faces whose sides are not parameter lines (mitred fillet strips, cross
//!   hole walls): a Coons blend of the four sides, zipped the same way;
//! - anything else (trimmed faces, holes): constrained Delaunay with interior seeds and
//!   Ruppert refinement.
//!
//! A face the planner cannot handle (its surface has no evaluable form, a boundary that does
//! not project or crosses itself) fails the whole plan, and the caller meshes the body the
//! old way.

pub(crate) mod cdt;
pub(crate) mod coons;
pub(crate) mod edges;
pub(crate) mod grid;
pub(crate) mod surface;
pub(crate) mod uv;

use std::collections::HashMap;

use solidcraft_geom::Vec3;
use solidcraft_geom::tess::{EvalSurface, FacePlan, TessPlan};
use truck_modeling as mt;

use crate::body::{Solid, edge_order};
use crate::{KernelError, Result};

/// How the faces of a plan were meshed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanStats {
    pub planar: usize,
    pub structured: usize,
    pub delaunay: usize,
}

/// Most lines a curved face's grid gets in one direction.
const MAX_LINES: usize = 1024;
/// Most points Ruppert refinement may add to one face, beyond its seeds.
const MAX_REFINE: usize = 20_000;

fn unsupported(what: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("not supported yet: tessellation: {what}"))
}

/// Plan the display tessellation of `solid` with chord tolerance `tol`.
pub(crate) fn plan(solid: &Solid, tol: f64) -> Result<(TessPlan, PlanStats)> {
    let faces: Vec<&mt::Face> = solid.face_iter().collect();
    let (edges, edge_faces) = edge_order(faces.iter().copied());
    // Every edge once, in parallel; faces then read their nodes.
    let sampled = crate::par::map("edge nodes", &edges, 64, |e| Ok(edges::sample_edge(e, tol)));
    let mut nodes: HashMap<_, Vec<Vec3>> = HashMap::with_capacity(edges.len());
    for (e, pts) in edges.iter().zip(sampled) {
        nodes.insert(e.id(), pts?);
    }
    let oriented = |e: &mt::Edge| -> Option<Vec<Vec3>> {
        let mut pts = nodes.get(&e.id())?.clone();
        if !e.orientation() {
            pts.reverse();
        }
        Some(pts)
    };
    let indexed: Vec<(usize, &mt::Face)> = faces.iter().copied().enumerate().collect();
    let planned = crate::par::map("face tessellation", &indexed, 8, |(fi, face)| {
        let fi = *fi;
        let surface = face.surface();
        let eval = surface::eval_surface(&surface).ok_or_else(|| unsupported(format!("face {fi}: surface not evaluable")))?;
        // Boundary nodes, loop by loop (each edge's last node is the next edge's first).
        let mut loops3: Vec<Vec<Vec3>> = Vec::new();
        for wire in face.absolute_boundaries() {
            let mut pts = Vec::new();
            for e in wire.edge_iter() {
                let mut ep = oriented(e).ok_or_else(|| unsupported("edge without nodes"))?;
                ep.pop();
                pts.extend(ep);
            }
            if pts.len() >= 2 {
                loops3.push(pts);
            }
        }
        let mut stats = PlanStats::default();
        let fp = plan_face(fi, &eval, &loops3, tol, &mut stats)?;
        let fp = FacePlan {
            flip: !face.orientation(),
            triangles: if face.orientation() { fp.triangles } else { fp.triangles.into_iter().map(|[a, b, c]| [a, c, b]).collect() },
            ..fp
        };
        Ok((fp, eval, stats))
    });
    let mut plan = TessPlan::default();
    let mut stats = PlanStats::default();
    for r in planned {
        let (fp, eval, s) = r?;
        let surface = u32::try_from(plan.surfaces.len()).map_err(|_| unsupported("too many faces"))?;
        plan.surfaces.push(eval);
        plan.faces.push(FacePlan { surface: Some(surface), ..fp });
        stats.planar += s.planar;
        stats.structured += s.structured;
        stats.delaunay += s.delaunay;
    }
    plan.edges = edges.iter().filter_map(oriented).collect();
    plan.edge_faces = edge_faces.into_iter().map(|fs| fs.into_iter().map(|f| u32::try_from(f).unwrap_or(u32::MAX)).collect()).collect();
    Ok((plan, stats))
}

/// One face: boundary loops in 3D (shared edge nodes) to parameters and triangles.
fn plan_face(fi: usize, s: &EvalSurface, loops3: &[Vec<Vec3>], tol: f64, stats: &mut PlanStats) -> Result<FacePlan> {
    let dom = uv::domain(s);
    let accept = (tol * 10.0).max(1e-6);
    let fail = |why: &str| unsupported(format!("face {fi}: {why}"));
    // Project every loop, unwrapping periodic parameters along it and splitting poles.
    let mut fixed: Vec<Vec3> = Vec::new();
    let mut uvs: Vec<[f64; 2]> = Vec::new();
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for pts in loops3 {
        let mut luv: Vec<[f64; 2]> = Vec::with_capacity(pts.len());
        let mut prev: Option<[f64; 2]> = None;
        for p in pts {
            let mut q = uv::project(s, &dom, *p, prev, accept).ok_or_else(|| fail("boundary does not lie on the surface"))?;
            if let Some(pr) = prev {
                for d in 0..2 {
                    q[d] = uv::unwrap(q[d], pr[d], dom.period[d]);
                }
            }
            luv.push(q);
            prev = Some(q);
        }
        // A loop that winds once around a periodic direction does not close in (u, v).
        if let (Some(first), Some(last)) = (luv.first(), luv.last()) {
            for d in 0..2 {
                if let Some(per) = dom.period[d]
                    && (uv::unwrap(first[d], last[d], Some(per)) - first[d]).abs() > per * 0.25
                {
                    return Err(fail("boundary winds around the surface"));
                }
            }
        }
        let start = uvs.len();
        let n = luv.len();
        for (k, (q, p)) in luv.iter().zip(pts).enumerate() {
            // At a pole the parameter along the degenerate direction is free: one copy with
            // the previous node's value, one with the next node's.
            let (prev, next) = (luv.get((k + n - 1) % n).copied(), luv.get((k + 1) % n).copied());
            let pole = (0..2).find(|&d| uv::degenerate(s, *q, d));
            match (pole, prev, next) {
                (Some(d), Some(a), Some(b)) => {
                    let mut qa = *q;
                    let mut qb = *q;
                    qa[d] = a[d];
                    qb[d] = b[d];
                    uvs.push(qa);
                    fixed.push(*p);
                    if (qa[d] - qb[d]).abs() > 1e-12 {
                        uvs.push(qb);
                        fixed.push(*p);
                    }
                }
                _ => {
                    uvs.push(*q);
                    fixed.push(*p);
                }
            }
        }
        ranges.push(start..uvs.len());
    }
    if uvs.len() < 3 {
        return Err(fail("degenerate boundary"));
    }
    let planar = matches!(s, EvalSurface::Plane { .. });
    let (extra, triangles) = if planar {
        stats.planar += 1;
        cdt::triangulate(&uvs, &ranges, cdt::Fill { seeds: &[], refine: None }).ok_or_else(|| fail("boundary crosses itself"))?
    } else {
        let mut bbox = [(f64::MAX, f64::MIN); 2];
        for p in &uvs {
            for d in 0..2 {
                bbox[d] = (bbox[d].0.min(p[d]), bbox[d].1.max(p[d]));
            }
        }
        let mut us = uv::divide(s, 0, bbox[0], bbox[1], tol, MAX_LINES);
        let mut vs = uv::divide(s, 1, bbox[1], bbox[0], tol, MAX_LINES);
        thin(&mut us, &mut vs, grid::MAX_FACE_GRID);
        let structured = match (ranges.len(), grid::quad(&uvs)) {
            (1, Some(q)) => {
                // Interior lines: the boundary's own where opposite sides agree, else the
                // tolerance division.
                let lines = |d: usize, div: &Vec<f64>| {
                    grid::aligned_lines(&uvs, &q, d).unwrap_or_else(|| div.iter().copied().filter(|x| *x > bbox[d].0 && *x < bbox[d].1).collect())
                };
                let (lu, lv) = (lines(0, &us), lines(1, &vs));
                let strip = match (lu.is_empty(), lv.is_empty()) {
                    (false, true) => grid::strip(&uvs, &q, 0),
                    (true, false) => grid::strip(&uvs, &q, 1),
                    _ => None,
                };
                strip.map(|t| (Vec::new(), t)).or_else(|| grid::structured(&uvs, &q, &grid::interior(lu, bbox[0]), &grid::interior(lv, bbox[1])))
            }
            _ => None,
        };
        let cells = |lines: &Vec<f64>, (a, b): (f64, f64)| (lines.len().saturating_sub(1).max(1) as f64) / (b - a).max(f64::MIN_POSITIVE);
        let scale = [cells(&us, bbox[0]), cells(&vs, bbox[1])];
        let structured = match structured {
            None if ranges.len() == 1 => coons::coons(&uvs, &fixed, scale, grid::MAX_FACE_GRID),
            other => other,
        };
        match structured {
            Some(r) => {
                stats.structured += 1;
                r
            }
            None => {
                stats.delaunay += 1;
                let polys: Vec<&[[f64; 2]]> = ranges.iter().filter_map(|r| uvs.get(r.clone())).collect();
                let inner = |lines: &Vec<f64>, (a, b): (f64, f64)| lines.iter().copied().filter(|x| *x > a && *x < b).collect::<Vec<f64>>();
                let seeds = cdt::inside_grid(&inner(&us, bbox[0]), &inner(&vs, bbox[1]), &polys);
                let eval = |u: f64, v: f64| s.derivs(u, v).map(|(p, _, _)| p);
                // One unit per tolerance cell in each direction: a straight direction (one
                // cell) keeps long triangles, Ruppert shapes them in that metric.
                let sag = cdt::Sag { eval: &eval, tol, scale };
                let fill = cdt::Fill { seeds: &seeds, refine: Some((sag, MAX_REFINE)) };
                cdt::triangulate(&uvs, &ranges, fill).ok_or_else(|| fail("boundary crosses itself"))?
            }
        }
    };
    // At a pole the boundary has two copies of one node: a triangle on both copies has two
    // corners at one point and no area (its other two sides are one segment both ways).
    let pole_pair = |i: u32, j: u32| i != j && fixed.get(i as usize).zip(fixed.get(j as usize)).is_some_and(|(p, q)| p == q);
    let mut triangles = triangles;
    triangles.retain(|t| !(pole_pair(t[0], t[1]) || pole_pair(t[1], t[2]) || pole_pair(t[2], t[0])));
    if !closed_boundary(&ranges, &fixed, &triangles) {
        return Err(fail("triangulation does not follow the boundary"));
    }
    let mut all = uvs;
    all.extend(extra);
    Ok(FacePlan { face: u32::try_from(fi).unwrap_or(u32::MAX), surface: None, uv: all, fixed, normals: Vec::new(), flip: false, triangles })
}

/// Every boundary segment (consecutive nodes of a loop) is the side of exactly one triangle:
/// the face covers its boundary with nothing missing or doubled, so it meets its neighbours
/// exactly. Pole copies of one node (consecutive in a loop, at one point) are one vertex: the
/// segment between them has no length, and a side ending on either copy is the same side.
fn closed_boundary(ranges: &[std::ops::Range<usize>], fixed: &[Vec3], triangles: &[[u32; 3]]) -> bool {
    let mut one: Vec<u32> = (0..fixed.len() as u32).collect();
    let root = |one: &[u32], mut i: u32| {
        while let Some(&j) = one.get(i as usize).filter(|j| **j != i) {
            i = j;
        }
        i
    };
    let segment = |r: &std::ops::Range<usize>, k: usize| ((r.start + k) as u32, (r.start + (k + 1) % r.len()) as u32);
    for r in ranges {
        for k in 0..r.len() {
            let (a, b) = segment(r, k);
            if fixed.get(a as usize).zip(fixed.get(b as usize)).is_some_and(|(p, q)| p == q) {
                let (ra, rb) = (root(&one, a), root(&one, b));
                if let Some(x) = one.get_mut(rb.max(ra) as usize) {
                    *x = ra.min(rb);
                }
            }
        }
    }
    let side = |a: u32, b: u32| {
        let (a, b) = (root(&one, a), root(&one, b));
        (a != b).then_some((a.min(b), a.max(b)))
    };
    let mut sides: HashMap<(u32, u32), u32> = HashMap::with_capacity(triangles.len() * 3);
    for t in triangles {
        for e in 0..3 {
            if let Some(k) = side(t[e], t[(e + 1) % 3]) {
                *sides.entry(k).or_default() += 1;
            }
        }
    }
    ranges.iter().all(|r| {
        (0..r.len()).all(|k| {
            let (a, b) = segment(r, k);
            side(a, b).is_none_or(|k| sides.get(&k).copied() == Some(1))
        })
    })
}

/// Keep at most `cap` grid points by taking every k-th line in both directions (ends kept).
fn thin(us: &mut Vec<f64>, vs: &mut Vec<f64>, cap: usize) {
    let total = us.len().saturating_mul(vs.len());
    if total <= cap {
        return;
    }
    let f = (total as f64 / cap.max(4) as f64).sqrt();
    let pick = |d: &mut Vec<f64>| {
        let n = ((d.len() as f64 / f).ceil() as usize).max(2);
        if d.len() > n {
            let len = d.len();
            *d = (0..n).filter_map(|i| d.get(i * (len - 1) / (n - 1)).copied()).collect();
        }
    };
    pick(us);
    pick(vs);
}

#[cfg(test)]
mod tests;
