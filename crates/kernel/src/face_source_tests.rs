use crate::{FaceSource, extrude, transform_matrix};
use solvecraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

#[test]
fn constructed_annulus_sides_follow_exact_source_edges() {
    let ring = Region2 { outer: Loop2::circle(Vec2::default(), 10.), holes: vec![Loop2::circle(Vec2::default(), 9.).reversed()] };
    for (start, end) in [(0., 7.), (-7., 0.), (-7., 7.)] {
        let bodies = extrude(&Plane::XY, std::slice::from_ref(&ring), start, end).unwrap();
        let body = &bodies[0];
        let sources = body.face_sources().unwrap();
        assert_eq!(sources.len(), body.face_count());
        let mesh = body.tessellate(0.001).unwrap();
        let mut inner = 0;
        let mut outer = 0;
        for (index, source) in sources.iter().enumerate() {
            let Some(FaceSource::ExtrudeSide { segment: Seg2::Arc { radius, .. } }) = source else { continue };
            if (*radius - 10.).abs() < 1e-12 {
                outer += 1;
            } else {
                assert!((*radius - 9.).abs() < 1e-12);
                inner += 1;
            }
            let mut vertices = 0;
            for (triangle, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
                if *face as usize != index {
                    continue;
                }
                for point in mesh.tri(triangle).unwrap() {
                    assert!((point.x.hypot(point.y) - radius).abs() < 1e-8, "face {index}, source {radius}, {point:?}");
                    vertices += 1;
                }
            }
            assert!(vertices > 0);
        }
        assert_eq!((outer, inner), (2, 2), "full circles split into two actual edges each");
        assert_eq!(sources.iter().filter(|source| source.is_none()).count(), 2);
    }
}

#[test]
fn constructed_side_identity_survives_affine_placement_without_recognition() {
    let region = Region2 { outer: Loop2::circle(Vec2::default(), 10.), holes: vec![] };
    let bodies = extrude(&Plane::named("XY").unwrap(), &[region], 0., 7.).unwrap();
    let source = &bodies[0];
    let placed = transform_matrix(source, [[0., 1., 0., 0.], [-1., 0., 0., 0.], [0., 0., 1., 0.], [25., 30., 40., 1.]]).unwrap();
    assert_eq!(placed.face_sources(), source.face_sources());
    assert!((placed.size() - source.size()).abs() < 1e-8);
    assert!(placed.faces(0.001).unwrap().iter().all(|face| face.centroid.dist(Vec3::new(25., 30., 43.5)) < 11.));
}

#[test]
fn constructed_large_arc_split_and_reversal_preserve_one_source_segment() {
    let arc = Seg2::Arc { center: Vec2::default(), radius: 10., start: 0., sweep: std::f64::consts::PI * 5. / 3. };
    let outline = Loop2 { segs: vec![arc, Seg2::Line { a: arc.end(), b: arc.start() }] };
    for outline in [outline.clone(), outline.reversed()] {
        let body = extrude(&Plane::XY, &[Region2 { outer: outline, holes: vec![] }], 0., 7.).unwrap().remove(0);
        let sources = body.face_sources().unwrap();
        let arcs: Vec<_> = sources
            .iter()
            .filter_map(|source| match source {
                Some(FaceSource::ExtrudeSide { segment: Seg2::Arc { radius, sweep, .. } }) => Some((radius, sweep)),
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 2, "large input arc explicitly creates two actual boundary edges");
        assert!(arcs.iter().all(|(radius, sweep)| (**radius - 10.).abs() < 1e-12 && (sweep.abs() - std::f64::consts::PI * 5. / 3.).abs() < 1e-12));
        assert_eq!(sources.iter().filter(|source| matches!(source, Some(FaceSource::ExtrudeSide { segment: Seg2::Line { .. } }))).count(), 1);
        let mesh = body.tessellate(0.001).unwrap();
        for (triangle, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
            if matches!(sources[*face as usize], Some(FaceSource::ExtrudeSide { segment: Seg2::Arc { .. } })) {
                assert!(mesh.tri(triangle).unwrap().iter().all(|point| (point.x.hypot(point.y) - 10.).abs() < 1e-7));
            }
        }
    }
}
