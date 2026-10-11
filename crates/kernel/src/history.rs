//! Exact face and edge history: for each face and edge of an operation's result, what it came
//! from among the operation's operands, recorded by the code that built it (never sampled).
//!
//! Indices are positions in a body's face order (`face_iter`) and edge order (`unique_edges`,
//! the order of [`Body::edges`] and of a tessellation's edges). Operand numbers are positions in
//! the operation's own operand list, which each operation documents. A body made by an
//! operation carries its history (see [`Body::history`]); [`History::substitute`] composes an
//! operation with one that made one of its operands.

use std::collections::BTreeSet;

use crate::body::{Body, FaceSource, Solid};
use crate::{KernelError, Result};

/// Where a result face came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaceOrigin {
    /// A face of input `operand` (its index in the operand's face order).
    Face { operand: usize, face: usize },
    /// Swept from segment `segment` of the profile's loops (in order: outer, then holes).
    Profile { segment: usize },
    /// The start or end cap of a sweep (the profile itself, or where it ends up).
    Cap { end: bool },
    /// Made in place of edge `edge` of input `operand` (a fillet or chamfer).
    Edge { operand: usize, edge: usize },
    /// An offset of face `face` of input `operand` (a shell's inner wall).
    Offset { operand: usize, face: usize },
}

/// How a result face relates to its origins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The same face as its origin (same region).
    Kept,
    /// Its origin's surface, trimmed differently.
    Modified,
    /// One of several result faces made from one input face.
    Split,
    /// Made from several input faces (coplanar or cosurface faces joined).
    Merged,
    /// New to the operation (no input face under it).
    Generated,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceProv {
    pub origins: Vec<FaceOrigin>,
    pub change: Change,
}

/// Where a result edge came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeOrigin {
    /// (Part of) edge `edge` of input `operand`.
    Edge { operand: usize, edge: usize },
    /// Where two faces meet that met nowhere before (a boolean's intersection curve).
    Meet { a: FaceOrigin, b: FaceOrigin },
}

/// A result edge: the result faces it separates and what it came from (empty when it is new
/// and lies between faces new to the operation).
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeProv {
    pub faces: Vec<usize>,
    pub origins: Vec<EdgeOrigin>,
}

/// History of an operation's result: one entry per face and per edge, in the result's orders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    /// How many operands the operation had.
    pub operands: usize,
    pub faces: Vec<FaceProv>,
    pub edges: Vec<EdgeProv>,
}

fn failed(detail: &str) -> KernelError {
    KernelError::Failed(format!("history: {detail}"))
}

impl FaceOrigin {
    /// The same origin with its operand (if it has one) renumbered by `f`.
    fn map_operand(self, f: impl Fn(usize) -> usize) -> FaceOrigin {
        match self {
            FaceOrigin::Face { operand, face } => FaceOrigin::Face { operand: f(operand), face },
            FaceOrigin::Edge { operand, edge } => FaceOrigin::Edge { operand: f(operand), edge },
            FaceOrigin::Offset { operand, face } => FaceOrigin::Offset { operand: f(operand), face },
            other => other,
        }
    }
}

impl EdgeOrigin {
    /// The same origin with its operands renumbered by `f`.
    fn map_operand(self, f: impl Fn(usize) -> usize) -> EdgeOrigin {
        match self {
            EdgeOrigin::Edge { operand, edge } => EdgeOrigin::Edge { operand: f(operand), edge },
            EdgeOrigin::Meet { a, b } => EdgeOrigin::Meet { a: a.map_operand(&f), b: b.map_operand(&f) },
        }
    }
}

impl History {
    /// The history of an operation that keeps every face and edge of its one operand in place
    /// (a rigid move, a rebuild that keeps the topology's order).
    pub fn identity(body: &Body) -> History {
        let faces =
            (0..body.face_count()).map(|face| FaceProv { origins: vec![FaceOrigin::Face { operand: 0, face }], change: Change::Kept }).collect();
        let edges = edge_faces(&body.solid)
            .into_iter()
            .enumerate()
            .map(|(edge, faces)| EdgeProv { faces, origins: vec![EdgeOrigin::Edge { operand: 0, edge }] })
            .collect();
        History { operands: 1, faces, edges }
    }

    /// Check that this history describes `result` made from `operands`: one entry per face and
    /// edge, and every origin within its operand's faces and edges.
    pub fn check(&self, result: &Body, operands: &[&Body]) -> Result<()> {
        let counts: Vec<(usize, usize)> = operands.iter().map(|b| (b.face_count(), b.edge_count())).collect();
        self.check_counts((result.face_count(), result.edge_count()), &counts)
    }

    /// [`History::check`] against the result's and each operand's face and edge counts.
    pub(crate) fn check_counts(&self, result: (usize, usize), counts: &[(usize, usize)]) -> Result<()> {
        if self.operands != counts.len() {
            return Err(failed("operand count does not match"));
        }
        if self.faces.len() != result.0 || self.edges.len() != result.1 {
            return Err(failed("entries do not match the result's faces and edges"));
        }
        let face_ok = |o: &FaceOrigin| match *o {
            FaceOrigin::Face { operand, face } | FaceOrigin::Offset { operand, face } => counts.get(operand).is_some_and(|c| face < c.0),
            FaceOrigin::Edge { operand, edge } => counts.get(operand).is_some_and(|c| edge < c.1),
            FaceOrigin::Profile { .. } | FaceOrigin::Cap { .. } => true,
        };
        if !self.faces.iter().all(|f| f.origins.iter().all(face_ok)) {
            return Err(failed("a face origin lies outside its operand"));
        }
        for e in &self.edges {
            if e.faces.iter().any(|f| *f >= self.faces.len()) {
                return Err(failed("an edge lies on a face outside the result"));
            }
            let ok = e.origins.iter().all(|o| match o {
                EdgeOrigin::Edge { operand, edge } => counts.get(*operand).is_some_and(|c| *edge < c.1),
                EdgeOrigin::Meet { a, b } => face_ok(a) && face_ok(b),
            });
            if !ok {
                return Err(failed("an edge origin lies outside its operand"));
            }
        }
        Ok(())
    }

    /// Compose with the history of the operation that made operand `k`: origins on operand `k`
    /// become that operation's origins, its operands (none, for a body made from a sketch)
    /// taking `k`'s place in the operand list. Fails, without changing `self`, where a lineage
    /// cannot be expressed (an offset of a face that operation generated, a blend of an edge
    /// it generated, an edge between merged faces).
    pub fn substitute(&mut self, k: usize, inner: &History) -> Result<()> {
        if k >= self.operands {
            return Err(failed("no such operand"));
        }
        let n = inner.operands;
        // Operands before k stay, k's come in at k, those after k move by n - 1.
        let outer = |op: usize| if op < k { op } else { op + n - 1 };
        let lifted = |op: usize| op + k;
        let inner_face = |face: usize| inner.faces.get(face).ok_or_else(|| failed("a face origin lies outside the inner result"));
        let inner_edge = |edge: usize| inner.edges.get(edge).ok_or_else(|| failed("an edge origin lies outside the inner result"));
        // A face origin as origins of the composed operation, with the inner face's change.
        let map_face = |o: FaceOrigin| -> Result<(Vec<FaceOrigin>, Option<Change>)> {
            match o {
                FaceOrigin::Face { operand, face } if operand == k => {
                    let p = inner_face(face)?;
                    Ok((p.origins.iter().map(|x| x.map_operand(lifted)).collect(), Some(p.change)))
                }
                FaceOrigin::Offset { operand, face } if operand == k => match inner_face(face)?.origins.as_slice() {
                    [FaceOrigin::Face { operand, face }] => Ok((vec![FaceOrigin::Offset { operand: lifted(*operand), face: *face }], None)),
                    _ => Err(failed("an offset of a face the inner operation generated or merged")),
                },
                FaceOrigin::Edge { operand, edge } if operand == k => match inner_edge(edge)?.origins.as_slice() {
                    [EdgeOrigin::Edge { operand, edge }] => Ok((vec![FaceOrigin::Edge { operand: lifted(*operand), edge: *edge }], None)),
                    _ => Err(failed("a blend of an edge the inner operation generated")),
                },
                other => Ok((vec![other.map_operand(outer)], None)),
            }
        };
        let mut faces = Vec::with_capacity(self.faces.len());
        for f in &self.faces {
            let mut origins = BTreeSet::new();
            let (mut split, mut kept) = (f.change == Change::Split, f.change == Change::Kept);
            for o in &f.origins {
                let (os, change) = map_face(*o)?;
                origins.extend(os);
                split |= change == Some(Change::Split);
                kept &= change.is_none_or(|c| c == Change::Kept);
            }
            let origins: Vec<FaceOrigin> = origins.into_iter().collect();
            let change = if f.change == Change::Generated || !origins.iter().any(|o| matches!(o, FaceOrigin::Face { .. })) {
                Change::Generated
            } else if origins.len() > 1 {
                Change::Merged
            } else if split {
                Change::Split
            } else if kept {
                Change::Kept
            } else {
                Change::Modified
            };
            faces.push(FaceProv { origins, change });
        }
        // One side of a meeting: a single face of the composed operation.
        let side = |o: FaceOrigin| -> Result<FaceOrigin> {
            match map_face(o)?.0.as_slice() {
                [x] => Ok(*x),
                _ => Err(failed("an edge meets a face the inner operation merged")),
            }
        };
        let mut edges = Vec::with_capacity(self.edges.len());
        for e in &self.edges {
            let mut origins = BTreeSet::new();
            for o in &e.origins {
                match *o {
                    EdgeOrigin::Edge { operand, edge } if operand == k => {
                        origins.extend(inner_edge(edge)?.origins.iter().map(|x| x.map_operand(lifted)))
                    }
                    EdgeOrigin::Edge { .. } => {
                        origins.insert(o.map_operand(outer));
                    }
                    EdgeOrigin::Meet { a, b } => {
                        origins.insert(EdgeOrigin::Meet { a: side(a)?, b: side(b)? });
                    }
                }
            }
            edges.push(EdgeProv { faces: e.faces.clone(), origins: origins.into_iter().collect() });
        }
        self.operands = self.operands + n - 1;
        self.faces = faces;
        self.edges = edges;
        Ok(())
    }

    /// The result's face sources, carried from its operands' through this history: a face
    /// kept, modified or split from one face keeps that face's source; a merged face keeps the
    /// source its origins share, if they share one; a generated face has none.
    pub fn face_sources(&self, operands: &[&Body]) -> Vec<Option<FaceSource>> {
        let source = |o: &FaceOrigin| match *o {
            FaceOrigin::Face { operand, face } => operands.get(operand).and_then(|b| b.face_sources()).and_then(|s| s.get(face)).cloned().flatten(),
            _ => None,
        };
        self.faces
            .iter()
            .map(|f| {
                if f.change == Change::Generated {
                    return None;
                }
                let mut sources = f.origins.iter().map(source);
                let first = sources.next().flatten()?;
                sources.all(|s| s.as_ref() == Some(&first)).then_some(first)
            })
            .collect()
    }
}

/// The faces each edge separates, in the body's edge order.
pub(crate) fn edge_faces(solid: &Solid) -> Vec<Vec<usize>> {
    crate::body::edge_order(solid.face_iter()).1
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
