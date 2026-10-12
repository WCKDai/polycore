//! The history of a prism boolean's result (see [`super::prism_boolean`]), from what its faces
//! were built over: each wall over a section segment whose [`Tag`] says which operand wall it
//! was read beside, each cap over a section region lying on operand caps at its level. Edges
//! follow from the same tags: a wall's edge with a cap is the operand's edge between them (or
//! the meeting of the wall with the other operand's cap); an edge between walls at a section
//! vertex is the operand's edge there (or the meeting of two operands' walls).

use std::collections::{BTreeSet, HashMap};

use solvecraft_geom::{Plane, Region2, Vec2, Vec3};
use truck_modeling as mt;

use super::{Index, Section, Tag, dist_to, inside};
use crate::BoolOp;
use crate::body::{Body, edge_order, from_p3};
use crate::history::{EdgeOrigin, FaceOrigin, FaceOrigin::Face, FaceProv, Side};

/// A cap of an operand: a planar face square to the prism direction.
struct Cap {
    origin: FaceOrigin,
    /// Its level and whether it faces along the direction.
    z: f64,
    up: bool,
    outline: Outline,
}

/// A section's regions with the walls its outline was read beside and its segments' middles,
/// to tell whether two sections overlap.
struct Outline {
    regions: Vec<Region2>,
    walls: BTreeSet<(usize, usize)>,
    middles: Vec<Vec2>,
}

impl Outline {
    fn of(s: &Section) -> Outline {
        let (segs, tags) = s.segments();
        Outline {
            regions: s.regions.clone(),
            walls: tags.iter().filter_map(|t| Some((t.operand, t.wall?))).collect(),
            middles: segs.iter().map(|g| g.point_at(0.5)).collect(),
        }
    }
}

/// What a prism boolean's result is built from: the operands (target, tool), the direction and
/// the plane square to it that sections are drawn in, and for simple prisms each operand's
/// extent along the direction.
pub(super) struct Context<'a> {
    pub op: BoolOp,
    pub operands: [&'a Body; 2],
    pub index: [&'a Index; 2],
    pub plane: Plane,
    pub tol: f64,
    pub heights: Option<[(f64, f64); 2]>,
}

/// How one piece of the result was built.
pub(super) enum Built {
    /// One body of `crate::extrude` of the section's one region between levels `lo` and `hi`.
    Extruded { section: Section, lo: f64, hi: f64 },
    /// Sewn (`sew_connected_sections`) from these faces, the sewing saying which each face of
    /// the piece was built from (`None`: it could not).
    Sewn { specs: Vec<Spec>, faces: Option<Vec<usize>> },
}

/// A face sewn into a stepped prism.
pub(super) enum Spec {
    /// A wall over a section segment (tagged `tag`) from level `z0` up. `next` is the wall over
    /// the next segment of its loop, which it meets at `at`.
    Wall { tag: Tag, z0: f64, next: usize, at: Vec2 },
    /// A cap over `section` at level `z`, facing along the direction when `up`.
    Cap { section: Section, z: f64, up: bool },
}

/// A face of a piece: the wall over a segment with this tag between two levels, or a cap.
#[derive(Clone, Copy)]
enum Kind {
    Wall(Tag, f64, f64),
    Cap(usize),
}

impl Context<'_> {
    /// `body`, made of `pieces` (their shells in order), with its history over the operands
    /// (none where it is not known).
    pub(super) fn attach<B: std::borrow::Borrow<Body>>(&self, body: Body, pieces: &[(B, Built)]) -> Body {
        let pieces: Vec<(&Body, &Built)> = pieces.iter().map(|(b, k)| (b.borrow(), k)).collect();
        let (faces, edges) = self.history(&pieces).unzip();
        crate::record::traced(body, &crate::record::counts(&self.operands), faces, edges.as_ref())
    }

    /// The faces (each a region of operand faces, and whether it is one untouched) and the edges
    /// (by identity) of the pieces, in order; `None` where something is not known.
    pub(super) fn history(&self, pieces: &[(&Body, &Built)]) -> Option<(Vec<FaceProv>, HashMap<mt::EdgeID, Vec<EdgeOrigin>>)> {
        let caps = self.caps()?;
        let mut faces = Vec::new();
        let mut edges = HashMap::new();
        for (body, built) in pieces {
            let (f, e) = match built {
                Built::Extruded { section, lo, hi } => self.extruded(&caps, body, section, *lo, *hi)?,
                Built::Sewn { specs, faces } => self.sewn(&caps, body, specs, faces.as_deref()?)?,
            };
            faces.extend(f);
            edges.extend(e);
        }
        Some((crate::record::regions(faces), edges))
    }

    /// Both operands' caps, their outlines tagged.
    fn caps(&self) -> Option<Vec<Cap>> {
        let d = self.plane.normal();
        let mut caps = Vec::new();
        let (mut work, mut segments) = (2_000_000usize, super::MAX_PLANAR_SEGMENTS * 2);
        for (operand, (body, index)) in self.operands.iter().zip(self.index).enumerate() {
            for (face, f) in body.solid.face_iter().enumerate() {
                let mt::Surface::Plane(p) = f.oriented_surface() else { continue };
                let n = p.normal();
                let n = Vec3::new(n.x, n.y, n.z).normalized()?;
                if n.dot(d).abs() < 1.0 - 1e-9 {
                    continue;
                }
                let section = Section::from_loops(index.loops(f, face, &self.plane, self.tol, &mut work, &mut segments)?)?;
                let (z, up) = (self.plane.height(from_p3(p.origin())), n.dot(d) > 0.0);
                caps.push(Cap { origin: Face { operand, face }, z, up, outline: Outline::of(&section.oriented()) });
            }
        }
        Some(caps)
    }

    /// The operand caps under a result cap over `section` at level `z` (facing along the
    /// direction when `up`): the target's caps there facing the same way, and the tool's facing
    /// the same way (a union or intersection) or the other way (a cut, whose tool's caps bound
    /// what it leaves), wherever they overlap the section.
    fn cap_origins(&self, caps: &[Cap], section: &Section, z: f64, up: bool) -> BTreeSet<FaceOrigin> {
        let outline = Outline::of(section);
        caps.iter()
            .filter(|c| {
                let facing = match c.origin {
                    Face { operand: 1, .. } if self.op == BoolOp::Cut => c.up != up,
                    _ => c.up == up,
                };
                facing && (c.z - z).abs() < self.tol && self.overlap(&outline, &c.outline)
            })
            .map(|c| c.origin)
            .collect()
    }

    /// Do two sections (at one level) overlap? Where their outlines share a wall, or a point of
    /// one's outline, clear of the other's, lies inside it.
    fn overlap(&self, x: &Outline, y: &Outline) -> bool {
        let within = |x: &Outline, y: &Outline| x.middles.iter().any(|m| dist_to(&y.regions, *m) > self.tol && inside(&y.regions, *m));
        !x.walls.is_disjoint(&y.walls) || within(x, y) || within(y, x)
    }

    /// The operand wall a segment was read beside.
    fn wall(t: Tag) -> Option<FaceOrigin> {
        Some(Face { operand: t.operand, face: t.wall? })
    }

    /// The operand's edges (by number) bounding exactly the faces `faces`, and touching vertex
    /// `at` when given.
    fn edges_between(&self, operand: usize, [f, g]: [usize; 2], at: Option<usize>) -> Vec<usize> {
        let Some(index) = self.index.get(operand) else { return Vec::new() };
        let Some(between) = index.between.get(&(f.min(g), f.max(g))) else { return Vec::new() };
        between
            .iter()
            .copied()
            .filter(|e| at.is_none_or(|v| index.bounds.get(*e).is_some_and(|(_, front, back)| *front == Some(v) || *back == Some(v))))
            .collect()
    }

    /// Is the wall over a segment tagged `t` between `lo` and `hi` its operand wall untouched?
    /// The whole cap edge it was read off, the operand's whole height (simple prisms), and a
    /// wall with no other cap edges than its two.
    fn whole(&self, caps: &[Cap], t: Tag, lo: f64, hi: f64) -> bool {
        let (Some(heights), Some(wall), Some(_), Some(_)) = (self.heights, t.wall, t.start, t.end) else { return false };
        let Some((l, h)) = heights.get(t.operand) else { return false };
        let Some(index) = self.index.get(t.operand) else { return false };
        let cap_edges: usize = caps
            .iter()
            .filter_map(|c| match c.origin {
                Face { operand, face } if operand == t.operand => index.between.get(&(face.min(wall), face.max(wall))).map(Vec::len),
                _ => None,
            })
            .sum();
        (lo - l).abs() < self.tol && (hi - h).abs() < self.tol && cap_edges == 2
    }

    /// The origins of the edge between the wall over a segment tagged `t` and a cap lying on the
    /// operand faces `cap`: the operand's edge between the wall and its own cap there (the very
    /// edge the segment was read off, when that is one), else where the wall meets the cap (all of
    /// the faces it lies on).
    fn cap_edge(&self, t: Tag, cap: &BTreeSet<FaceOrigin>) -> Option<Vec<EdgeOrigin>> {
        let (x, w) = (t.operand, t.wall?);
        let own: Vec<usize> =
            cap.iter().filter_map(|o| if let Face { operand, face } = *o { (operand == x).then_some(face) } else { None }).collect();
        if let Some(e) = t.edge
            && self.index.get(x)?.bounds.get(e).is_some_and(|(fs, _, _)| fs.contains(&w) && fs.iter().any(|f| own.contains(f)))
        {
            return Some(vec![EdgeOrigin::Edge { operand: x, edge: e }]);
        }
        let along: Vec<usize> = own.iter().flat_map(|c| self.edges_between(x, [w, *c], None)).collect();
        match along.as_slice() {
            [e] => Some(vec![EdgeOrigin::Edge { operand: x, edge: *e }]),
            [] => Some(vec![EdgeOrigin::meet(Self::wall(t)?, Side::new(cap.iter().copied())?)]),
            _ => None,
        }
    }

    /// The origins of the edge between the walls over two segments where the first (tagged
    /// `t1`) runs into the second (`t2`): the operand's edge there between its walls (at the
    /// operand vertex the segments meet at, else the one edge between them), where two
    /// operands' walls meet, or none within one wall (a segment split there).
    fn junction(&self, t1: Tag, t2: Tag) -> Option<Vec<EdgeOrigin>> {
        let (p, q) = (Self::wall(t1)?, Self::wall(t2)?);
        if t1.operand != t2.operand {
            return Some(vec![EdgeOrigin::meet(p, q)]);
        }
        let (x, w1, w2) = (t1.operand, t1.wall?, t2.wall?);
        match (t1.end, t2.start) {
            // At that vertex: the operand's edge there (a seam, within one wall), else two edges
            // of one wall's cap outline running on through it.
            (Some(v), Some(u)) if v == u => self.one_edge(x, [w1, w2], Some(v)).or_else(|| (w1 == w2).then(Vec::new)),
            _ if w1 == w2 => Some(Vec::new()),
            // Elsewhere (a segment split off there, or read off another cap): only the operand's
            // walls are around the edge, so the operand has its edge between them there, and
            // that is the one edge between them where there is one.
            _ => self.one_edge(x, [w1, w2], None),
        }
    }

    /// The origins of the edge between walls over one segment stacked at a level (tagged `t1`
    /// below, `t2` above): within one wall (none), along the operand's edge between two of its
    /// walls, or where two operands' walls meet.
    fn stacked(&self, t1: Tag, t2: Tag) -> Option<Vec<EdgeOrigin>> {
        let (p, q) = (Self::wall(t1)?, Self::wall(t2)?);
        if p == q {
            return Some(Vec::new());
        }
        if t1.operand != t2.operand {
            return Some(vec![EdgeOrigin::meet(p, q)]);
        }
        self.one_edge(t1.operand, [t1.wall?, t2.wall?], None)
    }

    /// The operand's edge between the faces `faces` (touching vertex `at` when given), when it
    /// has exactly one.
    fn one_edge(&self, operand: usize, faces: [usize; 2], at: Option<usize>) -> Option<Vec<EdgeOrigin>> {
        match self.edges_between(operand, faces, at).as_slice() {
            [edge] => Some(vec![EdgeOrigin::Edge { operand, edge: *edge }]),
            _ => None,
        }
    }

    /// Faces and edges of a piece `crate::extrude` built from a section of one region. Its
    /// history says which faces are walls over which segment (numbered over the region's
    /// loops in order) and which are its caps; the start cap's wires run over the segments in
    /// loop order (as the extrude builds them), so the vertex after each wire edge is where one
    /// segment runs into the next, or splits.
    #[allow(clippy::type_complexity)]
    fn extruded(
        &self,
        caps: &[Cap],
        body: &Body,
        section: &Section,
        lo: f64,
        hi: f64,
    ) -> Option<(Vec<(BTreeSet<FaceOrigin>, bool)>, HashMap<mt::EdgeID, Vec<EdgeOrigin>>)> {
        let h = body.history()?;
        let tags = section.segments().1;
        let tops = [self.cap_origins(caps, section, lo, false), self.cap_origins(caps, section, hi, true)];
        let kinds: Vec<Kind> = h
            .faces
            .iter()
            .map(|f| match f.origins.as_slice() {
                [FaceOrigin::Profile { segment }] => Some(Kind::Wall(*tags.get(*segment)?, lo, hi)),
                [FaceOrigin::Cap { end }] => Some(Kind::Cap(usize::from(*end))),
                _ => None,
            })
            .collect::<Option<_>>()?;
        let faces = kinds
            .iter()
            .map(|k| match *k {
                Kind::Wall(t, lo, hi) => Some((std::iter::once(Self::wall(t)?).collect(), self.whole(caps, t, lo, hi))),
                Kind::Cap(end) => Some(tops.get(end).filter(|o| !o.is_empty())?.clone()).map(|o| (o, false)),
            })
            .collect::<Option<Vec<_>>>()?;
        let (edges, bounds) = edge_order(body.solid.face_iter());
        let position: HashMap<mt::EdgeID, usize> = edges.iter().enumerate().map(|(i, e)| (e.id(), i)).collect();
        // The segment of the wall beside an edge of the start cap.
        let segment_of = |e: &mt::Edge| -> Option<usize> {
            bounds.get(*position.get(&e.id())?)?.iter().find_map(|f| match h.faces.get(*f)?.origins.as_slice() {
                [FaceOrigin::Profile { segment }] => Some(*segment),
                _ => None,
            })
        };
        let start = kinds.iter().position(|k| matches!(k, Kind::Cap(0)))?;
        let mut junctions: HashMap<mt::VertexID, (Tag, Tag)> = HashMap::new();
        for w in body.solid.face_iter().nth(start)?.absolute_boundaries() {
            let es: Vec<mt::Edge> = w.edge_iter().cloned().collect();
            let segs: Vec<usize> = es.iter().map(segment_of).collect::<Option<_>>()?;
            if segs.windows(2).any(|p| p.first() > p.get(1)) {
                return None;
            }
            for (k, e) in es.iter().enumerate() {
                let (s, next) = (*segs.get(k)?, *segs.get((k + 1) % segs.len())?);
                let (t, u) = (*tags.get(s)?, *tags.get(next)?);
                // A segment the extrude split in two runs on through the vertex between.
                let pair = if s != next || k + 1 == segs.len() { (t, u) } else { (Tag { end: None, ..t }, Tag { start: None, ..t }) };
                junctions.insert(e.back().id(), pair);
            }
        }
        let mut origins = HashMap::new();
        for (e, fs) in edges.iter().zip(&bounds) {
            let [f, g] = fs.as_slice() else { return None };
            let o = match (kinds.get(*f)?, kinds.get(*g)?) {
                (Kind::Wall(t, ..), Kind::Cap(end)) | (Kind::Cap(end), Kind::Wall(t, ..)) => self.cap_edge(*t, tops.get(*end)?)?,
                (Kind::Wall(..), Kind::Wall(..)) => {
                    let (t1, t2) = [e.absolute_front().id(), e.absolute_back().id()].iter().find_map(|v| junctions.get(v))?;
                    self.junction(*t1, *t2)?
                }
                _ => return None,
            };
            origins.insert(e.id(), o);
        }
        Some((faces, origins))
    }

    /// Faces and edges of a piece sewn from `specs`: the sewing reports which spec each face was
    /// built from. Walls meeting at a vertex of their loop (when two walls meet at both ends,
    /// the edge's vertex says which); walls over one segment meeting at a level; walls meeting
    /// caps.
    #[allow(clippy::type_complexity)]
    fn sewn(
        &self,
        caps: &[Cap],
        body: &Body,
        specs: &[Spec],
        built: &[usize],
    ) -> Option<(Vec<(BTreeSet<FaceOrigin>, bool)>, HashMap<mt::EdgeID, Vec<EdgeOrigin>>)> {
        let cap_of: Vec<Option<BTreeSet<FaceOrigin>>> = specs
            .iter()
            .map(|s| match s {
                Spec::Cap { section, z, up } => Some(self.cap_origins(caps, section, *z, *up)).filter(|o| !o.is_empty()),
                Spec::Wall { .. } => None,
            })
            .collect();
        let faces = built
            .iter()
            .map(|i| match specs.get(*i)? {
                Spec::Wall { tag, .. } => Some((std::iter::once(Self::wall(*tag)?).collect(), false)),
                Spec::Cap { .. } => cap_of.get(*i)?.clone().map(|o| (o, false)),
            })
            .collect::<Option<Vec<_>>>()?;
        let (edges, bounds) = edge_order(body.solid.face_iter());
        let mut origins = HashMap::new();
        for (e, fs) in edges.iter().zip(&bounds) {
            let [f, g] = fs.as_slice() else { return None };
            let (i, j) = (*built.get(*f)?, *built.get(*g)?);
            let o = match (specs.get(i)?, specs.get(j)?) {
                (Spec::Wall { tag, .. }, Spec::Cap { .. }) => self.cap_edge(*tag, cap_of.get(j)?.as_ref()?)?,
                (Spec::Cap { .. }, Spec::Wall { tag, .. }) => self.cap_edge(*tag, cap_of.get(i)?.as_ref()?)?,
                (Spec::Wall { tag: t, z0: a0, next: ni, at: pi, .. }, Spec::Wall { tag: u, z0: b0, next: nj, at: pj, .. }) => {
                    if (a0 - b0).abs() > self.tol {
                        // One over the other: the lower first.
                        if a0 < b0 { self.stacked(*t, *u)? } else { self.stacked(*u, *t)? }
                    } else {
                        // The vertex the edge stands on (in plan) says which way round they meet.
                        let p = self.plane.to_local(from_p3(e.absolute_front().point()));
                        let at = |q: &Vec2| p.dist(*q) < self.tol * 10.0;
                        if *ni == j && at(pi) {
                            self.junction(*t, *u)?
                        } else if *nj == i && at(pj) {
                            self.junction(*u, *t)?
                        } else {
                            return None;
                        }
                    }
                }
                _ => return None,
            };
            origins.insert(e.id(), o);
        }
        Some((faces, origins))
    }
}
