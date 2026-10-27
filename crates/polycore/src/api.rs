//! Machine-readable kernel contract for Polykrom and agents.
//!
//! The contract records who produced a result, how long it took, what it can honestly do and
//! why an operation refused. Geometry remains the existing kernel's [`Body`]; the contract is a
//! stable facade rather than a second geometry implementation.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Aabb3, Mesh, Plane, Region2, Vec3};
use web_time::Instant;

use crate::history::History;
use crate::{Body, BoolOp, KernelError};

/// Version of the typed facade. It changes when the meaning of a request or result changes.
pub const API_VERSION: u32 = 1;

/// Name, version and build identity of a kernel backend. It belongs in every cache key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelId {
    pub name: String,
    pub version: String,
    pub backend: String,
    pub build: String,
    pub api_version: u32,
}

impl KernelId {
    pub fn new(name: &str, version: &str, backend: &str, build: &str) -> Self {
        Self { name: name.into(), version: version.into(), backend: backend.into(), build: build.into(), api_version: API_VERSION }
    }
}

/// Operation families that a backend can attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Primitive,
    Extrude,
    Revolve,
    Sweep,
    Loft,
    Boolean,
    Blend,
    Offset,
    Split,
    Mass,
    Tessellate,
    AnalysisMesh,
    Read,
    Write,
    Check,
}

/// Interchange formats exposed by the contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Step,
    Iges,
    Native,
    UsdOmniSolid,
    UsdMesh,
    ThreeMf,
    Gltf,
    Stl,
    Obj,
    Mjcf,
    Urdf,
    Sdf,
}

/// What one backend honestly guarantees on this build.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    pub operations: BTreeSet<OperationKind>,
    pub read_formats: BTreeSet<Format>,
    pub write_formats: BTreeSet<Format>,
    pub exact_history: bool,
    pub watertight_tessellation: bool,
    pub analysis_mesh: bool,
    pub deterministic_threads: bool,
    pub wasm32: bool,
    pub limits: BTreeMap<String, f64>,
    pub notes: Vec<String>,
}

impl Capabilities {
    pub fn supports(&self, operation: OperationKind) -> bool {
        self.operations.contains(&operation)
    }
}

/// A budgeted execution context. Cancellation is cooperative and checked at operation
/// boundaries and inside backends that support finer progress checks.
#[derive(Clone)]
pub struct Cx {
    model_tolerance: f64,
    deadline: Option<Instant>,
    thread_budget: Option<usize>,
    cancelled: Arc<AtomicBool>,
}

impl std::fmt::Debug for Cx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cx")
            .field("model_tolerance", &self.model_tolerance)
            .field("deadline", &self.deadline)
            .field("thread_budget", &self.thread_budget)
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl Default for Cx {
    fn default() -> Self {
        Self { model_tolerance: 1e-6, deadline: None, thread_budget: None, cancelled: Arc::new(AtomicBool::new(false)) }
    }
}

impl Cx {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_model_tolerance(mut self, tolerance: f64) -> Result<Self, Refusal> {
        if tolerance.is_finite() && tolerance > 0.0 {
            self.model_tolerance = tolerance;
            Ok(self)
        } else {
            Err(Refusal::invalid("cx", "model tolerance must be finite and positive"))
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.deadline = Instant::now().checked_add(timeout);
        self
    }

    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn with_thread_budget(mut self, threads: usize) -> Result<Self, Refusal> {
        if threads == 0 {
            return Err(Refusal::invalid("cx", "thread budget must be at least one"));
        }
        self.thread_budget = Some(threads);
        Ok(self)
    }

    pub fn model_tolerance(&self) -> f64 {
        self.model_tolerance
    }

    pub fn thread_budget(&self) -> Option<usize> {
        self.thread_budget
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn ensure_active(&self, stage: &'static str) -> Result<(), Refusal> {
        if self.is_cancelled() {
            return Err(Refusal::new(RefusalKind::Cancelled, stage, Vec::new(), "operation cancelled"));
        }
        if self.deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(Refusal::new(RefusalKind::OutOfBudget, stage, Vec::new(), "operation deadline reached"));
        }
        Ok(())
    }
}

/// Why an operation produced no result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalKind {
    Unsupported,
    InvalidInput,
    Failed,
    OutOfBudget,
    Cancelled,
    Internal,
}

/// A durable-ish entity named by a refusal. Raw indices are intentionally not the only agent
/// reference; history and geometric selectors are added by the facade as operations migrate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityRef {
    Body,
    Face { index: usize },
    Edge { index: usize },
    Vertex { index: usize },
    Selection { description: String },
}

/// A structured refusal: no caller receives a bare error string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{kind:?} at {stage}: {message}")]
pub struct Refusal {
    pub kind: RefusalKind,
    pub stage: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<EntityRef>,
    pub message: String,
}

impl Refusal {
    pub fn new(kind: RefusalKind, stage: &'static str, entities: Vec<EntityRef>, message: impl Into<String>) -> Self {
        Self { kind, stage: stage.into(), entities, message: message.into() }
    }

    pub fn unsupported(stage: &'static str, message: impl Into<String>) -> Self {
        Self::new(RefusalKind::Unsupported, stage, Vec::new(), message)
    }

    pub fn invalid(stage: &'static str, message: impl Into<String>) -> Self {
        Self::new(RefusalKind::InvalidInput, stage, Vec::new(), message)
    }

    pub fn from_kernel(stage: &'static str, error: KernelError) -> Self {
        let kind = match &error {
            KernelError::Invalid(_) => RefusalKind::InvalidInput,
            KernelError::TooLarge(_) => RefusalKind::Unsupported,
            KernelError::Internal(_) => RefusalKind::Internal,
            KernelError::Failed(_) => RefusalKind::Failed,
        };
        Self::new(kind, stage, Vec::new(), error.to_string())
    }
}

/// Validity of a body, with every problem listed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validity {
    pub valid: bool,
    pub problems: Vec<String>,
}

impl Validity {
    pub fn from_problems(problems: Vec<String>) -> Self {
        Self { valid: problems.is_empty(), problems }
    }
}

/// Evidence attached to every successful result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub backend: KernelId,
    pub elapsed_seconds: f64,
    pub model_tolerance: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub achieved_tolerance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validity: Option<Validity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// A successful result with provenance and evidence.
#[derive(Clone, Debug)]
pub struct Done<T> {
    pub value: T,
    pub history: Option<History>,
    pub evidence: Evidence,
}

pub type Outcome<T> = std::result::Result<Done<T>, Refusal>;

/// Primitive values in millimetres. Torus is about +Z unless its frame support grows later.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Primitive {
    Box { min: Vec3, max: Vec3 },
    Cylinder { base: Vec3, axis: Vec3, radius: f64, height: f64 },
    Sphere { center: Vec3, radius: f64 },
    Torus { center: Vec3, major: f64, minor: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeSpec {
    pub plane: Plane,
    pub regions: Vec<Region2>,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevolveSpec {
    pub plane: Plane,
    pub regions: Vec<Region2>,
    pub axis_origin: solidcraft_geom::Vec2,
    pub axis_dir: solidcraft_geom::Vec2,
    pub angle: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendKind {
    Fillet,
    Chamfer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlendSpec {
    pub kind: BlendKind,
    pub picks: Vec<Vec3>,
    pub size: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OffsetSpec {
    Shell { open: Vec<Vec3>, thickness: f64 },
    Thicken { thickness: f64, symmetric: bool },
    Faces { picks: Vec<Vec3>, distance: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TessellationSpec {
    pub chord_tolerance: f64,
    pub refine: bool,
}

impl Default for TessellationSpec {
    fn default() -> Self {
        Self { chord_tolerance: 0.01, refine: true }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MassProps {
    pub volume: f64,
    pub area: f64,
    pub center_of_mass: Vec3,
    pub bbox: Aabb3,
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub shells: usize,
}

/// The stable operation surface. Methods may be added before Phase 2; the semantics here are
/// already stable: every result has backend identity and evidence, and every failure is atomic.
pub trait Kernel: Send + Sync {
    fn identity(&self) -> &KernelId;
    fn capabilities(&self) -> &Capabilities;
    fn primitive(&self, cx: &Cx, primitive: &Primitive) -> Outcome<Body>;
    fn extrude(&self, cx: &Cx, spec: &ExtrudeSpec) -> Outcome<Vec<Body>>;
    fn revolve(&self, cx: &Cx, spec: &RevolveSpec) -> Outcome<Vec<Body>>;
    fn boolean(&self, cx: &Cx, op: BoolOp, targets: &[Body], tools: &[Body]) -> Outcome<Vec<Body>>;
    fn blend(&self, cx: &Cx, body: &Body, spec: &BlendSpec) -> Outcome<Body>;
    fn offset(&self, cx: &Cx, body: &Body, spec: &OffsetSpec) -> Outcome<Body>;
    fn check(&self, body: &Body) -> Validity;
    fn mass(&self, cx: &Cx, body: &Body) -> Outcome<MassProps>;
    fn tessellate(&self, cx: &Cx, body: &Body, spec: &TessellationSpec) -> Outcome<Mesh>;
    fn analysis_mesh(&self, cx: &Cx, body: &Body) -> Outcome<Mesh>;
    fn read(&self, cx: &Cx, format: Format, bytes: &[u8]) -> Outcome<Vec<Body>>;
    fn write(&self, cx: &Cx, format: Format, bodies: &[Body]) -> Outcome<Vec<u8>>;
}

/// The current Truck-backed implementation.
#[derive(Clone, Debug)]
pub struct TruckKernel {
    id: KernelId,
    capabilities: Capabilities,
}

impl Default for TruckKernel {
    fn default() -> Self {
        let id = KernelId::new("polycore", env!("CARGO_PKG_VERSION"), "truck", option_env!("POLYCORE_BUILD_HASH").unwrap_or("development"));
        let operations = [
            OperationKind::Primitive,
            OperationKind::Extrude,
            OperationKind::Revolve,
            OperationKind::Boolean,
            OperationKind::Blend,
            OperationKind::Offset,
            OperationKind::Mass,
            OperationKind::Tessellate,
            OperationKind::Read,
            OperationKind::Write,
            OperationKind::Check,
        ]
        .into_iter()
        .collect();
        let read_formats = [Format::Step, Format::Iges].into_iter().collect();
        let write_formats = [Format::Step].into_iter().collect();
        let mut limits = BTreeMap::new();
        limits.insert("max_display_triangles".into(), crate::MAX_TRIANGLES as f64);
        let capabilities = Capabilities {
            operations,
            read_formats,
            write_formats,
            exact_history: false,
            watertight_tessellation: false,
            analysis_mesh: false,
            deterministic_threads: false,
            wasm32: true,
            limits,
            notes: vec![
                "legacy Truck bridge; backend routing is explicit".into(),
                "some boundary and curved blends are unsupported".into(),
                "display tessellation is not yet guaranteed watertight".into(),
            ],
        };
        Self { id, capabilities }
    }
}

impl TruckKernel {
    pub fn new() -> Self {
        Self::default()
    }

    fn evidence(&self, cx: &Cx, started: Instant, validity: Option<Validity>, notes: Vec<String>) -> Evidence {
        Evidence {
            backend: self.id.clone(),
            elapsed_seconds: started.elapsed().as_secs_f64(),
            model_tolerance: cx.model_tolerance(),
            achieved_tolerance: None,
            validity,
            notes,
        }
    }

    fn shape_done(&self, cx: &Cx, started: Instant, body: Body, notes: Vec<String>) -> Done<Body> {
        let validity = Validity::from_problems(body.validity());
        let history = body.history().cloned();
        Done { value: body, history, evidence: self.evidence(cx, started, Some(validity), notes) }
    }
}

impl Kernel for TruckKernel {
    fn identity(&self) -> &KernelId {
        &self.id
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn primitive(&self, cx: &Cx, primitive: &Primitive) -> Outcome<Body> {
        cx.ensure_active("primitive")?;
        let started = Instant::now();
        let body = match primitive {
            Primitive::Box { min, max } => crate::box_solid(*min, *max),
            Primitive::Cylinder { base, axis, radius, height } => crate::cylinder(*base, *axis, *radius, *height),
            Primitive::Sphere { center, radius } => crate::sphere(*center, *radius),
            Primitive::Torus { center, major, minor } => crate::torus(*center, *major, *minor),
        }
        .map_err(|error| Refusal::from_kernel("primitive", error))?;
        cx.ensure_active("primitive")?;
        Ok(self.shape_done(cx, started, body, Vec::new()))
    }

    fn extrude(&self, cx: &Cx, spec: &ExtrudeSpec) -> Outcome<Vec<Body>> {
        cx.ensure_active("extrude")?;
        let started = Instant::now();
        let bodies = crate::extrude(&spec.plane, &spec.regions, spec.start, spec.end).map_err(|error| Refusal::from_kernel("extrude", error))?;
        cx.ensure_active("extrude")?;
        let problems: Vec<String> = bodies.iter().flat_map(|body| body.validity()).collect();
        let history = bodies.first().and_then(|body| body.history().cloned());
        Ok(Done { value: bodies, history, evidence: self.evidence(cx, started, Some(Validity::from_problems(problems)), Vec::new()) })
    }

    fn revolve(&self, cx: &Cx, spec: &RevolveSpec) -> Outcome<Vec<Body>> {
        cx.ensure_active("revolve")?;
        let started = Instant::now();
        let bodies = crate::revolve(&spec.plane, &spec.regions, spec.axis_origin, spec.axis_dir, spec.angle)
            .map_err(|error| Refusal::from_kernel("revolve", error))?;
        cx.ensure_active("revolve")?;
        let problems: Vec<String> = bodies.iter().flat_map(|body| body.validity()).collect();
        let history = bodies.first().and_then(|body| body.history().cloned());
        Ok(Done { value: bodies, history, evidence: self.evidence(cx, started, Some(Validity::from_problems(problems)), Vec::new()) })
    }

    fn boolean(&self, cx: &Cx, op: BoolOp, targets: &[Body], tools: &[Body]) -> Outcome<Vec<Body>> {
        cx.ensure_active("boolean")?;
        if targets.is_empty() {
            return Err(Refusal::invalid("boolean", "at least one target body is required"));
        }
        let started = Instant::now();
        let mut current: Vec<Body> = targets.to_vec();
        for tool in tools {
            let mut next = Vec::new();
            let mut touched = false;
            for target in current {
                cx.ensure_active("boolean")?;
                let result = crate::boolean(&target, tool, op).map_err(|error| Refusal::from_kernel("boolean", error))?;
                match (op, result) {
                    (BoolOp::Union, Some(body)) | (BoolOp::Cut, Some(body)) | (BoolOp::Intersect, Some(body)) => {
                        touched = true;
                        next.push(body);
                    }
                    (BoolOp::Union, None) => next.push(target),
                    (BoolOp::Cut | BoolOp::Intersect, None) => {}
                }
            }
            if op == BoolOp::Union && !touched {
                next.push(tool.clone());
            }
            current = next;
        }
        cx.ensure_active("boolean")?;
        let problems: Vec<String> = current.iter().flat_map(|body| body.validity()).collect();
        let history = current.first().and_then(|body| body.history().cloned());
        Ok(Done {
            value: current,
            history,
            evidence: self.evidence(cx, started, Some(Validity::from_problems(problems)), vec![format!("boolean op {op:?}")]),
        })
    }

    fn blend(&self, cx: &Cx, body: &Body, spec: &BlendSpec) -> Outcome<Body> {
        cx.ensure_active("blend")?;
        let started = Instant::now();
        let value = match spec.kind {
            BlendKind::Fillet => crate::fillet(body, &spec.picks, spec.size),
            BlendKind::Chamfer => crate::chamfer(body, &spec.picks, spec.size),
        }
        .map_err(|error| Refusal::from_kernel("blend", error))?;
        cx.ensure_active("blend")?;
        Ok(self.shape_done(cx, started, value, vec![format!("{:?} size {}", spec.kind, spec.size)]))
    }

    fn offset(&self, cx: &Cx, body: &Body, spec: &OffsetSpec) -> Outcome<Body> {
        cx.ensure_active("offset")?;
        let started = Instant::now();
        let value = match spec {
            OffsetSpec::Shell { open, thickness } => crate::shell(body, open, *thickness),
            OffsetSpec::Thicken { thickness, symmetric } => crate::thicken(body, *thickness, *symmetric),
            OffsetSpec::Faces { picks, distance } => crate::offset_faces(body, picks, *distance),
        }
        .map_err(|error| Refusal::from_kernel("offset", error))?;
        cx.ensure_active("offset")?;
        Ok(self.shape_done(cx, started, value, vec!["offset via legacy Truck bridge".into()]))
    }

    fn check(&self, body: &Body) -> Validity {
        Validity::from_problems(body.validity())
    }

    fn mass(&self, cx: &Cx, body: &Body) -> Outcome<MassProps> {
        cx.ensure_active("mass")?;
        let started = Instant::now();
        let measured = crate::measure(body).map_err(|error| Refusal::from_kernel("mass", error))?;
        cx.ensure_active("mass")?;
        let value = MassProps {
            volume: measured.volume,
            area: measured.area,
            center_of_mass: measured.centroid,
            bbox: measured.bbox,
            faces: measured.faces,
            edges: measured.edges,
            vertices: measured.vertices,
            shells: measured.shells,
        };
        Ok(Done {
            value,
            history: None,
            evidence: self.evidence(
                cx,
                started,
                None,
                vec!["mass from the current tessellation-based integrator; exact B-rep integration is Phase 1".into()],
            ),
        })
    }

    fn tessellate(&self, cx: &Cx, body: &Body, spec: &TessellationSpec) -> Outcome<Mesh> {
        cx.ensure_active("tessellate")?;
        let started = Instant::now();
        let mesh = body.tessellate_with(spec.chord_tolerance, spec.refine).map_err(|error| Refusal::from_kernel("tessellate", error))?;
        cx.ensure_active("tessellate")?;
        let notes = vec![if mesh.is_closed() { "mesh reports closed".into() } else { "mesh reports open".into() }];
        Ok(Done { value: mesh, history: None, evidence: self.evidence(cx, started, None, notes) })
    }

    fn analysis_mesh(&self, _cx: &Cx, _body: &Body) -> Outcome<Mesh> {
        Err(Refusal::unsupported("analysis_mesh", "tagged watertight analysis meshes arrive in Phase 1"))
    }

    fn read(&self, cx: &Cx, format: Format, bytes: &[u8]) -> Outcome<Vec<Body>> {
        cx.ensure_active("read")?;
        let started = Instant::now();
        let text = std::str::from_utf8(bytes).map_err(|error| Refusal::invalid("read", format!("input is not UTF-8 text: {error}")))?;
        let (bodies, warnings): (Vec<Body>, Vec<String>) = match format {
            Format::Step => {
                let imported = crate::step_import(text).map_err(|error| Refusal::from_kernel("read.step", error))?;
                (imported.bodies.into_iter().map(|body| body.body).collect(), imported.warnings)
            }
            Format::Iges => {
                let imported = crate::iges_import(text).map_err(|error| Refusal::from_kernel("read.iges", error))?;
                (imported.bodies.into_iter().map(|body| body.body).collect(), imported.warnings)
            }
            other => return Err(Refusal::unsupported("read", format!("reading {other:?} is not implemented by the Truck bridge"))),
        };
        cx.ensure_active("read")?;
        let problems: Vec<String> = bodies.iter().flat_map(|body| body.validity()).collect();
        Ok(Done { value: bodies, history: None, evidence: self.evidence(cx, started, Some(Validity::from_problems(problems)), warnings) })
    }

    fn write(&self, cx: &Cx, format: Format, bodies: &[Body]) -> Outcome<Vec<u8>> {
        cx.ensure_active("write")?;
        if bodies.is_empty() {
            return Err(Refusal::invalid("write", "at least one body is required"));
        }
        if format != Format::Step {
            return Err(Refusal::unsupported("write", format!("writing {format:?} is not implemented by the Truck bridge")));
        }
        let started = Instant::now();
        let refs: Vec<&Body> = bodies.iter().collect();
        let text = crate::step_export(&refs, "PolyCore").map_err(|error| Refusal::from_kernel("write.step", error))?;
        cx.ensure_active("write")?;
        Ok(Done { value: text.into_bytes(), history: None, evidence: self.evidence(cx, started, None, vec!["STEP AP203 output".into()]) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kernel() -> TruckKernel {
        TruckKernel::new()
    }

    fn unit_box() -> Body {
        let k = kernel();
        let cx = Cx::new();
        k.primitive(&cx, &Primitive::Box { min: Vec3::ZERO, max: Vec3::new(20.0, 20.0, 20.0) }).unwrap().value
    }

    #[test]
    fn identity_capabilities_and_cancellation_are_explicit() {
        let k = kernel();
        assert_eq!(k.identity().backend, "truck");
        assert_eq!(k.identity().api_version, API_VERSION);
        assert!(k.capabilities().supports(OperationKind::Boolean));
        assert!(!k.capabilities().supports(OperationKind::AnalysisMesh));
        assert!(!k.capabilities().watertight_tessellation);

        let cx = Cx::new();
        cx.cancel();
        let refusal = k.primitive(&cx, &Primitive::Box { min: Vec3::ZERO, max: Vec3::new(1.0, 1.0, 1.0) }).unwrap_err();
        assert_eq!(refusal.kind, RefusalKind::Cancelled);
        assert_eq!(refusal.stage, "primitive");
    }

    #[test]
    fn primitive_mass_and_evidence_are_consistent() {
        let k = kernel();
        let cx = Cx::new();
        let primitive = k.primitive(&cx, &Primitive::Box { min: Vec3::ZERO, max: Vec3::new(20.0, 30.0, 40.0) }).unwrap();
        assert!(primitive.evidence.validity.as_ref().is_some_and(|validity| validity.valid));
        let measured = k.mass(&cx, &primitive.value).unwrap();
        assert!((measured.value.volume - 24_000.0).abs() < 1e-6);
        assert_eq!(measured.value.faces, 6);
        assert_eq!(&measured.evidence.backend, k.identity());
    }

    #[test]
    fn boolean_folds_multi_operand_inputs() {
        let k = kernel();
        let cx = Cx::new();
        let target = k.primitive(&cx, &Primitive::Box { min: Vec3::ZERO, max: Vec3::new(20.0, 20.0, 20.0) }).unwrap().value;
        let tools = [
            k.primitive(&cx, &Primitive::Box { min: Vec3::new(10.0, 0.0, 0.0), max: Vec3::new(30.0, 20.0, 20.0) }).unwrap().value,
            k.primitive(&cx, &Primitive::Box { min: Vec3::new(-10.0, 0.0, 0.0), max: Vec3::new(5.0, 20.0, 20.0) }).unwrap().value,
        ];
        let cut = k.boolean(&cx, BoolOp::Cut, &[target], &tools).unwrap();
        assert_eq!(cut.value.len(), 1);
        let measured = k.mass(&cx, &cut.value[0]).unwrap();
        assert!((measured.value.volume - 2_000.0).abs() < 1e-4, "{}", measured.value.volume);
    }

    #[test]
    fn tessellation_and_step_round_trip() {
        let k = kernel();
        let cx = Cx::new();
        let body = unit_box();
        let mesh = k.tessellate(&cx, &body, &TessellationSpec::default()).unwrap();
        assert!(!mesh.value.triangles.is_empty() && !mesh.value.positions.is_empty());
        assert!(!k.capabilities().watertight_tessellation);
        let written = k.write(&cx, Format::Step, std::slice::from_ref(&body)).unwrap();
        let read = k.read(&cx, Format::Step, &written.value).unwrap();
        assert_eq!(read.value.len(), 1);
        assert!(read.evidence.validity.as_ref().is_some_and(|validity| validity.valid));
        let measured = k.mass(&cx, &read.value[0]).unwrap();
        assert!((measured.value.volume - 8_000.0).abs() < 1e-3, "{}", measured.value.volume);
    }

    #[test]
    fn unsupported_operations_and_formats_refuse_cleanly() {
        let k = kernel();
        let cx = Cx::new();
        let refusal = k.analysis_mesh(&cx, &unit_box()).unwrap_err();
        assert_eq!(refusal.kind, RefusalKind::Unsupported);
        assert_eq!(refusal.stage, "analysis_mesh");
        let refusal = k.write(&cx, Format::UsdOmniSolid, &[unit_box()]).unwrap_err();
        assert_eq!(refusal.kind, RefusalKind::Unsupported);
        assert!(refusal.message.contains("UsdOmniSolid"));
    }
}
