use super::history::{History, Lineage};
use super::*;
use truck_geometry::prelude::IntersectionCurve;
use truck_meshalgo::prelude::*;
use truck_topology::*;

/// Only solids consisting of faces whose surface is implemented this trait can be used for set operations.
pub trait ShapeOpsSurface:
    ParametricSurface3D
    + ParameterDivision2D
    + SearchParameter<D2, Point = Point3>
    + SearchNearestParameter<D2, Point = Point3>
    + Invertible
    + Send
    + Sync {
}
impl<S> ShapeOpsSurface for S where S: ParametricSurface3D
        + ParameterDivision2D
        + SearchParameter<D2, Point = Point3>
        + SearchNearestParameter<D2, Point = Point3>
        + Invertible
        + Send
        + Sync
{
}

/// Only solids consisting of edges whose curve is implemented this trait can be used for set operations.
pub trait ShapeOpsCurve<S: ShapeOpsSurface>:
    ParametricCurve3D
    + TrySubs
    + ParameterDivision1D<Point = Point3>
    + Cut
    + Invertible
    + From<IntersectionCurve<PolylineCurve<Point3>, S>>
    + SearchParameter<D1, Point = Point3>
    + SearchNearestParameter<D1, Point = Point3>
    + Send
    + Sync {
}
impl<C, S: ShapeOpsSurface> ShapeOpsCurve<S> for C where C: ParametricCurve3D
        + TrySubs
        + ParameterDivision1D<Point = Point3>
        + Cut
        + Invertible
        + From<IntersectionCurve<PolylineCurve<Point3>, S>>
        + SearchParameter<D1, Point = Point3>
        + SearchNearestParameter<D1, Point = Point3>
        + Send
        + Sync
{
}

/// SolidCraft: the meshed form of a shell that a set operation works on (see [`meshes`]).
pub type MeshedShell = Shell<Point3, PolylineCurve<Point3>, Option<PolygonMesh>>;

/// SolidCraft: the meshes of `solid`'s shells at `tol`, as a set operation makes them: an
/// operand used in many operations at one tolerance (a large body cut down to many small
/// regions) is meshed once and handed to [`and_with_history_meshed`], which is most of what
/// such an operation costs. They fit any copy of `solid` with its faces in the same order.
pub fn meshes<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(solid: &Solid<Point3, C, S>, tol: f64) -> Vec<MeshedShell> {
    solid.boundaries().iter().map(|shell| shell.robust_triangulation(tol)).collect()
}

/// SolidCraft: whether `shell` faces inward (a solid's void, a cut's inverted tool), by the sign
/// of its volume with each face counted as it is oriented (a face's mesh is its surface's,
/// whatever the face's orientation).
fn faces_inward(shell: &MeshedShell) -> Option<bool> {
    let mut volume = 0.0;
    for face in shell.face_iter() {
        let v = face.surface()?.volume();
        volume += if face.orientation() { v } else { -v };
    }
    Some(volume < 0.0)
}

/// SolidCraft: whether `pt` is inside the closed `shell`, by the faces a ray from it crosses.
/// For an AND (`and`) each face counts as it is oriented, and a shell facing inward (`inward`,
/// see [`faces_inward`]) holds what lies outside it: the crossings count 0 there and -1 inside.
/// An OR keeps truck's count, by the faces' surfaces alone: it folds a solid's voids in as if
/// they were solids of their own, which is no union with voids whichever way they are counted.
fn inside_shell(shell: &MeshedShell, pt: Point3, and: bool, inward: bool) -> Option<bool> {
    let dir = hash::take_one_unit(pt);
    let mut count = 0;
    for face in shell.face_iter() {
        let sign = if face.orientation() || !and { 1 } else { -1 };
        count += sign * face.surface()?.signed_crossing_faces(pt, dir);
    }
    Some(if inward { count >= 0 } else { count >= 1 })
}

fn process_one_pair_of_shells<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
    shell0: &Shell<Point3, C, S>,
    meshed0: Option<&MeshedShell>,
    shell1: &Shell<Point3, C, S>,
    tol: f64,
    lineage: &mut Lineage<C, S>,
    and: bool,
    every_pair: bool,
) -> Option<[Shell<Point3, C, S>; 2]> {
    nonpositive_tolerance!(tol);
    // SolidCraft: robust meshing (boundaries from earlier operations lie on surfaces only
    // within tolerance). A mesh made before is copied: the loops stores cut its edges.
    let poly_shell0 = match meshed0 {
        Some(m) if m.len() == shell0.len() => m.mapped(|p| *p, |c| c.clone(), |s| s.clone()),
        _ => shell0.robust_triangulation(tol),
    };
    let poly_shell1 = shell1.robust_triangulation(tol);
    // SolidCraft: only the face pairs whose meshes' boxes overlap can meet (the broad phase).
    // `every_pair` intersects them all, as truck did, for the test hooks to compare with.
    let pairs = match every_pair {
        true => broad_phase::every_pair(poly_shell0.len(), poly_shell1.len()),
        false => broad_phase::face_pairs(&poly_shell0, &poly_shell1, tol),
    };
    let loops_store::LoopsStoreQuadruple {
        geom_loops_store0: loops_store0,
        geom_loops_store1: loops_store1,
        ..
    } = crate::tr!(loops_store::create_loops_stores_for(shell0, &poly_shell0, shell1, &poly_shell1, &pairs, tol, lineage), "loops stores");
    let mut cls0 = crate::tr!(divide_face::divide_faces(shell0, &loops_store0, tol, lineage), "divide faces 0");
    cls0.integrate_by_component();
    let mut cls1 = crate::tr!(divide_face::divide_faces(shell1, &loops_store1, tol, lineage), "divide faces 1");
    cls1.integrate_by_component();
    let [mut and0, mut or0, unknown0] = cls0.and_or_unknown();
    let inward1 = and && !unknown0.is_empty() && faces_inward(&poly_shell1)?;
    unknown0.into_iter().try_for_each(|face| {
        // SolidCraft: no panics (wasm can't catch them).
        let pt = face.boundaries().first()?.vertex_iter().next()?.point();
        if inside_shell(&poly_shell1, pt, and, inward1)? {
            and0.push(face);
        } else {
            or0.push(face);
        }
        Some(())
    })?;
    let [mut and1, mut or1, unknown1] = cls1.and_or_unknown();
    let inward0 = and && !unknown1.is_empty() && faces_inward(&poly_shell0)?;
    unknown1.into_iter().try_for_each(|face| {
        // SolidCraft: no panics (wasm can't catch them).
        let pt = face.boundaries().first()?.vertex_iter().next()?.point();
        if inside_shell(&poly_shell0, pt, and, inward0)? {
            and1.push(face);
        } else {
            or1.push(face);
        }
        Some(())
    })?;
    and0.append(&mut and1);
    or0.append(&mut or1);
    Some([and0, or0])
}

/// AND operation between two solids.
pub fn and<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
    solid0: &Solid<Point3, C, S>,
    solid1: &Solid<Point3, C, S>,
    tol: f64,
) -> Option<Solid<Point3, C, S>> {
    boolean(solid0, None, solid1, tol, true, false).map(|(solid, _)| solid)
}

/// OR operation between two solids.
pub fn or<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
    solid0: &Solid<Point3, C, S>,
    solid1: &Solid<Point3, C, S>,
    tol: f64,
) -> Option<Solid<Point3, C, S>> {
    boolean(solid0, None, solid1, tol, false, false).map(|(solid, _)| solid)
}

/// SolidCraft: AND operation between two solids, with where each face and edge of the result
/// came from (operand 0 is `solid0`, operand 1 is `solid1`): the same result as [`and`], and
/// its history, `None` when a face or edge of it was not recorded. The operands must not share
/// faces or edges.
pub fn and_with_history<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
    solid0: &Solid<Point3, C, S>,
    solid1: &Solid<Point3, C, S>,
    tol: f64,
) -> Option<(Solid<Point3, C, S>, Option<History>)> {
    boolean(solid0, None, solid1, tol, true, false)
}

/// SolidCraft: [`and_with_history`] with `solid0` meshed already ([`meshes`] of it, or of a
/// copy of it, at `tol`): the same result.
pub fn and_with_history_meshed<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
    solid0: &Solid<Point3, C, S>,
    meshed0: &[MeshedShell],
    solid1: &Solid<Point3, C, S>,
    tol: f64,
) -> Option<(Solid<Point3, C, S>, Option<History>)> {
    boolean(solid0, Some(meshed0), solid1, tol, true, false)
}

/// SolidCraft: OR operation between two solids, with where each face and edge of the result
/// came from (operand 0 is `solid0`, operand 1 is `solid1`): the same result as [`or`], and
/// its history, `None` when a face or edge of it was not recorded. The operands must not share
/// faces or edges.
pub fn or_with_history<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
    solid0: &Solid<Point3, C, S>,
    solid1: &Solid<Point3, C, S>,
    tol: f64,
) -> Option<(Solid<Point3, C, S>, Option<History>)> {
    boolean(solid0, None, solid1, tol, false, false)
}

/// Combines the operands' first shells, then folds in their other shells one at a time
/// (`and` chooses the AND or the OR shell of each pass). The history is `None` when a face or
/// edge of the result was not recorded; the result stands without it. `every_pair` skips the
/// broad phase (the test hooks only).
fn boolean<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
    solid0: &Solid<Point3, C, S>,
    meshed0: Option<&[MeshedShell]>,
    solid1: &Solid<Point3, C, S>,
    tol: f64,
    and: bool,
    every_pair: bool,
) -> Option<(Solid<Point3, C, S>, Option<History>)> {
    let pick = |[and_shell, or_shell]: [Shell<Point3, C, S>; 2]| if and { and_shell } else { or_shell };
    let mut lineage = Lineage::new([solid0, solid1]);
    let mut iter0 = solid0.boundaries().iter();
    let mut iter1 = solid1.boundaries().iter();
    let shell0 = iter0.next()?;
    let shell1 = iter1.next()?;
    let meshed0 = meshed0.filter(|m| m.len() == solid0.boundaries().len()).and_then(|m| m.first());
    let mut shell = pick(crate::tr!(process_one_pair_of_shells(shell0, meshed0, shell1, tol, &mut lineage, and, every_pair), "first shells"));
    for other in iter0.chain(iter1) {
        shell = pick(crate::tr!(process_one_pair_of_shells(&shell, None, other, tol, &mut lineage, and, every_pair), "another shell"));
    }
    let solid = crate::tr!(Solid::try_new(components_in_order(&shell)).ok(), "result solid");
    let history = lineage.history(&solid);
    Some((solid, history))
}

// SolidCraft: truck's connected components come out in hash order of face addresses, so the
// result's face order changed from run to run. Keep the shell's own order instead.
fn components_in_order<C, S>(shell: &Shell<Point3, C, S>) -> Vec<Shell<Point3, C, S>> {
    let pos: rustc_hash::FxHashMap<_, usize> = shell.face_iter().enumerate().map(|(i, f)| (f.id(), i)).collect();
    let key = |f: &Face<Point3, C, S>| pos.get(&f.id()).copied().unwrap_or(usize::MAX);
    let mut comps: Vec<Vec<Face<Point3, C, S>>> = shell
        .connected_components()
        .into_iter()
        .map(|c| {
            let mut faces: Vec<_> = c.into_iter().collect();
            faces.sort_by_key(|f| key(f));
            faces
        })
        .collect();
    comps.sort_by_key(|c| c.first().map(|f| key(f)).unwrap_or(usize::MAX));
    comps.into_iter().map(Shell::from).collect()
}

/// The boolean with every pair of faces intersected, as before the broad phase: kept to check
/// that the broad phase gives the same results.
///
/// Not part of the API. Built only for tests, or with the `test-hooks` feature, so the
/// SolidCraft kernel tests can run these checks in CI (this crate's own tests do not run there).
#[cfg(any(test, feature = "test-hooks"))]
#[doc(hidden)]
pub mod test_hooks {
    use super::*;

    /// [`and`] (`and` true) or [`or`], with every face pair intersected.
    pub fn every_pair<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
        solid0: &Solid<Point3, C, S>,
        solid1: &Solid<Point3, C, S>,
        tol: f64,
        and: bool,
    ) -> Option<Solid<Point3, C, S>> {
        boolean(solid0, None, solid1, tol, and, true).map(|(solid, _)| solid)
    }

    /// The face pairs the broad phase keeps for faces with these boxes (`[min, max]` per axis;
    /// `None` for a face without one).
    pub fn pairs_of_boxes(boxes0: &[Option<[[f64; 2]; 3]>], boxes1: &[Option<[[f64; 2]; 3]>]) -> Vec<(usize, usize)> {
        let boxed = |b: &[Option<[[f64; 2]; 3]>]| b.iter().map(|x| x.map(broad_phase::Aabb::new)).collect::<Vec<_>>();
        broad_phase::overlapping_pairs(&boxed(boxes0), &boxed(boxes1))
    }

    /// How many face pairs of the solids' first shells the broad phase keeps, and how many
    /// there are, at `tol`.
    pub fn pairs_kept<C: ShapeOpsCurve<S>, S: ShapeOpsSurface>(
        solid0: &Solid<Point3, C, S>,
        solid1: &Solid<Point3, C, S>,
        tol: f64,
    ) -> Option<(usize, usize)> {
        let (shell0, shell1) = (solid0.boundaries().first()?, solid1.boundaries().first()?);
        let (poly0, poly1) = (shell0.robust_triangulation(tol), shell1.robust_triangulation(tol));
        let kept = broad_phase::face_pairs(&poly0, &poly1, tol);
        Some((kept.len(), poly0.len() * poly1.len()))
    }
}

#[cfg(test)]
mod tests;
