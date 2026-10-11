//! Recording the exact history of the bodies the creation builders make (extrude, revolve,
//! sweeps, lofts, primitives, and the builders that rebuild a body around new faces), by
//! topology identity inside the building call: which face a profile edge was swept into,
//! which edges an operand still owns. Nothing is sampled.
//!
//! Profile numbering: the segments of the builder's regions in the caller's order (each region
//! its outer loop, then its holes), as [`FaceOrigin::Profile`] counts them. Builders turn loops
//! counter-clockwise (holes clockwise) before building; [`loop_origins`] undoes that.

use std::collections::HashMap;

use solvecraft_geom::{Loop2, Region2, Seg2};
use truck_modeling as mt;

use crate::Result;
use crate::body::{Body, edge_order};
use crate::history::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History};

/// The profile number of each segment of `lp` as `ccw()` orders it (as `ccw().reversed()` does
/// for a hole), the loop's own first segment being number `base`.
pub(crate) fn ccw_numbers(lp: &Loop2, hole: bool, base: usize) -> Vec<usize> {
    let n = lp.segs.len();
    let reversed = lp.is_ccw() == hole;
    (0..n).map(|j| base + if reversed { n - 1 - j } else { j }).collect()
}

/// For every loop of `regions` (each region's outer loop, then its holes), the profile
/// segments under each of its segments in the order the builders orient the loop (one each,
/// until a builder re-splits the loop).
pub(crate) fn loop_origins(regions: &[Region2]) -> Vec<Vec<Vec<usize>>> {
    let mut base = 0;
    let mut out = Vec::new();
    for r in regions {
        for (k, lp) in std::iter::once(&r.outer).chain(&r.holes).enumerate() {
            out.push(ccw_numbers(lp, k > 0, base).into_iter().map(|n| vec![n]).collect());
            base += lp.segs.len();
        }
    }
    out
}

/// Which arcs of `originals` (arcs of one circle) each arc of `pieces` (arcs of the same
/// circle) runs over: a circle re-split at other angles comes from every original arc it
/// overlaps.
pub(crate) fn arc_overlaps(pieces: &[Seg2], originals: &[Seg2]) -> Vec<Vec<usize>> {
    use std::f64::consts::TAU;
    // An arc as its counter-clockwise start angle and (positive) sweep.
    let span = |s: &Seg2| match *s {
        Seg2::Arc { start, sweep, .. } => Some(if sweep < 0.0 { (start + sweep, -sweep) } else { (start, sweep) }),
        _ => None,
    };
    let eps = 1e-9;
    pieces
        .iter()
        .map(|p| {
            let Some((p0, pw)) = span(p) else { return Vec::new() };
            (0..originals.len())
                .filter(|&j| {
                    originals.get(j).and_then(span).is_some_and(|(o0, ow)| {
                        // The original's start, seen from the piece's start going round.
                        let d = (o0 - p0).rem_euclid(TAU);
                        d < pw - eps || d + ow > TAU + eps || ow >= TAU - eps
                    })
                })
                .collect()
        })
        .collect()
}

/// A face made from profile segments `numbers`.
pub(crate) fn profile(numbers: &[usize]) -> FaceProv {
    generated(numbers.iter().map(|&segment| FaceOrigin::Profile { segment }).collect())
}

/// A face made from all of `numbers` (in any order, repeats allowed).
pub(crate) fn profile_union(numbers: impl IntoIterator<Item = usize>) -> FaceProv {
    let numbers: std::collections::BTreeSet<usize> = numbers.into_iter().collect();
    generated(numbers.into_iter().map(|segment| FaceOrigin::Profile { segment }).collect())
}

/// Walls from the given profile segments in order, then the start and end caps: the face
/// order of every builder that closes its walls with [`crate::build::cap_and_close`].
pub(crate) fn walls_then_caps(walls: impl IntoIterator<Item = Vec<usize>>) -> Vec<FaceProv> {
    walls.into_iter().map(|w| profile(&w)).chain([cap(false), cap(true)]).collect()
}

/// The index of each of `body`'s edges by identity, in its edge order.
pub(crate) fn edge_index(body: &Body) -> HashMap<mt::EdgeID, usize> {
    edge_order(body.solid.face_iter()).0.iter().enumerate().map(|(i, e)| (e.id(), i)).collect()
}

/// A face of operand `operand` and how this one relates to it.
pub(crate) fn from_face(operand: usize, face: usize, change: Change) -> FaceProv {
    FaceProv { origins: vec![FaceOrigin::Face { operand, face }], change }
}

/// A start (`end == false`) or end cap.
pub(crate) fn cap(end: bool) -> FaceProv {
    generated(vec![FaceOrigin::Cap { end }])
}

/// A face new to the operation, made from `origins`.
pub(crate) fn generated(origins: Vec<FaceOrigin>) -> FaceProv {
    FaceProv { origins, change: Change::Generated }
}

/// Attach to `body`, made from `operands`, the history whose faces are `faces` (in its face
/// order). Each edge's origin is the operand edge it is (the very edge, kept by the building
/// code), if any.
pub(crate) fn attach(body: Body, operands: &[&Body], faces: Vec<FaceProv>) -> Result<Body> {
    attach_edges(body, operands, faces, HashMap::new())
}

/// [`attach`], with `copies`: result edges the building code made as exact copies of operand
/// edges (the same curve, rebuilt), and which.
pub(crate) fn attach_edges(body: Body, operands: &[&Body], faces: Vec<FaceProv>, copies: HashMap<mt::EdgeID, EdgeOrigin>) -> Result<Body> {
    let mut owned = copies;
    for (operand, b) in operands.iter().enumerate() {
        for (id, edge) in edge_index(b) {
            owned.entry(id).or_insert(EdgeOrigin::Edge { operand, edge });
        }
    }
    let (edges, bounds) = edge_order(body.solid.face_iter());
    let edges = edges.iter().zip(bounds).map(|(e, faces)| EdgeProv { faces, origins: owned.get(&e.id()).copied().into_iter().collect() }).collect();
    let history = History { operands: operands.len(), faces, edges };
    body.with_history(history, operands)
}

/// How far a column of swept faces runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Column {
    /// Until no face lies beyond (an open sweep whose caps are not among the faces) or the
    /// column comes back to its first face (a full turn).
    ToEnd,
    /// Exactly this many faces (the sweep's caps are among the faces).
    Steps(usize),
}

/// Follow swept faces from the profile edges (`seeds`) they start at. A face swept from an
/// edge is bounded by one loop of four edges with the edge and its moved copy opposite each
/// other, and the copy is where the next step of the sweep starts (truck builds every swept,
/// revolved and ruled side face that way). The face at `skip` is never entered. Returns, for each
/// face, the seed whose column it lies in; `None` when the faces don't follow that pattern
/// (a face reached twice, a column with a fork or of the wrong length).
pub(crate) fn follow<'a>(
    faces: impl IntoIterator<Item = &'a mt::Face>,
    seeds: &[mt::EdgeID],
    skip: Option<usize>,
    run: Column,
) -> Option<Vec<Option<usize>>> {
    let faces: Vec<&mt::Face> = faces.into_iter().collect();
    let mut by_edge: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        if skip == Some(i) {
            continue;
        }
        for e in f.absolute_boundaries().iter().flat_map(|w| w.edge_iter()) {
            let slot = by_edge.entry(e.id()).or_default();
            if !slot.contains(&i) {
                slot.push(i);
            }
        }
    }
    let mut owner: Vec<Option<usize>> = vec![None; faces.len()];
    let limit = match run {
        Column::Steps(n) => n,
        Column::ToEnd => faces.len(),
    };
    for (seed, start) in seeds.iter().enumerate() {
        let (mut edge, mut prev, mut first) = (*start, None, None);
        let mut count = 0;
        while count < limit {
            let next: Vec<usize> = by_edge.get(&edge).map(|v| v.iter().copied().filter(|f| Some(*f) != prev).collect()).unwrap_or_default();
            let next = match (next.as_slice(), first) {
                ([], _) => break,
                ([f], _) => *f,
                // A full turn: the profile edge bounds the first face and the last.
                ([f, _], None) if run == Column::ToEnd => *f,
                _ => return None,
            };
            if Some(next) == first {
                break;
            }
            let slot = owner.get_mut(next)?;
            if slot.is_some() {
                return None;
            }
            *slot = Some(seed);
            let [wire] = faces.get(next)?.absolute_boundaries().as_slice() else { return None };
            let ids: Vec<mt::EdgeID> = wire.edge_iter().map(|e| e.id()).collect();
            if ids.len() != 4 {
                return None;
            }
            let at = ids.iter().position(|id| *id == edge)?;
            edge = *ids.get((at + 2) % 4)?;
            prev = Some(next);
            first.get_or_insert(next);
            count += 1;
        }
        if matches!(run, Column::Steps(n) if count != n) {
            return None;
        }
    }
    Some(owner)
}

/// The history of a solid swept from the planar face `profile` (truck's `tsweep` or `rsweep`
/// of it), whose boundary edges `seeds` come from the profile segments given with them. Open
/// (`closed == false`): the profile face is the start cap, each edge's faces follow from it to
/// the end cap (the one face left), the same number of faces for every edge. Closed (a full
/// turn): no caps, each edge's faces go round.
pub(crate) fn swept_face(solid: &crate::body::Solid, profile: &mt::Face, seeds: &[(mt::EdgeID, Vec<usize>)], closed: bool) -> Option<Vec<FaceProv>> {
    let faces: Vec<&mt::Face> = solid.face_iter().collect();
    let start = faces.iter().position(|f| f.id() == profile.id());
    let run = match (closed, start) {
        (true, None) => Column::ToEnd,
        (false, Some(_)) => {
            // Two caps, and as many faces for every edge.
            let steps = faces.len().checked_sub(2)?.checked_div(seeds.len())?;
            if steps == 0 || steps * seeds.len() + 2 != faces.len() {
                return None;
            }
            Column::Steps(steps)
        }
        _ => return None,
    };
    // Every face but the caps lies in a column, so the one face left open is the end cap.
    columns(faces, seeds, start, run)?
        .into_iter()
        .enumerate()
        .map(|(i, p)| match p {
            Some(p) => Some(p),
            None if Some(i) == start => Some(cap(false)),
            None => (!closed).then(|| cap(true)),
        })
        .collect()
}

/// Each face's provenance by the column it lies in ([`follow`]).
fn columns<'a>(
    faces: impl IntoIterator<Item = &'a mt::Face>,
    seeds: &[(mt::EdgeID, Vec<usize>)],
    skip: Option<usize>,
    run: Column,
) -> Option<Vec<Option<FaceProv>>> {
    let ids: Vec<mt::EdgeID> = seeds.iter().map(|(id, _)| *id).collect();
    Some(follow(faces, &ids, skip, run)?.into_iter().map(|o| o.and_then(|s| seeds.get(s)).map(|(_, numbers)| profile(numbers))).collect())
}

/// Side faces swept from `seeds` (edges with the profile segments under them; see [`follow`]),
/// then the start and end caps, the order [`crate::build::cap_and_close`] leaves them in.
/// `None` unless every side lies in some edge's column.
pub(crate) fn capped_sides<'a>(
    faces: impl IntoIterator<Item = &'a mt::Face>,
    seeds: &[(mt::EdgeID, Vec<usize>)],
    run: Column,
) -> Option<Vec<FaceProv>> {
    let mut out: Vec<FaceProv> = columns(faces, seeds, None, run)?.into_iter().collect::<Option<_>>()?;
    out.extend([cap(false), cap(true)]);
    Some(out)
}

/// Attach `faces` when recorded; a body whose faces could not be followed has no history.
pub(crate) fn attach_if(body: Body, operands: &[&Body], faces: Option<Vec<FaceProv>>) -> Result<Body> {
    match faces {
        Some(faces) => attach(body, operands, faces),
        None => Ok(body),
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
