//! SolidCraft: the broad phase of a set operation. Of every pair of faces (one from each shell),
//! only those whose meshes' bounding boxes overlap are intersected.
//!
//! A face pair's intersection is found from its meshes' colliding triangles, and two triangles
//! are tested only where their boxes overlap. A triangle's box lies inside its face mesh's box,
//! so faces whose boxes miss each other have no colliding triangles and no intersection: leaving
//! them out changes nothing. The boxes are grown by a margin all the same. A face with no mesh,
//! an empty one or a coordinate that is not finite pairs with every face, as before.

use super::integrate::MeshedShell;
use truck_base::cgmath64::*;

/// An axis-aligned box: `[min, max]` per axis.
#[derive(Clone, Copy, Debug)]
pub(super) struct Aabb([[f64; 2]; 3]);

impl Aabb {
    /// The box of `points` grown by `margin`; `None` when there are no points or one is not
    /// finite (the face then pairs with every face).
    fn of(points: &[Point3], margin: f64) -> Option<Aabb> {
        let mut b = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
        for p in points {
            for (k, axis) in b.iter_mut().enumerate() {
                let x = p[k];
                if !x.is_finite() {
                    return None;
                }
                axis[0] = axis[0].min(x);
                axis[1] = axis[1].max(x);
            }
        }
        if points.is_empty() {
            return None;
        }
        Some(Aabb(b.map(|[lo, hi]| [lo - margin, hi + margin])))
    }

    /// Whether the boxes overlap or touch.
    fn meets(&self, other: &Aabb) -> bool {
        self.0.iter().zip(&other.0).all(|(a, b)| a[0] <= b[1] && b[0] <= a[1])
    }
}

/// The boxes of a meshed shell's faces, in face order, each grown by `margin`.
pub(super) fn face_boxes(shell: &MeshedShell, margin: f64) -> Vec<Option<Aabb>> {
    let margin = if margin.is_finite() { margin.max(0.0) } else { 0.0 };
    shell
        .face_iter()
        .map(|face| face.surface().and_then(|mesh| Aabb::of(mesh.positions(), margin)))
        .collect()
}

/// The pairs `(i, j)` of a face `i` of the first shell and a face `j` of the second whose boxes
/// overlap (a face without a box pairs with every face), in the order the brute-force loop
/// visits them: by `i`, then by `j`. Sort and sweep along x: the boxes are visited by their low
/// x, and each pairs with the other shell's boxes that are still open there and meet it.
pub(super) fn overlapping_pairs(boxes0: &[Option<Aabb>], boxes1: &[Option<Aabb>]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut events: Vec<(f64, usize, usize)> = Vec::with_capacity(boxes0.len() + boxes1.len());
    for (side, boxes) in [boxes0, boxes1].into_iter().enumerate() {
        for (i, b) in boxes.iter().enumerate() {
            match b {
                Some(b) => events.push((b.0[0][0], side, i)),
                None if side == 0 => pairs.extend((0..boxes1.len()).map(|j| (i, j))),
                None => pairs.extend((0..boxes0.len()).map(|i0| (i0, i))),
            }
        }
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut open: [Vec<(usize, Aabb)>; 2] = [Vec::new(), Vec::new()];
    for (low, side, i) in events {
        let Some(Some(mine)) = [boxes0, boxes1][side].get(i).copied() else { continue };
        let others = &mut open[1 - side];
        // Every open box starts at or before `low`; those that end before it are done.
        others.retain(|(_, b)| b.0[0][1] >= low);
        for (j, other) in others.iter() {
            if mine.meets(other) {
                pairs.push(if side == 0 { (i, *j) } else { (*j, i) });
            }
        }
        open[side].push((i, mine));
    }
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// The face pairs of two meshed shells whose boxes, grown by `tol`, overlap
/// ([`overlapping_pairs`]).
pub(super) fn face_pairs(shell0: &MeshedShell, shell1: &MeshedShell, tol: f64) -> Vec<(usize, usize)> {
    overlapping_pairs(&face_boxes(shell0, tol), &face_boxes(shell1, tol))
}

/// Every pair of faces, in the brute-force order (the test hooks compare with it).
pub(super) fn every_pair(len0: usize, len1: usize) -> Vec<(usize, usize)> {
    (0..len0).flat_map(|i| (0..len1).map(move |j| (i, j))).collect()
}
