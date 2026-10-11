use super::*;
use crate::{BoolOp, boolean, box_solid, measure, sphere, step_export, step_import};

fn cube(half: f64) -> Body {
    box_solid(Vec3::new(-half, -half, -half), Vec3::new(half, half, half)).unwrap()
}

fn nested_island() -> Body {
    let hollow = boolean(&cube(50.0), &cube(40.0), BoolOp::Cut).unwrap().unwrap();
    // This order puts the containing outer shell ahead of the disconnected island.
    let joined = boolean(&cube(20.0), &hollow, BoolOp::Union).unwrap().unwrap();
    boolean(&joined, &cube(10.0), BoolOp::Cut).unwrap().unwrap()
}

fn assert_material(body: &Body, expected: f64, probes: &[(Vec3, bool)]) {
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    let mesh = body.tessellate_with(0.01, true).unwrap();
    assert!((mesh.measure().volume - expected).abs() < expected * 1e-6, "{} vs {expected}", mesh.measure().volume);
    for (p, want) in probes {
        assert_eq!(mesh.contains(*p), *want, "material at {p:?}");
    }
}

fn check_nested(body: &Body) {
    let probes = [(Vec3::ZERO, false), (Vec3::X * 15.0, true), (Vec3::X * 30.0, false), (Vec3::X * 45.0, true)];
    assert_material(body, 544_000.0, &probes);
    let lumps = body.lumps().unwrap();
    assert_eq!(lumps.len(), 2);
    let mut volumes: Vec<_> = lumps.iter().map(|b| measure(b).unwrap().volume).collect();
    volumes.sort_by(f64::total_cmp);
    for (got, want) in volumes.iter().zip([56_000.0, 488_000.0]) {
        assert!((got - want).abs() < 1e-6, "{volumes:?}");
    }
    let meshes: Vec<_> = lumps.iter().map(|b| b.tessellate_with(0.01, true).unwrap()).collect();
    for (p, want) in probes {
        assert_eq!(meshes.iter().filter(|m| m.contains(p)).count(), usize::from(want), "lump material at {p:?}");
    }
}

#[test]
fn nested_cavities_stay_with_their_material_islands_for_every_shell_order() {
    let body = nested_island();
    check_nested(&body);
    let before = step_export(&[&body], "nested cavities").unwrap();
    let shells = body.deep_copy().boundaries().clone();
    assert_eq!(shells.len(), 4);
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                for d in 0..4 {
                    let order = [a, b, c, d];
                    if order.iter().enumerate().any(|(i, x)| order.iter().skip(i + 1).any(|y| x == y)) {
                        continue;
                    }
                    let permuted = guard("permuted cavity shells", || {
                        Body::new(Solid::try_new(order.into_iter().map(|i| shells[i].clone()).collect()).unwrap())
                    })
                    .unwrap();
                    check_nested(&permuted);
                }
            }
        }
    }
    assert_eq!(step_export(&[&body], "nested cavities").unwrap(), before);
}

#[test]
fn nested_voids_survive_step_round_trip_and_further_cuts() {
    let body = nested_island();
    let text = step_export(&[&body], "nested cavities").unwrap();
    let imported = step_import(&text).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut lumps = Vec::new();
    for imported in imported.bodies {
        lumps.extend(imported.body.lumps().unwrap());
    }
    assert_eq!(lumps.len(), 2);
    let mut volumes: Vec<_> = lumps.iter().map(|b| measure(b).unwrap().volume).collect();
    volumes.sort_by(f64::total_cmp);
    for (got, want) in volumes.iter().zip([56_000.0, 488_000.0]) {
        assert!((got - want).abs() < 1e-3, "{volumes:?}");
    }
    let opening = box_solid(Vec3::new(5.0, -2.0, -2.0), Vec3::new(25.0, 2.0, 2.0)).unwrap();
    let mut cut_volume = 0.0;
    for lump in &lumps {
        if let Some(cut) = boolean(lump, &opening, BoolOp::Cut).unwrap() {
            cut_volume += measure(&cut).unwrap().volume;
            assert!(!cut.tessellate_with(0.01, true).unwrap().contains(Vec3::X * 15.0));
        }
    }
    // Only x=10..20 is material along this 4×4 opening: 160 mm³ removed.
    assert!((cut_volume - 543_840.0).abs() < 1e-3, "{cut_volume}");
}

#[test]
fn curved_internal_void_and_disconnected_material_keep_analytic_volume() {
    let hollow = boolean(&cube(20.0), &sphere(Vec3::ZERO, 10.0).unwrap(), BoolOp::Cut).unwrap().unwrap();
    let island = sphere(Vec3::ZERO, 4.0).unwrap();
    let joined = boolean(&island, &hollow, BoolOp::Union).unwrap().unwrap();
    let reverse = boolean(&hollow, &island, BoolOp::Union).unwrap().unwrap();
    assert!((measure(&joined).unwrap().volume - measure(&reverse).unwrap().volume).abs() < 1e-4);
    let body = boolean(&joined, &sphere(Vec3::ZERO, 2.0).unwrap(), BoolOp::Cut).unwrap().unwrap();
    let lumps = body.lumps().unwrap();
    assert_eq!(lumps.len(), 2);
    let mut volumes: Vec<_> = lumps.iter().map(|b| measure(b).unwrap().volume).collect();
    volumes.sort_by(f64::total_cmp);
    let pi = std::f64::consts::PI;
    let want = [4.0 / 3.0 * pi * (64.0 - 8.0), 64_000.0 - 4.0 / 3.0 * pi * 1000.0];
    for (got, want) in volumes.iter().zip(want) {
        assert!((got - want).abs() < want * 0.002, "{got} vs {want}");
    }
    let meshes: Vec<_> = lumps.iter().map(|b| b.tessellate_with(0.01, true).unwrap()).collect();
    for (p, want) in [(Vec3::ZERO, 0), (Vec3::X * 3.0, 1), (Vec3::X * 7.0, 0), (Vec3::X * 15.0, 1)] {
        assert_eq!(meshes.iter().filter(|m| m.contains(p)).count(), want);
    }
    let imported = step_import(&step_export(&[&body], "curved nested cavities").unwrap()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let imported: Vec<_> = imported.bodies.into_iter().flat_map(|b| b.body.lumps().unwrap()).collect();
    assert_eq!(imported.len(), 2);
    let imported_meshes: Vec<_> = imported.iter().map(|b| b.tessellate_with(0.01, true).unwrap()).collect();
    for (p, want) in [(Vec3::ZERO, 0), (Vec3::X * 3.0, 1), (Vec3::X * 7.0, 0), (Vec3::X * 15.0, 1)] {
        assert_eq!(imported_meshes.iter().filter(|m| m.contains(p)).count(), want);
    }
}

fn joined_shells(shells: Vec<mt::Shell>) -> Body {
    guard("invalid ownership fixture", || Body::new(Solid::try_new(shells).unwrap())).unwrap()
}

#[test]
fn unowned_overlapping_and_touching_cavities_are_errors_not_dropped_geometry() {
    let outer = cube(50.0);
    for (min, max) in [
        (Vec3::new(60.0, 60.0, 60.0), Vec3::new(70.0, 70.0, 70.0)),
        (Vec3::new(45.0, -5.0, -5.0), Vec3::new(55.0, 5.0, 5.0)),
        (Vec3::new(40.0, -5.0, -5.0), Vec3::new(50.0, 5.0, 5.0)),
    ] {
        let mut inner = box_solid(min, max).unwrap().deep_copy();
        inner.not();
        let body = joined_shells(outer.deep_copy().boundaries().iter().chain(inner.boundaries()).cloned().collect());
        let before = step_export(&[&body], "invalid ownership").unwrap();
        assert!(body.lumps().is_err());
        assert_eq!(step_export(&[&body], "invalid ownership").unwrap(), before);
    }
    let mut a = box_solid(Vec3::new(-10.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 10.0)).unwrap().deep_copy();
    a.not();
    let mut b = box_solid(Vec3::new(0.0, -10.0, -10.0), Vec3::new(20.0, 10.0, 10.0)).unwrap().deep_copy();
    b.not();
    let body = joined_shells(outer.deep_copy().boundaries().iter().chain(a.boundaries()).chain(b.boundaries()).cloned().collect());
    assert!(body.lumps().is_err());
}

#[test]
fn shell_grouping_preserves_large_disconnected_patterns_and_bounds_hostile_work() {
    let mesh = cube(1.0).tessellate_with(0.01, true).unwrap();
    let copies: Vec<_> = (0..512)
        .map(|i| {
            let mut m = mesh.clone();
            for p in &mut m.positions {
                p.x += f64::from(i) * 4.0;
            }
            m
        })
        .collect();
    assert_eq!(shell_groups::groups(&copies, &vec![8.0; 512], 1e-8).unwrap().len(), 512);
    let excessive = vec![Mesh::default(); shell_groups::MAX_SHELLS + 1];
    assert!(shell_groups::groups(&excessive, &vec![1.0; excessive.len()], 1e-8).unwrap_err().to_string().contains("shell count"));
    let mut oversized = mesh.clone();
    oversized.positions.resize(shell_groups::MAX_POINTS + 1, Vec3::ZERO);
    assert!(shell_groups::groups(&[oversized], &[8.0], 1e-8).unwrap_err().to_string().contains("grouping limit"));
    let mut repeated = mesh.clone();
    repeated.triangles = mesh.triangles.iter().copied().cycle().take(7000).collect();
    assert!(shell_groups::groups(&[repeated.clone(), repeated], &[8.0, 8.0], 1e-8).unwrap_err().to_string().contains("boundary work"));
    let mut outer = cube(2.0).tessellate_with(0.01, true).unwrap();
    let mut inner = mesh;
    outer.triangles = outer.triangles.iter().copied().cycle().take(4000).collect();
    inner.triangles = inner.triangles.iter().copied().cycle().take(4000).collect();
    inner.positions.resize(4000, Vec3::ZERO);
    assert!(shell_groups::groups(&[outer, inner], &[64.0, -8.0], 1e-8).unwrap_err().to_string().contains("containment work"));
}

#[test]
fn translated_and_scaled_nested_voids_keep_material_membership() {
    let body = nested_island();
    for scale in [0.01, 0.25, 2.0] {
        let origin = Vec3::new(1234.0, -987.0, 765.0);
        let transformed = crate::transform_matrix(
            &body,
            [[scale, 0.0, 0.0, 0.0], [0.0, scale, 0.0, 0.0], [0.0, 0.0, scale, 0.0], [origin.x, origin.y, origin.z, 1.0]],
        )
        .unwrap();
        let lumps = transformed
            .lumps()
            .unwrap_or_else(|e| panic!("scale={scale}: {e}; bounds={:?}", transformed.tessellate_with(1e-4, true).unwrap().bounds()));
        assert_eq!(lumps.len(), 2);
        let meshes: Vec<_> = lumps.iter().map(|b| b.tessellate_with((scale * 0.01).max(1e-5), true).unwrap()).collect();
        for (x, want) in [(0.0, 0), (15.0, 1), (30.0, 0), (45.0, 1)] {
            assert_eq!(meshes.iter().filter(|m| m.contains(origin + Vec3::X * (x * scale))).count(), want);
        }
    }
}

#[test]
fn shortcut_clearance_refuses_curved_contact_hidden_between_mesh_vertices() {
    let direction = Vec3::new(1.0, 2.0, 3.0).normalized().unwrap();
    let a = sphere(Vec3::ZERO, 10.0).unwrap();
    for distance in [19.999, 20.001] {
        let b = sphere(direction * distance, 10.0).unwrap();
        for shells in [
            a.deep_copy().boundaries().iter().chain(b.deep_copy().boundaries()).cloned().collect(),
            b.deep_copy().boundaries().iter().chain(a.deep_copy().boundaries()).cloned().collect(),
        ] {
            assert!(joined_shells(shells).separated_lumps().is_err(), "distance {distance}");
        }
    }
    let far = sphere(direction * 21.0, 10.0).unwrap();
    let safe = crate::ops::join_pieces(vec![a, far]).unwrap().unwrap();
    assert_eq!(safe.separated_lumps().unwrap().len(), 2);
    let outer = sphere(Vec3::ZERO, 10.0).unwrap();
    let mut protruding = sphere(direction * 9.001, 1.0).unwrap().deep_copy();
    protruding.not();
    let invalid = joined_shells(outer.deep_copy().boundaries().iter().chain(protruding.boundaries()).cloned().collect());
    assert!(invalid.separated_lumps().is_err());
}
