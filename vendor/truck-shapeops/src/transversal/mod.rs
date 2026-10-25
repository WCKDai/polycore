// SolidCraft: which face pairs a set operation intersects.
mod broad_phase;
mod divide_face;
mod faces_classification;
mod history;
mod integrate;
mod intersection_curve;
// SolidCraft: expose the constructor for workspace regression coverage of the vendored fix.
#[doc(hidden)]
pub use intersection_curve::IntersectionCurveWithParameters;
mod loops_store;
mod polyline_construction;
// SolidCraft: the brute-force boolean, for the kernel's equivalence tests.
#[cfg(feature = "test-hooks")]
#[doc(hidden)]
pub use integrate::test_hooks as broad_phase_test_hooks;
pub use history::{EdgeOrigin, FaceIndex, FaceOrigin, History};
pub use integrate::{and, and_with_history, and_with_history_meshed, meshes, or, or_with_history, MeshedShell, ShapeOpsCurve, ShapeOpsSurface};
