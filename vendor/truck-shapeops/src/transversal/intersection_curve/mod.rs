use truck_base::cgmath64::*;
use truck_geometry::prelude::*;
use truck_meshalgo::prelude::*;

/// polyline base ntersection curve with parameter
#[derive(Debug, Clone, derive_more::Deref, derive_more::DerefMut)]
pub struct IntersectionCurveWithParameters<S> {
    #[deref]
    #[deref_mut]
    ic: IntersectionCurve<PolylineCurve<Point3>, S>,
    params0: PolylineCurve<Point2>,
    params1: PolylineCurve<Point2>,
}

impl<S> From<IntersectionCurveWithParameters<S>> for IntersectionCurve<PolylineCurve<Point3>, S> {
    fn from(a: IntersectionCurveWithParameters<S>) -> Self { a.ic }
}

impl<S> IntersectionCurveWithParameters<S>
where S: ParametricSurface3D + SearchNearestParameter<D2, Point = Point3>
{
    /// SolidCraft: construct a projected curve, returning `None` for degenerate input.
    pub fn try_new(
        surface0: S,
        surface1: S,
        poly: PolylineCurve<Point3>,
        tol: f64,
    ) -> Option<Self>
    where
        S: 'static,
    {
        // SolidCraft: a point contact is not a curve. Reject it before indexing the
        // endpoints, and reject collapsed/non-finite segments before normalizing.
        if poly.len() < 2 || poly.windows(2).any(|p| {
            let length2 = (p[1] - p[0]).magnitude2();
            !length2.is_finite() || length2 <= 0.0
        }) {
            return None;
        }
        let mut polyline = PolylineCurve(Vec::new());
        let mut params0 = PolylineCurve(Vec::new());
        let mut params1 = PolylineCurve(Vec::new());
        // SolidCraft: a point's search from no starting point begins with the nearest of a grid
        // of the surface's points, evaluated for every point of the polyline alike (#125): most
        // of what an intersection cost. The grids are evaluated once here and each point's
        // nearest found among them as the search finds it, so every search goes on from where it
        // would have and gives the same answer.
        let grids = (Presearch::of(&surface0), Presearch::of(&surface1));
        let project = |point: Point3, normal: Vector3| {
            let hint0 = grids.0.as_ref().map(|g| g.nearest(point));
            let hint1 = grids.1.as_ref().map(|g| g.nearest(point));
            double_projection(&surface0, hint0, &surface1, hint1, point, normal, 100)
        };
        for p in poly.windows(2) {
            let n = (p[1] - p[0]).normalize();
            let (q, p0, p1) = project(p[0], n)?;
            polyline.push(q);
            params0.push(p0);
            params1.push(p1);
        }
        let (q, p0, p1) = if poly[0].near(&poly[poly.len() - 1]) {
            (polyline[0], params0[0], params1[0])
        } else {
            let n = (poly[poly.len() - 1] - poly[poly.len() - 2]).normalize();
            project(poly[poly.len() - 1], n)?
        };
        polyline.push(q);
        params0.push(p0);
        params1.push(p1);
        Some(Self {
            ic: IntersectionCurve::new_unchecked(
                Box::new(surface0),
                Box::new(surface1),
                polyline,
                tol,
            ),
            params0,
            params1,
        })
    }
}

impl<S> ParametricCurve for IntersectionCurveWithParameters<S>
where S: ParametricSurface3D + SearchNearestParameter<D2, Point = Point3>
{
    type Point = Point3;
    type Vector = Vector3;
    #[inline(always)]
    fn subs(&self, t: f64) -> Point3 { self.ic.subs(t) }
    #[inline(always)]
    fn der(&self, t: f64) -> Vector3 { self.ic.der(t) }
    #[inline(always)]
    fn der2(&self, t: f64) -> Vector3 { self.ic.der2(t) }
    #[inline(always)]
    fn parameter_range(&self) -> ParameterRange { self.ic.parameter_range() }
}

impl<S> BoundedCurve for IntersectionCurveWithParameters<S> where S: ParametricSurface3D + SearchNearestParameter<D2, Point = Point3> {}

impl<S> ParameterDivision1D for IntersectionCurveWithParameters<S>
where S: ParametricSurface3D + SearchNearestParameter<D2, Point = Point3>
{
    type Point = Point3;
    #[inline(always)]
    fn parameter_division(&self, range: (f64, f64), tol: f64) -> (Vec<f64>, Vec<Point3>) {
        self.ic.parameter_division(range, tol)
    }
}

impl<S> Cut for IntersectionCurveWithParameters<S>
where S: ParametricSurface3D + SearchNearestParameter<D2, Point = Point3>
{
    #[inline(always)]
    fn cut(&mut self, t: f64) -> Self {
        Self {
            ic: self.ic.cut(t),
            params0: self.params0.cut(t),
            params1: self.params1.cut(t),
        }
    }
}

impl<S: Clone> Invertible for IntersectionCurveWithParameters<S> {
    fn invert(&mut self) {
        self.ic.invert();
        self.params0.invert();
        self.params1.invert();
    }
}

impl<S> SearchParameter<D1> for IntersectionCurveWithParameters<S>
where S: ParametricSurface3D + SearchNearestParameter<D2, Point = Point3>
{
    type Point = Point3;
    fn search_parameter<H: Into<SPHint1D>>(
        &self,
        point: Point3,
        hint: H,
        trials: usize,
    ) -> Option<f64> {
        self.ic.search_parameter(point, hint, trials)
    }
}

impl<S> SearchNearestParameter<D1> for IntersectionCurveWithParameters<S>
where S: ParametricSurface3D + SearchNearestParameter<D2, Point = Point3>
{
    type Point = Point3;
    fn search_nearest_parameter<H: Into<SPHint1D>>(
        &self,
        point: Point3,
        hint: H,
        trials: usize,
    ) -> Option<f64> {
        self.ic.search_nearest_parameter(point, hint, trials)
    }
}

/// SolidCraft: the grid of a surface's points that truck's search from no starting point
/// (`SearchNearestParameter` with no hint) takes the nearest of first (`algo::surface::presearch`):
/// truck-modeling's B-spline and NURBS surfaces over their parameter ranges in 50 divisions, its
/// revolved curves in 100; a plane is searched in closed form, and any other surface is not
/// known here (`None`).
pub(crate) struct Presearch {
    grid: Vec<((f64, f64), Point3)>,
}

impl Presearch {
    /// The grid of `surface`, evaluated as the presearch evaluates it.
    pub(crate) fn of<S: ParametricSurface3D + 'static>(surface: &S) -> Option<Self> {
        if !caching_enabled() {
            return None;
        }
        let any: &dyn std::any::Any = surface;
        let (((u0, u1), (v0, v1)), division) = match any.downcast_ref::<truck_modeling::Surface>()? {
            truck_modeling::Surface::BSplineSurface(s) => (s.range_tuple(), 50),
            truck_modeling::Surface::NurbsSurface(s) => (s.range_tuple(), 50),
            truck_modeling::Surface::RevolutedCurve(s) => (s.range_tuple(), 100),
            truck_modeling::Surface::Plane(_) => return None,
        };
        let mut grid = Vec::with_capacity((division + 1) * (division + 1));
        for i in 0..=division {
            for j in 0..=division {
                let p = i as f64 / division as f64;
                let q = j as f64 / division as f64;
                let u = u0 * (1.0 - p) + u1 * p;
                let v = v0 * (1.0 - q) + v1 * q;
                grid.push(((u, v), surface.subs(u, v)));
            }
        }
        Some(Presearch { grid })
    }

    /// The grid's parameters nearest `point`, the first of equals in the presearch's order.
    pub(crate) fn nearest(&self, point: Point3) -> (f64, f64) {
        let mut res = (0.0, 0.0);
        let mut min = f64::INFINITY;
        for &(uv, q) in &self.grid {
            let dist = q.distance2(point);
            if dist < min {
                min = dist;
                res = uv;
            }
        }
        res
    }
}

#[cfg(any(test, feature = "test-hooks"))]
thread_local! {
    /// Set by the test hooks: every point's search evaluates its own grid, as before.
    pub(super) static UNCACHED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the grids are evaluated once per polyline (always, but in the test hooks' comparisons).
fn caching_enabled() -> bool {
    #[cfg(any(test, feature = "test-hooks"))]
    {
        !UNCACHED.with(|c| c.get())
    }
    #[cfg(not(any(test, feature = "test-hooks")))]
    {
        true
    }
}

pub fn intersection_curves<S>(
    surface0: S,
    polygon0: &PolygonMesh,
    surface1: S,
    polygon1: &PolygonMesh,
    tol: f64,
) -> Vec<(
    PolylineCurve<Point3>,
    Option<IntersectionCurveWithParameters<S>>,
)>
where
    S: ParametricSurface3D + SearchNearestParameter<D2, Point = Point3> + 'static,
{
    let interferences = polygon0.extract_interference(polygon1);
    let polylines = super::polyline_construction::construct_polylines(&interferences);
    polylines
        .into_iter()
        .map(|polyline| {
            let curve = IntersectionCurveWithParameters::try_new(
                surface0.clone(),
                surface1.clone(),
                polyline.clone(),
                tol,
            );
            (polyline, curve)
        })
        .collect()
}

#[cfg(test)]
mod tests;
