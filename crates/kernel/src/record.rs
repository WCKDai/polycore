//! Recording the exact history of the bodies the creation builders make (extrude, revolve,
//! sweeps, lofts, primitives, and the builders that rebuild a body around new faces), by
//! topology identity inside the building call: which face a profile edge was swept into,
//! which edges an operand still owns. Nothing is sampled.
//!
//! Faces our code builds are paired with their provenance as they are made (a [`Side`]); only
//! where truck makes the faces itself (`tsweep`, `rsweep`, `cone`) are they found afterwards,
//! by following each profile edge's column of swept faces ([`follow`]). Profile segments are
//! numbered by [`crate::numbered`].

use std::collections::{BTreeSet, HashMap};

use truck_modeling as mt;

use crate::Result;
use crate::body::{Body, edge_order};
use crate::history::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History};

/// A face as built, with its provenance (`None` where the construction could not be followed).
pub(crate) type Side = (mt::Face, Option<FaceProv>);

/// A face made from all of profile segments `numbers` (in any order, repeats allowed).
pub(crate) fn profile<'a>(numbers: impl IntoIterator<Item = &'a usize>) -> FaceProv {
    let numbers: BTreeSet<usize> = numbers.into_iter().copied().collect();
    generated(numbers.into_iter().map(|segment| FaceOrigin::Profile { segment }).collect())
}

/// A face of operand `operand` and how this one relates to it.
pub(crate) fn from_face(operand: usize, face: usize, change: Change) -> FaceProv {
    FaceProv { origins: vec![FaceOrigin::Face { operand, face }], change }
}

/// Every face of the one operand (with `count` faces) but `except`, kept as it is, in order:
/// the faces a builder carries over unchanged when it rebuilds a body around one face.
pub(crate) fn kept_except(count: usize, except: usize) -> impl Iterator<Item = FaceProv> {
    (0..count).filter(move |i| *i != except).map(|i| from_face(0, i, Change::Kept))
}

/// A start (`end == false`) or end cap.
pub(crate) fn cap(end: bool) -> FaceProv {
    generated(vec![FaceOrigin::Cap { end }])
}

/// A face new to the operation, made from `origins`.
pub(crate) fn generated(origins: Vec<FaceOrigin>) -> FaceProv {
    FaceProv { origins, change: Change::Generated }
}

/// Result faces that are each a region of input faces, given as the input faces under it and
/// whether it is one input face untouched (`whole`). Kept when whole; Merged with several
/// origins; Split when its one origin lies under other result faces too; Modified otherwise;
/// Generated with no origin.
pub(crate) fn regions(faces: Vec<(BTreeSet<FaceOrigin>, bool)>) -> Vec<FaceProv> {
    let mut faces: Vec<FaceProv> = faces
        .into_iter()
        .map(|(origins, whole)| {
            let change = match origins.len() {
                0 => Change::Generated,
                1 if whole => Change::Kept,
                1 => Change::Modified,
                _ => Change::Merged,
            };
            FaceProv { origins: origins.into_iter().collect(), change }
        })
        .collect();
    mark_shared_split(&mut faces);
    faces
}

/// Faces of one origin that is under other faces too, and that still say Kept or Modified,
/// are Split (also where histories are put side by side: pieces of one body can share a
/// source).
pub(crate) fn mark_shared_split(faces: &mut [FaceProv]) {
    let mut uses: HashMap<FaceOrigin, usize> = HashMap::new();
    for o in faces.iter().flat_map(|f| &f.origins) {
        *uses.entry(*o).or_default() += 1;
    }
    for f in faces.iter_mut() {
        if matches!(f.change, Change::Kept | Change::Modified)
            && let [o] = f.origins.as_slice()
            && uses.get(o).copied().unwrap_or(0) > 1
        {
            f.change = Change::Split;
        }
    }
}

/// The index of each of `solid`'s edges by identity, in its edge order.
pub(crate) fn edge_index(solid: &crate::body::Solid) -> HashMap<mt::EdgeID, usize> {
    edge_order(solid.face_iter()).0.iter().enumerate().map(|(i, e)| (e.id(), i)).collect()
}

/// The index of each of `solid`'s faces by identity, in its face order.
pub(crate) fn face_index(solid: &crate::body::Solid) -> HashMap<mt::FaceID, usize> {
    solid.face_iter().enumerate().map(|(i, f)| (f.id(), i)).collect()
}

/// Each of `solid`'s edges, by identity, as edge `i` (its position in the solid's edge order) of
/// operand `operand`: the operand's own edges, or a copy's made by `Body::deep_copy` or truck's
/// mapping builders (`translated`, `transformed`), which build the same faces, wires and edges
/// in the same order, so position `i` of the copy is position `i` of the body it copies.
pub(crate) fn edge_origins(solid: &crate::body::Solid, operand: usize) -> HashMap<mt::EdgeID, Vec<EdgeOrigin>> {
    edge_order(solid.face_iter()).0.iter().enumerate().map(|(edge, e)| (e.id(), vec![EdgeOrigin::Edge { operand, edge }])).collect()
}

/// The edge entries of the history of `body`, each edge's origins from `origins` (by identity);
/// `None` when an edge has none there.
pub(crate) fn edge_provs(body: &Body, origins: &HashMap<mt::EdgeID, Vec<EdgeOrigin>>) -> Option<Vec<EdgeProv>> {
    let (edges, bounds) = edge_order(body.solid.face_iter());
    edges.iter().zip(bounds).map(|(e, faces)| Some(EdgeProv { faces, origins: origins.get(&e.id())?.clone() })).collect()
}

/// `body` with the history whose faces are `faces` (in its face order) and whose edges'
/// origins are `edges` (by identity), made from operands with the face and edge `counts`; with
/// no history where either is missing, an edge has no entry, or the history does not describe
/// the body (an operation whose history could not be recorded still gives its result).
pub(crate) fn traced(
    body: Body,
    counts: &[(usize, usize)],
    faces: Option<Vec<FaceProv>>,
    edges: Option<&HashMap<mt::EdgeID, Vec<EdgeOrigin>>>,
) -> Body {
    let history = faces.zip(edges).and_then(|(faces, edges)| Some(History { operands: counts.len(), faces, edges: edge_provs(&body, edges)? }));
    body.with_checked_history(history, counts)
}

/// [`traced`] of the body `Body::new` makes of `solid`: faces given in `solid`'s face order
/// and edges by identity, both of which `Body::new` keeps.
pub(crate) fn build(
    solid: crate::body::Solid,
    counts: &[(usize, usize)],
    faces: Option<Vec<FaceProv>>,
    edges: Option<&HashMap<mt::EdgeID, Vec<EdgeOrigin>>>,
) -> Result<Body> {
    Ok(traced(Body::new(solid)?, counts, faces, edges))
}

/// The face and edge counts of `bodies`, for [`traced`].
pub(crate) fn counts(bodies: &[&Body]) -> Vec<(usize, usize)> {
    bodies.iter().map(|b| (b.face_count(), b.edge_count())).collect()
}

/// An operation's operand, with its edges indexed by identity (in its edge order).
pub(crate) struct Operand<'a> {
    pub body: &'a Body,
    pub edges: HashMap<mt::EdgeID, usize>,
}

impl<'a> Operand<'a> {
    pub(crate) fn new(body: &'a Body) -> Operand<'a> {
        Operand { body, edges: edge_index(&body.solid) }
    }
}

/// Attach to `body`, made from `operands`, the history whose faces are `faces` (in its face
/// order); a body whose faces could not be followed (`None`) has no history. Each edge's
/// origin is the operand edge it is (the very edge, kept by the building code), or what
/// `copies` says: result edges the building code made as exact copies of operand edges.
pub(crate) fn attach(body: Body, operands: &[Operand], faces: Option<Vec<FaceProv>>, copies: HashMap<mt::EdgeID, EdgeOrigin>) -> Result<Body> {
    let Some(faces) = faces else { return Ok(body) };
    let mut owned = copies;
    for (operand, o) in operands.iter().enumerate() {
        for (id, edge) in &o.edges {
            owned.entry(*id).or_insert(EdgeOrigin::Edge { operand, edge: *edge });
        }
    }
    let (edges, bounds) = edge_order(body.solid.face_iter());
    let edges = edges.iter().zip(bounds).map(|(e, faces)| EdgeProv { faces, origins: owned.get(&e.id()).copied().into_iter().collect() }).collect();
    let history = History { operands: operands.len(), faces, edges };
    let counts: Vec<(usize, usize)> = operands.iter().map(|o| (o.body.face_count(), o.edges.len())).collect();
    body.with_history_counts(history, &counts)
}

/// Ruled faces (truck's `try_wire_homotopy` of wires `a` and `b`), each with the provenance of
/// the edges it spans: every face holds one edge of each wire, and comes from the profile
/// segments under both (`a_numbers`, `b_numbers`: by edge, in wire order).
pub(crate) fn ruled(faces: &mt::Shell, a: &mt::Wire, b: &mt::Wire, a_numbers: &[Vec<usize>], b_numbers: &[Vec<usize>]) -> Vec<Side> {
    let under = |w: &mt::Wire, numbers: &[Vec<usize>]| -> HashMap<mt::EdgeID, Vec<usize>> {
        w.edge_iter().map(|e| e.id()).zip(numbers.iter().cloned()).collect()
    };
    let (ua, ub) = (under(a, a_numbers), under(b, b_numbers));
    faces
        .face_iter()
        .map(|f| {
            let held = |u: &HashMap<mt::EdgeID, Vec<usize>>| -> Option<Vec<usize>> {
                let mut found = f.absolute_boundaries().iter().flat_map(|w| w.edge_iter()).filter_map(|e| u.get(&e.id()));
                let one = found.next()?.clone();
                found.next().is_none().then_some(one)
            };
            let prov = held(&ua).zip(held(&ub)).map(|(x, y)| profile(x.iter().chain(&y)));
            (f.clone(), prov)
        })
        .collect()
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

/// Follow the faces truck sweeps from the profile edges (`seeds`) they start at. A face swept
/// from an edge is bounded by one loop: the edge, its moved copy, and the paths its ends sweep
/// (none where an end stays put: a cone's apex, `fixed`). The copy is the loop's one other edge
/// that shares no moving vertex with the edge, and is where the next step of the sweep starts.
/// The face at `skip` is never entered. Returns, for each face, the seed whose column it lies
/// in; `None` when the faces don't follow that pattern (a face reached twice, a column with a
/// fork or of the wrong length).
pub(crate) fn follow<'a>(
    faces: impl IntoIterator<Item = &'a mt::Face>,
    seeds: &[mt::EdgeID],
    skip: Option<usize>,
    run: Column,
    fixed: &[mt::VertexID],
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
            let here = wire.edge_iter().find(|e| e.id() == edge)?;
            let moving: Vec<mt::VertexID> = [here.front().id(), here.back().id()].into_iter().filter(|v| !fixed.contains(v)).collect();
            let mut copies = wire.edge_iter().filter(|e| e.id() != edge && !moving.contains(&e.front().id()) && !moving.contains(&e.back().id()));
            let (Some(copy), None) = (copies.next(), copies.next()) else { return None };
            edge = copy.id();
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

/// Each face's provenance by the column it lies in ([`follow`]), from the profile segments
/// under each seed edge.
pub(crate) fn columns<'a>(
    faces: impl IntoIterator<Item = &'a mt::Face>,
    seeds: &[(mt::EdgeID, Vec<usize>)],
    skip: Option<usize>,
    run: Column,
    fixed: &[mt::VertexID],
) -> Option<Vec<Option<FaceProv>>> {
    let ids: Vec<mt::EdgeID> = seeds.iter().map(|(id, _)| *id).collect();
    Some(follow(faces, &ids, skip, run, fixed)?.into_iter().map(|o| o.and_then(|s| seeds.get(s)).map(|(_, numbers)| profile(numbers))).collect())
}

/// The history of a solid truck swept from the planar face `profile` (`rsweep` of it), whose
/// boundary edges `seeds` come from the profile segments given with them. Open
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
    columns(faces, seeds, start, run, &[])?
        .into_iter()
        .enumerate()
        .map(|(i, p)| match p {
            Some(p) => Some(p),
            None if Some(i) == start => Some(cap(false)),
            None => (!closed).then(|| cap(true)),
        })
        .collect()
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
