//! Plugin interface (docs/PLUGINS.md).
//!
//! A plugin adds feature types and analyses without touching the document,
//! command or UI code. It declares them in a [`Manifest`]; the document
//! stores a plugin feature as data (plugin id, feature id, inputs as
//! persistent references, parameters, choices) and on every regeneration
//! resolves the inputs to geometry and asks the plugin to [`Plugin::build`]
//! a list of [`Op`]s. The document executes those through the same checked
//! paths as its own features (exact tool booleans with the result gate of
//! ADR 0005), so a plugin cannot produce unchecked geometry and a failed
//! build leaves the previous valid state, like any other feature.
//!
//! Builds are pure functions of their request (no document access, no
//! callbacks): the same data crosses a process boundary for out-of-process
//! plugins ([`process`]) as for in-process ones, and regeneration stays
//! deterministic.
pub mod geom;
pub mod process;
pub mod registry;
pub mod schema;

pub use geom::{
    BodyGeom, Curve, FaceGeom, Frame, ParallelPlane, PlanarFace, Profile, Resolved, Segment,
    SketchGeom,
};
pub use process::ProcessPlugin;
pub use registry::{find, plugins, register, unregister};

pub use schema::{
    API_VERSION, AnalysisSchema, ChoiceSchema, FeatureSchema, InputKind, InputSchema, Manifest,
    ParamKind, ParamSchema,
};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Errors of plugin builds and plugin management.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PluginError {
    /// The manifest itself is invalid.
    #[error("plugin manifest: {0}")]
    Manifest(String),
    /// No such plugin, feature or analysis.
    #[error("{0}")]
    NotFound(String),
    /// A request input, parameter or choice is invalid.
    #[error("{0}")]
    Input(String),
    /// The inputs are valid but this geometry is outside what the feature
    /// supports (stated, not approximated).
    #[error("{0}")]
    Unsupported(String),
    /// The plugin failed (an external plugin crashed, timed out or answered
    /// nonsense).
    #[error("plugin failure: {0}")]
    Failed(String),
}

/// A build request: the feature's complete parameters and choices
/// (defaults filled, ranges checked by the host), its inputs resolved at
/// this point of the history, and the plugin's document data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildRequest {
    pub feature: String,
    pub params: BTreeMap<String, f64>,
    pub choices: BTreeMap<String, String>,
    pub inputs: BTreeMap<String, Resolved>,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl BuildRequest {
    /// Parameter `n` (complete requests have every declared one).
    pub fn param(&self, n: &str) -> Result<f64, PluginError> {
        self.params
            .get(n)
            .copied()
            .ok_or_else(|| PluginError::Input(format!("{}: missing parameter '{n}'", self.feature)))
    }

    pub fn choice(&self, n: &str) -> Result<&str, PluginError> {
        self.choices
            .get(n)
            .map(String::as_str)
            .ok_or_else(|| PluginError::Input(format!("{}: missing choice '{n}'", self.feature)))
    }

    pub fn face(&self, n: &str) -> Result<&FaceGeom, PluginError> {
        match self.inputs.get(n) {
            Some(Resolved::Face(f)) => Ok(f),
            _ => Err(PluginError::Input(format!(
                "{}: input '{n}' must be a face",
                self.feature
            ))),
        }
    }

    pub fn faces(&self, n: &str) -> Result<&[FaceGeom], PluginError> {
        match self.inputs.get(n) {
            Some(Resolved::Faces { faces }) => Ok(faces),
            _ => Err(PluginError::Input(format!(
                "{}: input '{n}' must be faces",
                self.feature
            ))),
        }
    }

    pub fn body(&self, n: &str) -> Result<&BodyGeom, PluginError> {
        match self.inputs.get(n) {
            Some(Resolved::Body(b)) => Ok(b),
            _ => Err(PluginError::Input(format!(
                "{}: input '{n}' must be a body",
                self.feature
            ))),
        }
    }

    pub fn sketch(&self, n: &str) -> Result<&SketchGeom, PluginError> {
        match self.inputs.get(n) {
            Some(Resolved::Sketch(s)) => Ok(s),
            _ => Err(PluginError::Input(format!(
                "{}: input '{n}' must be a sketch",
                self.feature
            ))),
        }
    }

    pub fn points(&self, n: &str) -> Result<&[[f64; 2]], PluginError> {
        match self.inputs.get(n) {
            Some(Resolved::Points { points }) => Ok(points),
            _ => Err(PluginError::Input(format!(
                "{}: input '{n}' must be points",
                self.feature
            ))),
        }
    }

    /// An optional input, when given.
    pub fn has(&self, n: &str) -> bool {
        self.inputs.contains_key(n)
    }
}

/// Material added or removed by a [`Op::Prism`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Join,
    Cut,
}

/// One geometry operation of a build, applied in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// The region `profile` minus `holes` (plane coordinates of `frame`)
    /// swept along `frame.z` over `[from, to]` (mm; `from < to`), joined to
    /// or cut from the body of input `body` (a Face or Body input, or a
    /// Sketch input on a face of a body). With
    /// `taper_deg` ≠ 0 the walls are drafted: the section shrinks by
    /// `tan(taper)` per mm going from `from` towards `to` (negative grows);
    /// tapered tools take one polygon or circle and no holes.
    ///
    /// `id` names the operation's faces for later features
    /// (`FaceRole::Plugin`): keep it stable across parameter changes (derive
    /// it from what the operation is, e.g. point index × 16 + part), unique
    /// within the build.
    Prism {
        id: u32,
        body: String,
        frame: Frame,
        profile: Profile,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        holes: Vec<Profile>,
        from: f64,
        to: f64,
        #[serde(default, skip_serializing_if = "is_zero")]
        taper_deg: f64,
        mode: Mode,
        /// Shown in errors ("boss 2: outer wall").
        #[serde(default)]
        label: String,
    },
    /// Round (`fillet`) or bevel (`chamfer`) the outer edge loop of a cap
    /// face of an earlier operation `of` of this build (`end`: its end cap,
    /// else its start cap), `size` mm (radius / leg), through the kernel's
    /// exact cap-loop blend (docs/FILLETS.md: the loop's walls must run
    /// straight down to a common depth; the kernel checks its result against
    /// the closed-form removed volume). With `holes`, the cap's inner loops
    /// too. `id` names the operation for errors.
    EdgeLoop {
        id: u32,
        of: u32,
        #[serde(default = "yes")]
        end: bool,
        blend: Blend,
        size: f64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        holes: bool,
        #[serde(default)]
        label: String,
    },
    /// Replace planar face `index` of input `face` (a Face or Faces input,
    /// as resolved for this build) by the plane through `origin` with outward
    /// unit `normal`, keeping the body's topology: the kernel's exact face
    /// replacement (docs/adr/0010: the face is a convex single loop of
    /// straight edges, its neighbours planes, one side edge per vertex; the
    /// result is checked by probes). Used for drafts of existing faces.
    /// The region `profile` (plane coordinates of `frame`, all at `x >= 0`)
    /// revolved `angle_deg` about `frame.y` through `frame.origin`, joined to
    /// or cut from the body of input `body` through the kernel's checked
    /// combine (the exact revolved booleans of the shapeops fork, with an
    /// independent membership check). For rings such as root fillets.
    Revolve {
        id: u32,
        body: String,
        frame: Frame,
        profile: Profile,
        #[serde(default = "full_turn")]
        angle_deg: f64,
        mode: Mode,
        #[serde(default)]
        label: String,
    },
    /// Exact draft of the planar faces in planes `faces` (a point and the
    /// outward unit normal each; faces of the body of input `body`, also ones
    /// made earlier in this build) about the parting plane through `plane_point` with normal
    /// `plane_normal`, by `angle_deg`, leaning in going along `pull`; with
    /// `two_sided` the faces crossing the plane are split and the far side
    /// leans in going the other way. The kernel's polyhedral draft: every
    /// moved vertex is the intersection of its faces' planes (no booleans).
    DraftFaces {
        id: u32,
        body: String,
        faces: Vec<[[f64; 3]; 2]>,
        plane_point: [f64; 3],
        plane_normal: [f64; 3],
        pull: [f64; 3],
        angle_deg: f64,
        #[serde(default)]
        two_sided: bool,
        #[serde(default)]
        label: String,
    },
    /// Exact fillet of radius `radius` of the straight edge where the faces
    /// in planes `a` and `b` (a point and the outward unit normal each) meet,
    /// on the body of input `body`: the kernel's edge fillet (convex or
    /// concave, the body prismatic along the edge; docs/FILLETS.md). Used for
    /// root fillets of ribs, webs and hooks.
    EdgeFillet {
        id: u32,
        body: String,
        a: [[f64; 3]; 2],
        b: [[f64; 3]; 2],
        radius: f64,
        #[serde(default)]
        label: String,
    },
    /// Exact concave root fillet of radius `radius` where the round boss
    /// (cylinder or drafted cone) of `boss_diameter` at its base on the axis through `at` along `axis` meets the
    /// planar face through `at` it stands on, on the body of input `body`.
    /// With no draft, the wall may also be an open run of a cylinder ending
    /// in radial end faces, with material on either side (the walls of a
    /// web along an arc).
    /// Built into the topology by the kernel (a torus band; no boolean),
    /// checked by probes.
    RootRound {
        id: u32,
        body: String,
        at: [f64; 3],
        axis: [f64; 3],
        boss_diameter: f64,
        radius: f64,
        /// Draft of the boss wall (degrees; 0: a cylinder): it narrows by
        /// tan(draft) per mm away from the base.
        #[serde(default)]
        draft_deg: f64,
        #[serde(default)]
        label: String,
    },
    /// Exact countersink (cone of `diameter` at the entry, included angle
    /// `angle_deg`) of the round hole of `hole_diameter` on the axis through
    /// `at` along `axis`, entering from the planar face through `at`
    /// perpendicular to the axis; on the body of input `body`. Built into
    /// the topology by the kernel (no boolean), checked by probes.
    Countersink {
        id: u32,
        body: String,
        at: [f64; 3],
        axis: [f64; 3],
        hole_diameter: f64,
        diameter: f64,
        angle_deg: f64,
        #[serde(default)]
        label: String,
    },
    ReplaceFace {
        id: u32,
        face: String,
        #[serde(default)]
        index: u32,
        origin: [f64; 3],
        normal: [f64; 3],
        #[serde(default)]
        label: String,
    },
}

fn yes() -> bool {
    true
}

fn full_turn() -> f64 {
    360.0
}

/// Edge blend of an [`Op::EdgeLoop`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Blend {
    Fillet,
    Chamfer,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// Geometry of the faces of a body handed to analyses.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "surface", rename_all = "snake_case")]
pub enum Surface {
    /// Plane with outward unit normal.
    Plane { normal: [f64; 3] },
    /// Cylinder about `axis` (unit) through `point`; `outward`: material
    /// inside the radius (a boss), else outside (a hole).
    Cylinder {
        point: [f64; 3],
        axis: [f64; 3],
        radius: f64,
        outward: bool,
    },
    /// Cone about `axis` with half-angle `half_angle` (radians).
    Cone {
        point: [f64; 3],
        axis: [f64; 3],
        half_angle: f64,
    },
    /// Any other surface (analyses use the tessellation normals).
    Other,
}

/// One face of an analysed body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceInfo {
    pub surface: Surface,
    pub area: f64,
}

/// A body handed to an analysis: exact face kinds plus a closed
/// tessellation (a derived approximation within `chord` mm, labelled as such
/// in reports), triangle `t` lying on face `tri_face[t]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyMesh {
    pub body: u32,
    pub chord: f64,
    pub faces: Vec<FaceInfo>,
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub tri_face: Vec<u32>,
}

/// An analysis request: like a build request, plus the analysed bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisRequest {
    pub analysis: String,
    pub params: BTreeMap<String, f64>,
    pub choices: BTreeMap<String, String>,
    pub inputs: BTreeMap<String, Resolved>,
    #[serde(default)]
    pub data: serde_json::Value,
    pub bodies: Vec<BodyMesh>,
}

/// A plugin. Implementations must be deterministic: the same request gives
/// the same answer.
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &Manifest;

    /// The operations of feature `req.feature` for this request.
    fn build(&self, req: &BuildRequest) -> Result<Vec<Op>, PluginError>;

    /// Run analysis `req.analysis`; the report is plugin-defined JSON.
    fn analyze(&self, req: &AnalysisRequest) -> Result<serde_json::Value, PluginError> {
        Err(PluginError::NotFound(format!(
            "{}: no analysis '{}'",
            self.manifest().id,
            req.analysis
        )))
    }

    /// Check the plugin's document data before it is stored (rules etc.).
    fn validate_data(&self, data: &serde_json::Value) -> Result<(), PluginError> {
        let _ = data;
        Ok(())
    }

    /// Document effects of the plugin's data: user parameters to create or
    /// update (e.g. a plastic rule's `ABS_T` thickness) and catalog materials
    /// to assign to bodies. The host applies them in the transaction that
    /// stores the data, so features bound to those parameters follow.
    fn data_effects(&self, data: &serde_json::Value) -> DataEffects {
        let _ = data;
        DataEffects::default()
    }

    /// Rule symbols for a new feature on body `body`: symbol (as used in
    /// [`ParamSchema::rule`] templates, e.g. `T`) → user parameter name
    /// (e.g. `ABS_T`). Empty: no rule applies; parameters take their
    /// defaults as plain values.
    fn rule_symbols(
        &self,
        data: &serde_json::Value,
        body: Option<u32>,
    ) -> BTreeMap<String, String> {
        let _ = (data, body);
        BTreeMap::new()
    }

    /// Rule templates for slots of core features (not this plugin's): core
    /// feature kind (`shell_face`, `fillet_loop`, `extrude`, ...) → slot →
    /// template over rule symbols. The host binds them on request
    /// (`bind_rule_parameters`), like Fusion's rule parameters feeding Shell,
    /// Fillet and Draft.
    fn core_rule_templates(&self, kind: &str) -> BTreeMap<String, String> {
        let _ = kind;
        BTreeMap::new()
    }

    /// [`Plugin::rule_symbols`] for a body that may belong to an assembly
    /// component (rules can be assigned to components, as in Fusion).
    /// Hosts call this; the default ignores the component.
    fn rule_symbols_in(
        &self,
        data: &serde_json::Value,
        at: &RuleContext,
    ) -> BTreeMap<String, String> {
        self.rule_symbols(data, at.body)
    }

    /// [`Plugin::data_effects`] knowing the document's components (to
    /// assign a component rule's material to its bodies). Hosts call this;
    /// the default ignores the components.
    fn data_effects_in(&self, data: &serde_json::Value, doc: &DocContext) -> DataEffects {
        let _ = doc;
        self.data_effects(data)
    }
}

/// Where a new feature's rule is looked up: its body and that body's
/// component (if any).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RuleContext {
    pub body: Option<u32>,
    pub component: Option<u32>,
}

/// Document facts a plugin's data effects may depend on: each component's
/// bodies.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DocContext {
    #[serde(default)]
    pub components: BTreeMap<u32, Vec<u32>>,
}

/// A user parameter a plugin's data defines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataParameter {
    pub name: String,
    pub expression: String,
    pub kind: ParamKind,
    #[serde(default)]
    pub comment: String,
}

/// See [`Plugin::data_effects`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DataEffects {
    #[serde(default)]
    pub parameters: Vec<DataParameter>,
    /// (body id, material catalog key).
    #[serde(default)]
    pub materials: Vec<(u32, String)>,
}

/// Substitute rule symbols in an expression template: every identifier
/// token equal to a symbol becomes its parameter name (`T/2` with
/// `T → ABS_T` gives `ABS_T/2`). None when the template uses a symbol that
/// is not defined (the parameter then takes its plain default).
pub fn expand_rule(template: &str, symbols: &BTreeMap<String, String>) -> Option<String> {
    let mut out = String::new();
    let mut tok = String::new();
    let flush = |tok: &mut String, out: &mut String| -> Option<()> {
        if tok.is_empty() {
            return Some(());
        }
        let starts_alpha = tok
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if starts_alpha {
            // Function names and unit suffixes stay as they are.
            const KEEP: &[&str] = &["min", "max", "abs", "mm", "cm", "m", "in", "deg"];
            match symbols.get(tok.as_str()) {
                Some(p) => out.push_str(p),
                None if KEEP.contains(&tok.as_str()) => out.push_str(tok),
                None => return None,
            }
        } else {
            out.push_str(tok);
        }
        tok.clear();
        Some(())
    };
    for c in template.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            tok.push(c);
        } else {
            flush(&mut tok, &mut out)?;
            out.push(c);
        }
    }
    flush(&mut tok, &mut out)?;
    Some(out)
}

/// The body an operation's `body` input names: a Face or Body input, or a
/// Sketch input lying on a face of a body.
pub fn target_body(req: &BuildRequest, input: &str) -> Option<u32> {
    match req.inputs.get(input)? {
        Resolved::Face(f) => Some(f.body),
        Resolved::Body(b) => Some(b.body),
        Resolved::Sketch(s) => s.body,
        _ => None,
    }
}

/// Check a build answer against the request: op ids unique, bodies named by
/// Face/Body inputs, finite frames and spans, valid profiles. Hosts call this
/// before executing anything (an external plugin's answer is untrusted).
pub fn check_ops(req: &BuildRequest, ops: &[Op]) -> Result<(), PluginError> {
    const MAX_OPS: usize = 4096;
    if ops.len() > MAX_OPS {
        return Err(PluginError::Failed(format!(
            "{}: {} operations (limit {MAX_OPS})",
            req.feature,
            ops.len()
        )));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut prisms = std::collections::BTreeSet::new();
    for op in ops {
        if let Op::DraftFaces {
            id,
            body,
            faces,
            plane_point,
            plane_normal,
            pull,
            angle_deg,
            label,
            ..
        } = op
        {
            let what = format!(
                "{}: {}",
                req.feature,
                if label.is_empty() { "draft" } else { label }
            );
            let bad = |m: &str| Err(PluginError::Failed(format!("{what}: {m}")));
            if !ids.insert(*id) {
                return bad("operation id repeated");
            }
            if target_body(req, body).is_none() {
                return bad(&format!(
                    "'{body}' is not a face, body or sketch-on-face input"
                ));
            }
            if faces.is_empty()
                || !faces.iter().flatten().flatten().all(|v| v.is_finite())
                || faces.iter().any(|f| (geom::norm(f[1]) - 1.0).abs() > 1e-9)
            {
                return bad("needs at least one face plane with a unit normal");
            }
            let ok = plane_point
                .iter()
                .chain(plane_normal)
                .chain(pull)
                .all(|v| v.is_finite())
                && geom::norm(*plane_normal) > 1e-12
                && geom::norm(*pull) > 1e-12
                && angle_deg.is_finite()
                && angle_deg.abs() < 89.0;
            if !ok {
                return bad("needs a finite plane, pull and an angle within ±89°");
            }
            continue;
        }
        if let Op::EdgeFillet {
            id,
            body,
            a,
            b,
            radius,
            label,
        } = op
        {
            let what = format!(
                "{}: {}",
                req.feature,
                if label.is_empty() {
                    "edge fillet"
                } else {
                    label
                }
            );
            let bad = |m: &str| Err(PluginError::Failed(format!("{what}: {m}")));
            if !ids.insert(*id) {
                return bad("operation id repeated");
            }
            if target_body(req, body).is_none() {
                return bad(&format!(
                    "'{body}' is not a face, body or sketch-on-face input"
                ));
            }
            let ok = a.iter().chain(b).flatten().all(|v| v.is_finite())
                && (geom::norm(a[1]) - 1.0).abs() < 1e-9
                && (geom::norm(b[1]) - 1.0).abs() < 1e-9
                && geom::norm(geom::cross(a[1], b[1])) > 1e-9
                && *radius > 0.0;
            if !ok {
                return bad(
                    "needs two finite, non-parallel planes with unit normals and a radius > 0",
                );
            }
            continue;
        }
        if let Op::RootRound {
            id,
            body,
            at,
            axis,
            boss_diameter,
            radius,
            label,
            ..
        } = op
        {
            let what = format!(
                "{}: {}",
                req.feature,
                if label.is_empty() {
                    "root fillet"
                } else {
                    label
                }
            );
            let bad = |m: &str| Err(PluginError::Failed(format!("{what}: {m}")));
            if !ids.insert(*id) {
                return bad("operation id repeated");
            }
            if target_body(req, body).is_none() {
                return bad(&format!(
                    "'{body}' is not a face, body or sketch-on-face input"
                ));
            }
            if !at.iter().chain(axis).all(|v| v.is_finite())
                || geom::norm(*axis) < 1e-12
                || !(*boss_diameter > 0.0 && *radius > 0.0)
            {
                return bad("needs a finite axis and positive diameter and radius");
            }
            continue;
        }
        if let Op::Revolve {
            id,
            body,
            frame,
            profile,
            angle_deg,
            label,
            ..
        } = op
        {
            let what = format!(
                "{}: {}",
                req.feature,
                if label.is_empty() { "revolve" } else { label }
            );
            let bad = |m: &str| Err(PluginError::Failed(format!("{what}: {m}")));
            if !ids.insert(*id) {
                return bad("operation id repeated");
            }
            if target_body(req, body).is_none() {
                return bad(&format!(
                    "'{body}' is not a face, body or sketch-on-face input"
                ));
            }
            if !frame.is_finite()
                || !(angle_deg.is_finite() && *angle_deg > 0.0 && *angle_deg <= 360.0)
            {
                return bad("needs a finite frame and an angle in (0, 360]");
            }
            if let Err(m) = profile.to_sketch() {
                return bad(&m);
            }
            continue;
        }
        if let Op::Countersink {
            id,
            body,
            at,
            axis,
            hole_diameter,
            diameter,
            angle_deg,
            label,
        } = op
        {
            let what = format!(
                "{}: {}",
                req.feature,
                if label.is_empty() {
                    "countersink"
                } else {
                    label
                }
            );
            let bad = |m: &str| Err(PluginError::Failed(format!("{what}: {m}")));
            if !ids.insert(*id) {
                return bad("operation id repeated");
            }
            if target_body(req, body).is_none() {
                return bad(&format!(
                    "'{body}' is not a face, body or sketch-on-face input"
                ));
            }
            if !at.iter().chain(axis).all(|v| v.is_finite())
                || geom::norm(*axis) < 1e-12
                || !(diameter > hole_diameter && *hole_diameter > 0.0)
                || !(*angle_deg > 0.0 && *angle_deg < 180.0)
            {
                return bad(
                    "needs a finite axis, diameter > hole diameter > 0 and an angle in (0, 180)",
                );
            }
            continue;
        }
        if let Op::ReplaceFace {
            id,
            face,
            index,
            origin,
            normal,
            label,
        } = op
        {
            let what = if label.is_empty() {
                format!("{}: operation {id}", req.feature)
            } else {
                format!("{}: {label}", req.feature)
            };
            let bad = |m: &str| Err(PluginError::Failed(format!("{what}: {m}")));
            if !ids.insert(*id) {
                return bad("operation id repeated");
            }
            let n = match req.inputs.get(face) {
                Some(Resolved::Face(_)) => 1,
                Some(Resolved::Faces { faces }) => faces.len(),
                _ => return bad(&format!("'{face}' is not a face input")),
            };
            if *index as usize >= n {
                return bad(&format!("'{face}' has no face {index}"));
            }
            if !origin.iter().chain(normal).all(|v| v.is_finite())
                || (geom::norm(*normal) - 1.0).abs() > 1e-9
            {
                return bad("the plane must be finite with a unit normal");
            }
            continue;
        }
        if let Op::EdgeLoop {
            id,
            of,
            size,
            label,
            ..
        } = op
        {
            let what = if label.is_empty() {
                format!("{}: operation {id}", req.feature)
            } else {
                format!("{}: {label}", req.feature)
            };
            if !ids.insert(*id) {
                return Err(PluginError::Failed(format!(
                    "{what}: operation id repeated"
                )));
            }
            if !prisms.contains(of) {
                return Err(PluginError::Failed(format!(
                    "{what}: blends operation {of}, which is not an earlier prism of this build"
                )));
            }
            if !(size.is_finite() && *size > 0.0) {
                return Err(PluginError::Failed(format!(
                    "{what}: size must be > 0, got {size}"
                )));
            }
            continue;
        }
        let Op::Prism {
            id,
            body,
            frame,
            profile,
            holes,
            from,
            to,
            taper_deg,
            label,
            ..
        } = op
        else {
            continue;
        };
        let what = if label.is_empty() {
            format!("{}: operation {id}", req.feature)
        } else {
            format!("{}: {label}", req.feature)
        };
        let bad = |m: &str| Err(PluginError::Failed(format!("{what}: {m}")));
        if !ids.insert(*id) {
            return bad("operation id repeated");
        }
        prisms.insert(*id);
        if target_body(req, body).is_none() {
            return bad(&format!(
                "'{body}' is not a face or body input, or a sketch on a face of a body"
            ));
        }
        if !frame.is_finite() {
            return bad("frame is not finite");
        }
        let ortho = [
            geom::dot(frame.x, frame.y),
            geom::dot(frame.y, frame.z),
            geom::dot(frame.z, frame.x),
            geom::norm(frame.x) - 1.0,
            geom::norm(frame.y) - 1.0,
            geom::norm(frame.z) - 1.0,
            geom::norm(geom::sub(geom::cross(frame.x, frame.y), frame.z)),
        ];
        if ortho.iter().any(|v| v.abs() > 1e-9) {
            return bad("frame is not right-handed orthonormal");
        }
        if !(from.is_finite() && to.is_finite() && to > from) {
            return bad(&format!(
                "span [{from}, {to}] must be finite and increasing"
            ));
        }
        if !(taper_deg.is_finite() && taper_deg.abs() < 89.0) {
            return bad(&format!(
                "taper {taper_deg}° must be finite and within ±89°"
            ));
        }
        for p in std::iter::once(profile).chain(holes) {
            if let Err(m) = p.to_sketch() {
                return bad(&m);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face() -> Resolved {
        Resolved::Face(FaceGeom {
            frame: Frame::from_normal([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]).unwrap(),
            body: 1,
            loops: vec![],
            body_span: [-10.0, 0.0],
            parallel: vec![],
        })
    }

    fn req() -> BuildRequest {
        BuildRequest {
            feature: "f".into(),
            params: BTreeMap::new(),
            choices: BTreeMap::new(),
            inputs: [("face".to_string(), face())].into(),
            data: serde_json::Value::Null,
        }
    }

    fn prism(id: u32, body: &str, from: f64, to: f64) -> Op {
        let Resolved::Face(f) = face() else {
            unreachable!()
        };
        Op::Prism {
            id,
            body: body.into(),
            frame: f.frame,
            profile: Profile::Circle {
                center: [0.0, 0.0],
                radius: 1.0,
            },
            holes: vec![],
            from,
            to,
            taper_deg: 0.0,
            mode: Mode::Join,
            label: String::new(),
        }
    }

    #[test]
    fn answers_are_checked_before_use() {
        let r = req();
        assert!(check_ops(&r, &[prism(0, "face", 0.0, 1.0)]).is_ok());
        let e = check_ops(
            &r,
            &[prism(0, "face", 0.0, 1.0), prism(0, "face", 0.0, 2.0)],
        );
        assert!(e.unwrap_err().to_string().contains("repeated"));
        assert!(check_ops(&r, &[prism(0, "nope", 0.0, 1.0)]).is_err());
        assert!(check_ops(&r, &[prism(0, "face", 1.0, 1.0)]).is_err());
        assert!(check_ops(&r, &[prism(0, "face", 0.0, f64::NAN)]).is_err());
    }

    #[test]
    fn rule_templates_expand_to_parameter_names() {
        let sym: BTreeMap<String, String> = [
            ("T".to_string(), "ABS_T".to_string()),
            ("DA".into(), "ABS_DA".into()),
        ]
        .into();
        assert_eq!(expand_rule("T/2", &sym).as_deref(), Some("ABS_T/2"));
        assert_eq!(
            expand_rule("max(T*0.8, 1 mm)", &sym).as_deref(),
            Some("max(ABS_T*0.8, 1 mm)")
        );
        assert_eq!(expand_rule("DA", &sym).as_deref(), Some("ABS_DA"));
        assert_eq!(expand_rule("NR*2", &sym), None);
        assert_eq!(expand_rule("2.5", &sym).as_deref(), Some("2.5"));
    }

    #[test]
    fn ops_serialize_with_tags() {
        let j = serde_json::to_value(prism(3, "face", 0.0, 1.0)).unwrap();
        assert_eq!(j["op"], "prism");
        assert_eq!(j["profile"]["shape"], "circle");
        assert_eq!(j["mode"], "join");
        let back: Op = serde_json::from_value(j).unwrap();
        assert_eq!(back, prism(3, "face", 0.0, 1.0));
    }
}
