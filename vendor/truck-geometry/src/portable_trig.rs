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

pub use pmath::{atan2, sin_cos};

/// The angle between `a` and `b` in `[0, π]`: cgmath's `InnerSpace::angle` (the arc tangent of
/// the cross product's length over the dot product) with [`atan2`] instead of the libm's.
pub fn angle(a: Vector3, b: Vector3) -> Rad<f64> {
    Rad(atan2(a.cross(b).magnitude(), a.dot(b)))
}

/// The rotation by `angle` about the unit vector `axis` through the origin: cgmath's
/// `Matrix4::from_axis_angle` (Rodrigues' formula in the same operation order) with the sine and
/// cosine of [`sin_cos`] instead of the libm's.
pub fn rotation(axis: Vector3, angle: Rad<f64>) -> Matrix4 {
    let (s, c) = sin_cos(angle.0);
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
