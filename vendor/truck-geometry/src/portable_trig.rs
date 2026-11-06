//! SolidCraft: sine, cosine, the angle between two vectors and the rotation about an axis from
//! IEEE 754 arithmetic alone (`pmath`), so that every platform computes the same bits.
//!
//! `f64::sin`, `f64::cos` and `f64::atan2` are the platform's libm. glibc's are correctly rounded
//! for every angle the kernel builds with, but macOS returns `sin(2.0943951023931953)` (2π/3 to
//! the f64, the band of a full revolve) an ULP above the correctly rounded value, and that ULP
//! reached the revolved surfaces: Polykrom's robustness seed 36 built a different body there.
//! The functions here are `pmath`'s, which evaluate in double-double arithmetic and round once;
//! `polycore`'s `portable_trig_tests` prove them correctly rounded for the kernel's angles with
//! exact integer arithmetic. `Matrix4::from_axis_angle` and `Vector3::angle` on the revolve,
//! sweep, rephase and surface-evaluation paths go through [`rotation`] and [`angle`] below
//! instead of cgmath's, which call the libm.

use crate::base::{InnerSpace, Matrix4, Rad, Vector3};
use std::cell::Cell;

pub use pmath::{atan2, sin_cos};

/// One angle's sine and cosine in the memo of [`sin_cos_memo`]: the angle's bits and what
/// [`sin_cos`] returned for them.
#[derive(Clone, Copy)]
struct Memo {
    key: u64,
    s: f64,
    c: f64,
}

/// The memo's sets; each holds two angles, the more recent first.
const MEMO_SETS: usize = 256;

thread_local! {
    // `sin_cos(+0.0)` is `(0.0, 1.0)`, so the empty memo is already correct for key 0.
    static MEMO: [Cell<Memo>; 2 * MEMO_SETS] = const { [const { Cell::new(Memo { key: 0, s: 0.0, c: 1.0 }) }; 2 * MEMO_SETS] };
}

/// [`sin_cos`] through a per-thread memo of the angles seen last (256 sets of two: 12 KB),
/// for the call sites that take the sine and cosine of the same few angles over and over, as
/// a revolved surface does under `algo::surface::presearch` (a grid of 101 angles, each
/// evaluated 101 times in turn, ten thousand times per intersection point: 14 of the 25
/// billion `sin_cos` calls of Polykrom's robustness seeds). The result is `sin_cos`'s bit for
/// bit whatever the memo holds: an entry is only ever what `sin_cos` returned for its key, and
/// the key is the angle's full bit pattern, so `-0.0`, the NaNs and the infinities get their
/// own. The memo is thread-local and so neither shared nor ordered between threads; it changes
/// timing only, never a value.
pub fn sin_cos_memo(angle: f64) -> (f64, f64) {
    let key = angle.to_bits();
    // Fibonacci hashing: the top bits of the product spread the grid's nearby angles.
    let set = 2 * (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - 8)) as usize;
    MEMO.with(|memo| {
        let first = memo[set].get();
        if first.key == key {
            return (first.s, first.c);
        }
        let second = memo[set + 1].get();
        if second.key == key {
            memo[set].set(second);
            memo[set + 1].set(first);
            return (second.s, second.c);
        }
        let (s, c) = sin_cos(angle);
        memo[set + 1].set(first);
        memo[set].set(Memo { key, s, c });
        (s, c)
    })
}

/// The angle between `a` and `b` in `[0, π]`: cgmath's `InnerSpace::angle` (the arc tangent of
/// the cross product's length over the dot product) with [`atan2`] instead of the libm's.
pub fn angle(a: Vector3, b: Vector3) -> Rad<f64> {
    Rad(atan2(a.cross(b).magnitude(), a.dot(b)))
}

/// The rotation by `angle` about the unit vector `axis` through the origin: cgmath's
/// `Matrix4::from_axis_angle` (Rodrigues' formula in the same operation order) with the sine and
/// cosine of [`sin_cos`] instead of the libm's.
pub fn rotation(axis: Vector3, angle: Rad<f64>) -> Matrix4 {
    rotation_from_sin_cos(axis, sin_cos(angle.0))
}

/// [`rotation`] with the sine and cosine from [`sin_cos_memo`]: the same matrix, for the
/// revolved surface's evaluations.
pub fn rotation_memo(axis: Vector3, angle: Rad<f64>) -> Matrix4 {
    rotation_from_sin_cos(axis, sin_cos_memo(angle.0))
}

/// Rodrigues' formula in cgmath's operation order from a given sine and cosine.
#[inline]
pub fn rotation_from_sin_cos(axis: Vector3, (s, c): (f64, f64)) -> Matrix4 {
    let k = 1.0 - c;
    let (x, y, z) = (axis.x, axis.y, axis.z);
    #[rustfmt::skip]
    let m = Matrix4::new(
        k * x * x + c,      k * x * y + s * z,  k * x * z - s * y,  0.0,
        k * x * y - s * z,  k * y * y + c,      k * y * z + s * x,  0.0,
        k * x * z + s * y,  k * y * z - s * x,  k * z * z + c,      0.0,
        0.0,                0.0,                0.0,                1.0,
    );
    m
}
