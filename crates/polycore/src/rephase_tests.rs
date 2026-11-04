//! `rephase_revolved` turns a revolved face's surface so the face sits away from the seam. The
//! turn must be the same bits on every platform: Polykrom's robustness seed 36 built a different
//! body on macOS because that platform's `sin(-2.0943951023931957)` is an ULP off glibc's, which
//! moved a revolved ring's mesh bounds to 25.000000000000007 and the fuzz generator's next box
//! with them. The rotation is now built from arithmetic alone ([`sin_cos`], [`axis_angle`]) of
//! an angle snapped to a grid ([`snap_angle`]), so the platform's `acos` in the angle search does
//! not reach it either.

use super::*;
use crate::revolve;
use polycore_geom::{Loop2, Plane, Region2, Seg2, Vec2};
use std::f64::consts::{PI, TAU};

/// The angle `rephase_revolved` turns by for a face whose middle the search found at `v`.
fn phi_of(v: f64) -> f64 {
    snap_angle(v.rem_euclid(TAU) - PI)
}

/// The columns of a matrix as bit patterns.
fn bits(m: mt::Matrix4) -> [[u64; 4]; 4] {
    let m: [[f64; 4]; 4] = m.into();
    m.map(|col| col.map(f64::to_bits))
}

/// Three search results and the bits they must give everywhere: the angle, its sine and cosine
/// and the rotation about the unit vector (1, 2, 3)/√14. The first is seed 36's angle (the
/// search returned `-2.0943951023931957 + PI`); the others turn the other way about −π/2 and
/// about +π/2. Recorded on Linux; the sine and cosine are within an ULP of glibc's.
const CASES: [(f64, u64, u64, u64, [[u64; 3]; 3]); 3] = [
    (
        -2.0943951023931957 + PI,
        0xc000c15200000000,
        0xbfebb67b58b330ca,
        0xbfdffffe7aca5fd1,
        [
            [0xbfd92490dfbbeb42, 0xbfdeb99fb40672d2, 0x3fe9194d614c22d2],
            [0x3fed13ab10202287, 0xbfb249203a423654, 0x3fda5468a9a0475f],
            [0xbfc21c1240831492, 0x3febfa5d4d8697aa, 0x3fddb6dbf8b7b935],
        ],
    ),
    (
        1.0,
        0xc00121fb80000000,
        0xbfeaed54308486d0,
        0xbfe14a28a2e8dd84,
        [
            [0xbfdb89b92e8be487, 0xbfdd18bac6719094, 0x3fe8f487c9e7d648],
            [0x3feca18d91c4754c, 0xbfb9a7c3a2e98414, 0x3fdbdaece4a3f27c],
            [0xbfbea5fc20b29634, 0x3fec521a19510dc6, 0x3fdccb078ba2cf80],
        ],
    ),
    (
        4.5,
        0x3ff5bc0900000000,
        0x3fef47ed18e71d41,
        0x3fcafb5e005c5056,
        [
            [0x3fd11947127400ba, 0x3fecb03809f2e3cb, 0xbfd69e0d136a854e],
            [0xbfe57907e56732ad, 0x3fdbec0f49458a68, 0x3fe32efd89608d6c],
            [0x3fe621cec086cc55, 0x3fb3b49722142f74, 0x3fe6fb03d251629a],
        ],
    ),
];

/// The turn's angle, sine, cosine and rotation matrix are the recorded bits: a platform whose
/// arithmetic (or a compiler that contracts it into fused multiply-adds) gives anything else
/// fails here rather than in Polykrom's robustness suite.
#[test]
fn rephase_rotation_has_the_same_bits_on_every_platform() {
    let tilted = mt::InnerSpace::normalize(mt::Vector3::new(1.0, 2.0, 3.0));
    for (v, phi_bits, sin_bits, cos_bits, tilted_bits) in CASES {
        let phi = phi_of(v);
        assert_eq!(phi.to_bits(), phi_bits, "angle for v = {v}: {phi:?}");
        let (s, c) = sin_cos(phi);
        assert_eq!((s.to_bits(), c.to_bits()), (sin_bits, cos_bits), "sin, cos of {phi:?}: {s:?} {c:?}");
        // About z the entries are the sine and cosine themselves (the axis has no x or y).
        let z = bits(axis_angle(mt::Vector3::new(0.0, 0.0, 1.0), phi));
        let one = 1.0f64.to_bits();
        assert_eq!(z, [[cos_bits, sin_bits, 0, 0], [(-s).to_bits(), cos_bits, 0, 0], [0, 0, one, 0], [0, 0, 0, one]], "about z, v = {v}");
        let m = bits(axis_angle(tilted, phi));
        for (col, want) in tilted_bits.iter().enumerate() {
            assert_eq!(&m[col][..3], &want[..], "column {col} about (1, 2, 3)/√14, v = {v}: {:x?}", m[col]);
            assert_eq!(m[col][3], 0);
        }
        assert_eq!(m[3], [0, 0, 0, one]);
    }
}

/// The angle is a multiple of 2⁻²⁰ within half a grid step of the search's angle, and the same
/// one when the search's result (the platform's `acos`) is an ULP either way.
#[test]
fn rephase_angle_is_snapped_to_the_grid() {
    for (v, ..) in CASES {
        let phi = phi_of(v);
        let raw = v.rem_euclid(TAU) - PI;
        assert_eq!(phi * 1048576.0, (phi * 1048576.0).round(), "{phi:?} is on the grid");
        assert!((phi - raw).abs() <= 0.5 / 1048576.0, "{phi:?} is the nearest grid point to {raw:?}");
        for bump in [-2i64, -1, 1, 2] {
            let v2 = f64::from_bits((v.to_bits() as i64 + bump) as u64);
            assert_eq!(phi_of(v2).to_bits(), phi.to_bits(), "v off by {bump} ULP: {v2:?}");
        }
    }
    // Exact where the grid is: 0, ±π/2 and ±π to the f64 are not on it, but their snaps are
    // within a step of them and the turn of half a step is exact.
    assert_eq!(snap_angle(0.0), 0.0);
    assert_eq!(snap_angle(1.0 / 1048576.0), 1.0 / 1048576.0);
    assert!(!snap_angle(f64::NAN).is_finite());
}

/// Correctly rounded values where they are known, and within two ULPs of the platform's libm
/// over the whole range `rephase_revolved` uses (a libm is within an ULP of the true value and
/// so is this; a wrong polynomial or reduction is off by far more).
#[test]
fn sin_cos_is_accurate() {
    assert_eq!(sin_cos(0.0), (0.0, 1.0));
    assert_eq!(sin_cos(-0.0).1, 1.0);
    let ulps = |a: f64, b: f64| (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs();
    // Exactly ±1 and a tiny remainder at the quarter turns to the f64, like the platform.
    for (x, want_s, want_c) in [
        (std::f64::consts::FRAC_PI_2, 1.0, 6.123233995736766e-17),
        (-std::f64::consts::FRAC_PI_2, -1.0, 6.123233995736766e-17),
        (PI, 1.2246467991473532e-16, -1.0),
    ] {
        let (s, c) = sin_cos(x);
        assert!(ulps(s, want_s) <= 1 && ulps(c, want_c) <= 1, "sin, cos of {x:?}: {s:?} {c:?}");
    }
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut worst = 0;
    for _ in 0..200_000 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let x = ((state >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) * (PI + 0.01);
        let (s, c) = sin_cos(x);
        let d = ulps(s, x.sin()).max(ulps(c, x.cos()));
        assert!(d <= 2, "sin, cos of {x:?}: {s:?} {c:?} against the platform's {:?} {:?}", x.sin(), x.cos());
        worst = worst.max(d);
    }
    println!("at most {worst} ULP from the platform's sin and cos");
    assert!(worst <= 2, "{worst} ULPs");
}

/// A rectangle in sketch coordinates, as a region.
fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
    let seg = |a: (f64, f64), b: (f64, f64)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) };
    Region2 {
        outer: Loop2 { segs: vec![seg((x0, y0), (x1, y0)), seg((x1, y0), (x1, y1)), seg((x1, y1), (x0, y1)), seg((x0, y1), (x0, y0))] },
        holes: vec![],
    }
}

/// Polykrom's robustness seed 36 starts with a ring of outer radius 25 revolved about z (a
/// rectangle on the front plane). Its faces are turned by `rephase_revolved`, and the mesh
/// Polykrom's fuzz generator reads the bounds of (the reference display mesh at a thousandth of
/// the size) reaches exactly ±25 where it did on Linux before: the generator places its next box
/// on those bounds, so 25.000000000000007 (macOS, before) built a different design.
#[test]
fn revolved_ring_mesh_bounds_are_exact() {
    let rephased_before = REPHASED.with(|n| n.get());
    let body = revolve(&Plane::XZ, &[rect(2.5, 0.0, 25.0, 27.5)], Vec2::ZERO, Vec2::new(0.0, -1.0), TAU).unwrap().pop().unwrap();
    assert!(REPHASED.with(|n| n.get()) > rephased_before, "the ring's faces were searched");
    let tol = (body.size() * 1e-3).max(1e-3);
    let b = body.reference_display_mesh_angular(tol, DISPLAY_ANGLE).unwrap().bounds();
    assert_eq!((b.max.x, b.min.x, b.max.y), (25.0, -25.0, 25.0), "{b:?}");
    assert_eq!(b.min.y, -24.999999999999996, "{b:?}");
    assert_eq!((b.min.z, b.max.z), (-27.5, 0.0), "{b:?}");
    // The turned surfaces themselves, which the booleans read, to the bit. The ring's bands come
    // in thirds (at 0°, 120° and 240°), every band's middle is found at the same angle, and all
    // of them turn by the same snapped −2π/3 (4.2e-7 rad short of it), so their generating
    // lines now stand 4.2e-7 rad past −120°, 0° and 120°, at radius 25 (outer) and 2.5 (inner).
    let mut lines = Vec::new();
    for f in body.solid.boundaries().iter().flat_map(|sh| sh.face_iter()) {
        let mt::Surface::RevolutedCurve(proc_) = f.surface() else { continue };
        let c = proc_.entity().entity_curve().clone();
        let (t0, t1) = mt::BoundedCurve::range_tuple(&c);
        let (p, q) = (mt::ParametricCurve::subs(&c, t0), mt::ParametricCurve::subs(&c, t1));
        assert_eq!((p.x.to_bits(), p.y.to_bits()), (q.x.to_bits(), q.y.to_bits()), "a generating line parallel to the axis: {p:?} {q:?}");
        assert_eq!([p.z, q.z].map(f64::abs).iter().sum::<f64>(), 27.5, "{p:?} {q:?}");
        lines.push([p.x.to_bits(), p.y.to_bits()]);
    }
    lines.sort_unstable();
    assert_eq!(
        lines,
        [
            [0x4003fffffffffe13, 0x3eb18e340fa00000], // (2.5, 0) turned by 4.2e-7
            [0x4038fffffffffd97, 0x3ee5f1c113700000], // (25, 0) turned by 4.2e-7
            [0xbff3ffff0cbe7be3, 0xc001520d176ffe7e], // inner, −120°
            [0xbff40000f341803e, 0x4001520c8afe5e01], // inner, 120°
            [0xc028fffecfee1adb, 0xc035a6905d4bfe1e], // outer, −120°
            [0xc02900013011e04e, 0x4035a68fadbdf582], // outer, 120°
        ],
        "{lines:x?}"
    );
}
