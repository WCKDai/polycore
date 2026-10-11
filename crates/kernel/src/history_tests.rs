use solvecraft_geom::Vec3;

use super::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History};
use crate::{Body, box_solid, transform};

fn face(operand: usize, face: usize) -> FaceOrigin {
    FaceOrigin::Face { operand, face }
}

fn prov(origins: Vec<FaceOrigin>, change: Change) -> FaceProv {
    FaceProv { origins, change }
}

fn centroids(b: &Body) -> Vec<Vec3> {
    b.faces(1e-3).unwrap().into_iter().map(|f| f.centroid).collect()
}

#[test]
fn a_transform_keeps_every_face_and_edge_in_place() {
    let a = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
    let t = Vec3::new(5.0, -3.0, 2.0);
    let moved = transform(&a, t, Vec3::ZERO, Vec3::Z, 0.7).unwrap();
    let h = moved.history().unwrap();
    h.check(&moved, &[&a]).unwrap();
    assert!(h.faces.iter().enumerate().all(|(i, f)| f.origins == vec![face(0, i)] && f.change == Change::Kept));
    // Face i of the result is face i of the input, moved.
    let rot = |p: Vec3| {
        let (s, c) = 0.7f64.sin_cos();
        Vec3::new(p.x * c - p.y * s, p.x * s + p.y * c, p.z) + t
    };
    for (before, after) in centroids(&a).into_iter().zip(centroids(&moved)) {
        assert!(rot(before).dist(after) < 1e-6, "{before:?} → {after:?}");
    }
    // Each edge separates the same faces it did.
    let (ea, em) = (super::edge_faces(&a.solid), super::edge_faces(&moved.solid));
    assert_eq!(ea, em);
    assert!(h.edges.iter().all(|e| e.faces.len() == 2 && e.origins.len() == 1));
    // The history's edges are the tessellation's edges, bounding the same faces.
    let mesh = moved.tessellate(1e-3).unwrap();
    let mesh_faces: Vec<Vec<usize>> = mesh.edge_faces.iter().map(|fs| fs.iter().map(|f| *f as usize).collect()).collect();
    assert_eq!(mesh_faces, h.edges.iter().map(|e| e.faces.clone()).collect::<Vec<_>>());
}

/// Joining an extrusion: the tool came from a sketch (no operand bodies), so substituting its
/// history removes it from the operand list and its faces become generated profile faces.
#[test]
fn substitute_a_body_made_from_a_sketch() {
    let extrude = History {
        operands: 0,
        faces: vec![prov(vec![FaceOrigin::Cap { end: false }], Change::Generated), prov(vec![FaceOrigin::Profile { segment: 3 }], Change::Generated)],
        edges: vec![EdgeProv { faces: vec![0, 1], origins: vec![] }],
    };
    let mut join = History {
        operands: 2,
        faces: vec![prov(vec![face(0, 2)], Change::Modified), prov(vec![face(1, 1)], Change::Kept)],
        edges: vec![EdgeProv { faces: vec![0, 1], origins: vec![EdgeOrigin::Meet { a: face(0, 2), b: face(1, 1) }] }],
    };
    join.substitute(1, &extrude).unwrap();
    assert_eq!(join.operands, 1);
    assert_eq!(join.faces[0], prov(vec![face(0, 2)], Change::Modified));
    assert_eq!(join.faces[1], prov(vec![FaceOrigin::Profile { segment: 3 }], Change::Generated));
    assert_eq!(join.edges[0].origins, vec![EdgeOrigin::Meet { a: face(0, 2), b: FaceOrigin::Profile { segment: 3 } }]);
}

#[test]
fn bodies_without_reported_history_have_none() {
    let a = box_solid(Vec3::ZERO, Vec3::new(4.0, 3.0, 1.0)).unwrap();
    assert!(a.history().is_none());
}

#[test]
fn check_refuses_a_history_that_does_not_describe_the_body() {
    let a = box_solid(Vec3::ZERO, Vec3::new(4.0, 3.0, 1.0)).unwrap();
    let mut h = History::identity(&a);
    h.check(&a, &[&a]).unwrap();
    assert!(h.check(&a, &[]).is_err(), "operand count");
    let mut short = h.clone();
    short.faces.pop();
    assert!(short.check(&a, &[&a]).is_err(), "face count");
    h.faces[0].origins = vec![face(0, 6)];
    assert!(h.check(&a, &[&a]).is_err(), "origin beyond the operand's faces");
    assert!(a.clone().with_history(h, &[&a]).is_err());
}

/// A two-step chain: op1 makes B from (A0, A1); op2 makes C from (X, B, Y). Composing puts
/// A0, A1 in B's place: C's operands become (X, A0, A1, Y).
#[test]
fn substitute_composes_kept_split_merged_and_generated_faces() {
    let op1 = History {
        operands: 2,
        faces: vec![
            prov(vec![face(0, 0)], Change::Kept),                              // B0 = A0.f0
            prov(vec![face(0, 1)], Change::Split),                             // B1, B2 = pieces of A0.f1
            prov(vec![face(0, 1)], Change::Split),                             //
            prov(vec![face(1, 4)], Change::Modified),                          // B3 = A1.f4 trimmed
            prov(vec![FaceOrigin::Profile { segment: 2 }], Change::Generated), // B4 new
        ],
        edges: vec![
            EdgeProv { faces: vec![0, 1], origins: vec![EdgeOrigin::Edge { operand: 0, edge: 7 }] },
            EdgeProv { faces: vec![3, 4], origins: vec![EdgeOrigin::Meet { a: face(1, 4), b: FaceOrigin::Profile { segment: 2 } }] },
        ],
    };
    let mut op2 = History {
        operands: 3,
        faces: vec![
            prov(vec![face(1, 0)], Change::Kept),                                    // kept B0 → kept A0.f0
            prov(vec![face(1, 1), face(1, 2)], Change::Merged),                      // B1 + B2 rejoined → one origin
            prov(vec![face(1, 3)], Change::Kept),                                    // kept B3 → modified A1.f4
            prov(vec![face(1, 4)], Change::Modified),                                // trimmed generated face
            prov(vec![face(0, 5), face(2, 1)], Change::Merged),                      // X and Y faces renumbered
            prov(vec![FaceOrigin::Edge { operand: 1, edge: 0 }], Change::Generated), // blend of B's edge 0
        ],
        edges: vec![
            EdgeProv { faces: vec![0, 1], origins: vec![EdgeOrigin::Edge { operand: 1, edge: 1 }] },
            EdgeProv { faces: vec![2, 4], origins: vec![EdgeOrigin::Meet { a: face(1, 3), b: face(2, 1) }] },
            EdgeProv { faces: vec![4, 5], origins: vec![EdgeOrigin::Edge { operand: 2, edge: 3 }] },
        ],
    };
    op2.substitute(1, &op1).unwrap();
    assert_eq!(op2.operands, 4);
    let f = &op2.faces;
    assert_eq!(f[0], prov(vec![face(1, 0)], Change::Kept));
    // Two pieces of one face, merged back: one origin, so not a merge across faces.
    assert_eq!(f[1], prov(vec![face(1, 1)], Change::Split));
    assert_eq!(f[2], prov(vec![face(2, 4)], Change::Modified));
    assert_eq!(f[3], prov(vec![FaceOrigin::Profile { segment: 2 }], Change::Generated));
    assert_eq!(f[4], prov(vec![face(0, 5), face(3, 1)], Change::Merged));
    assert_eq!(f[5], prov(vec![FaceOrigin::Edge { operand: 1, edge: 7 }], Change::Generated));
    let e = &op2.edges;
    assert_eq!(e[0].origins, vec![EdgeOrigin::Meet { a: face(2, 4), b: FaceOrigin::Profile { segment: 2 } }]);
    assert_eq!(e[1].origins, vec![EdgeOrigin::Meet { a: face(2, 4), b: face(3, 1) }]);
    assert_eq!(e[2].origins, vec![EdgeOrigin::Edge { operand: 3, edge: 3 }]);
}

#[test]
fn substitute_refuses_lineage_it_cannot_express_and_leaves_history_unchanged() {
    let inner = History {
        operands: 1,
        faces: vec![prov(vec![FaceOrigin::Profile { segment: 0 }], Change::Generated)],
        edges: vec![EdgeProv { faces: vec![0], origins: vec![] }],
    };
    // An offset of a face the inner operation generated.
    let mut offset = History { operands: 1, faces: vec![prov(vec![FaceOrigin::Offset { operand: 0, face: 0 }], Change::Generated)], edges: vec![] };
    let before = offset.clone();
    assert!(offset.substitute(0, &inner).is_err());
    assert_eq!(offset, before);
    // A blend of an edge the inner operation generated.
    let mut blend = History { operands: 1, faces: vec![prov(vec![FaceOrigin::Edge { operand: 0, edge: 0 }], Change::Generated)], edges: vec![] };
    assert!(blend.substitute(0, &inner).is_err());
    // Out-of-range references and operands.
    let mut bad = History { operands: 1, faces: vec![prov(vec![face(0, 9)], Change::Kept)], edges: vec![] };
    assert!(bad.substitute(0, &inner).is_err());
    assert!(bad.substitute(3, &inner).is_err());
}

#[test]
fn face_sources_follow_kept_split_and_agreeing_merged_faces() {
    let a = box_solid(Vec3::ZERO, Vec3::new(4.0, 3.0, 1.0)).unwrap().with_box_certificate(7).unwrap();
    let src = |i: usize| a.face_sources().unwrap()[i].clone();
    assert!(src(0).is_some() && src(1).is_some() && src(0) != src(1));
    let h = History {
        operands: 1,
        faces: vec![
            prov(vec![face(0, 0)], Change::Kept),
            prov(vec![face(0, 1)], Change::Split),
            prov(vec![face(0, 0), face(0, 1)], Change::Merged),
            prov(vec![face(0, 2), face(0, 2)], Change::Merged),
            prov(vec![FaceOrigin::Cap { end: true }], Change::Generated),
        ],
        edges: vec![],
    };
    let s = h.face_sources(&[&a]);
    assert_eq!(s, vec![src(0), src(1), None, src(2), None]);
}
