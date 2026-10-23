//! Plugin commands (docs/PLUGINS.md): list plugins, create plugin features,
//! store plugin document data (e.g. plastic rules) and run plugin analyses.
//! Editing a plugin feature goes through `redefine_feature` like any other
//! feature.
use rcad_document::{
    BodyId, DocError, Document, FeatureId, FeatureKind, Model, PluginInput, PluginParam,
};
use rcad_plugin as rp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Ids of the plugins built into this build (never replaced by installed
/// external plugins).
pub const BUILTIN: &[&str] = &[rcad_plugin_plastics::ID];

/// Register the built-in plugins (idempotent).
pub fn register_builtin() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // The built-in manifest is checked by its own tests; a failure here
        // would be a build defect, reported by `list_plugins` as missing.
        let _ = rp::register(std::sync::Arc::new(rcad_plugin_plastics::Plastics));
    });
}

/// Register the external plugins installed in the default search
/// directories ([`rp::process::search_dirs`]); returns the loaded ids and
/// load errors (as text). Front ends call this once at start.
pub fn load_installed() -> (Vec<String>, Vec<String>) {
    let (ok, errs) = rp::process::load_installed(&rp::process::search_dirs(), BUILTIN);
    (
        ok,
        errs.into_iter()
            .map(|(p, e)| format!("{}: {e}", p.display()))
            .collect(),
    )
}

/// `plugin_feature`: create a feature of a plugin. Parameters not given take
/// their defaults; when the feature's body has a rule (`use_rules`, default
/// true) parameters with a rule template are bound to the rule's
/// parameters instead (Fusion: dialog fields default to rule expressions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginFeatureCmd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub plugin: String,
    pub feature: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, PluginInput>,
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub choices: BTreeMap<String, String>,
    /// Explicit expression bindings (slot -> expression); they take
    /// precedence over values and rule templates.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub expressions: BTreeMap<String, String>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub use_rules: bool,
}

fn yes() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}

/// `set_plugin_data`: replace plugin `plugin`'s document data (validated by
/// the plugin; its effects, such as rule parameters and materials, applied in
/// the same undo step). `null` removes the data (the parameters it defined
/// stay; delete them separately).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetPluginDataCmd {
    pub plugin: String,
    pub data: serde_json::Value,
}

/// `run_plugin_analysis`: run a read-only plugin analysis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginAnalysisCmd {
    pub plugin: String,
    pub analysis: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, PluginInput>,
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub choices: BTreeMap<String, String>,
    /// Chord tolerance (mm) of the tessellation analyses read.
    #[serde(default = "default_chord")]
    pub chord: f64,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub use_rules: bool,
}

fn default_chord() -> f64 {
    0.05
}

/// One installed plugin as listed by `list_plugins`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub manifest: rp::Manifest,
    pub builtin: bool,
}

pub fn list() -> Vec<PluginInfo> {
    rp::plugins()
        .into_iter()
        .map(|p| PluginInfo {
            builtin: BUILTIN.contains(&p.manifest().id.as_str()),
            manifest: p.manifest().clone(),
        })
        .collect()
}

fn find(plugin: &str) -> Result<std::sync::Arc<dyn rp::Plugin>, DocError> {
    rp::find(plugin).ok_or_else(|| {
        DocError::Validation(format!(
            "plugin '{plugin}' is not installed (installed: {})",
            rp::plugins()
                .iter()
                .map(|p| p.manifest().id.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

fn plugin_err(e: rp::PluginError) -> DocError {
    match e {
        rp::PluginError::Unsupported(m) => DocError::ProductLimit(m),
        other => DocError::Validation(other.to_string()),
    }
}

/// The body a feature with these inputs works on (for its rule): the
/// placement face's body, a sketch's face body, or the first body input.
fn feature_body(
    doc: &Document,
    inputs: &BTreeMap<String, PluginInput>,
    model: &Model,
) -> Option<BodyId> {
    for i in inputs.values() {
        if let PluginInput::Face { plane } = i
            && let Some(r) = plane.face()
            && let Some(b) = doc.face_body(&r)
        {
            return Some(b);
        }
    }
    for i in inputs.values() {
        if let PluginInput::Sketch { sketch } = i {
            let ev = model.evaluation();
            if let Some(b) = ev
                .features
                .iter()
                .find(|r| r.id == *sketch)
                .and_then(|r| r.resolved_face.as_ref())
                .map(|f| f.body)
            {
                return Some(b);
            }
        }
    }
    inputs.values().find_map(|i| match i {
        PluginInput::Body { body } => Some(*body),
        _ => None,
    })
}

/// Rule symbols of `plugin` for `body` (empty when no rule applies).
fn rule_symbols(
    p: &dyn rp::Plugin,
    doc: &Document,
    plugin: &str,
    body: Option<BodyId>,
) -> BTreeMap<String, String> {
    let data = doc
        .plugin_data
        .get(plugin)
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let sym = p.rule_symbols(&data, body.map(|b| b.0));
    // Only symbols whose parameters exist (a rule whose effects were
    // applied); otherwise the templates would not evaluate.
    sym.into_iter()
        .filter(|(_, n)| doc.parameters.iter().any(|q| &q.name == n))
        .collect()
}

pub fn create_feature(m: &mut Model, c: &PluginFeatureCmd) -> Result<FeatureId, DocError> {
    let p = find(&c.plugin)?;
    let schema = p.manifest().feature(&c.feature).cloned().ok_or_else(|| {
        DocError::Validation(format!(
            "plugin '{}' has no feature '{}' (features: {})",
            c.plugin,
            c.feature,
            p.manifest()
                .features
                .iter()
                .map(|f| f.id.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })?;
    let (values, choices) = rp::schema::complete(
        &schema.id,
        &schema.params,
        &schema.choices,
        &c.params,
        &c.choices,
    )
    .map_err(plugin_err)?;
    for n in c.expressions.keys() {
        if !schema.params.iter().any(|q| &q.name == n) {
            return Err(DocError::UnknownParameter {
                feature: FeatureId(0),
                name: n.clone(),
            });
        }
    }
    let mut expressions = c.expressions.clone();
    if c.use_rules {
        let body = feature_body(m.document(), &c.inputs, m);
        let sym = rule_symbols(p.as_ref(), m.document(), &c.plugin, body);
        if !sym.is_empty() {
            for q in &schema.params {
                if c.params.contains_key(&q.name) || expressions.contains_key(&q.name) {
                    continue;
                }
                if let Some(e) = q.rule.as_deref().and_then(|t| rp::expand_rule(t, &sym)) {
                    expressions.insert(q.name.clone(), e);
                }
            }
        }
    }
    let params: BTreeMap<String, PluginParam> = schema
        .params
        .iter()
        .map(|q| {
            (
                q.name.clone(),
                PluginParam {
                    value: values[&q.name],
                    kind: q.kind,
                },
            )
        })
        .collect();
    let kind = FeatureKind::Plugin {
        plugin: c.plugin.clone(),
        feature: c.feature.clone(),
        plugin_version: p.manifest().version.clone(),
        inputs: c.inputs.clone(),
        params,
        choices,
    };
    let label = schema.name.clone();
    let name = c.name.clone();
    m.transact(&label, |d| {
        let n = name.unwrap_or_else(|| d.default_name(&label));
        let id = d.push(n, kind);
        d.feature_mut(id)?.expressions = expressions;
        Ok(id)
    })
}

pub fn set_data(m: &mut Model, c: &SetPluginDataCmd) -> Result<String, DocError> {
    let p = find(&c.plugin)?;
    let effects = if c.data.is_null() {
        rp::DataEffects::default()
    } else {
        p.validate_data(&c.data).map_err(plugin_err)?;
        p.data_effects(&c.data)
    };
    for q in &effects.parameters {
        rcad_document::params::valid_name(&q.name).map_err(|e| DocError::Expression {
            name: q.name.clone(),
            detail: e,
        })?;
        rcad_document::params::check_syntax(&q.expression).map_err(|e| DocError::Expression {
            name: q.name.clone(),
            detail: format!("'{}': {e}", q.expression),
        })?;
    }
    let mats: Vec<(BodyId, rcad_document::Material)> = effects
        .materials
        .iter()
        .map(|(b, k)| rcad_document::Material::from_catalog(k).map(|mat| (BodyId(*b), mat)))
        .collect::<Result<_, _>>()?;
    let label = format!("Set {} data", p.manifest().name);
    let data = c.data.clone();
    let plugin = c.plugin.clone();
    let n_params = effects.parameters.len();
    m.transact(&label, |d| {
        if data.is_null() {
            d.plugin_data.remove(&plugin);
        } else {
            d.plugin_data.insert(plugin.clone(), data.clone());
        }
        for q in &effects.parameters {
            let unit = match q.kind {
                rp::ParamKind::Length => rcad_document::ParamUnit::Mm,
                rp::ParamKind::Angle => rcad_document::ParamUnit::Deg,
                rp::ParamKind::Count | rp::ParamKind::Ratio => rcad_document::ParamUnit::Unitless,
            };
            match d.parameters.iter_mut().find(|x| x.name == q.name) {
                Some(x) => {
                    x.expression = q.expression.clone();
                    x.unit = unit;
                    x.comment = q.comment.clone();
                }
                None => d.parameters.push(rcad_document::Parameter {
                    name: q.name.clone(),
                    expression: q.expression.clone(),
                    unit,
                    comment: q.comment.clone(),
                }),
            }
        }
        for (b, mat) in &mats {
            d.body_mut(*b)?.material = Some(mat.clone());
        }
        Ok(())
    })?;
    Ok(format!(
        "{} data stored; {n_params} parameter(s), {} material assignment(s)",
        c.plugin,
        mats.len()
    ))
}

/// Exact face kinds and a closed tessellation of `body` for analyses.
fn body_mesh(m: &Model, body: BodyId, chord: f64) -> Result<rp::BodyMesh, DocError> {
    let ev = m.evaluation();
    let b = ev
        .body_by_id(body)
        .ok_or_else(|| DocError::Validation(format!("body {body} does not exist")))?;
    let brep = b.geometry.brep().ok_or_else(|| {
        DocError::ProductLimit(format!("body {body} is a mesh; analyses need a B-rep body"))
    })?;
    let mesh = rcad_kernel::tessellate(brep, chord).map_err(|e| DocError::Kernel {
        feature: FeatureId(0),
        detail: format!("tessellation of {body}: {e}"),
    })?;
    // Exact face kinds where the kernel can classify the body; otherwise
    // (e.g. implicit intersection edges after curved joins) every face is
    // `Other` and analyses work from the tessellation alone.
    let nf = mesh
        .tri_face
        .iter()
        .map(|&f| f as usize + 1)
        .max()
        .unwrap_or(0);
    let kinds = rcad_kernel::classify_faces(brep)
        .ok()
        .filter(|k| k.len() >= nf)
        .unwrap_or_else(|| vec![rcad_kernel::DirectFace::Other; nf]);
    let mut area = vec![0.0; kinds.len()];
    for (t, tri) in mesh.triangles.iter().enumerate() {
        let [a, b, c] = tri.map(|i| mesh.positions[i as usize]);
        let n = rp::geom::cross(rp::geom::sub(b, a), rp::geom::sub(c, a));
        area[mesh.tri_face[t] as usize] += rp::geom::norm(n) / 2.0;
    }
    let faces = kinds
        .iter()
        .zip(area)
        .map(|(k, area)| rp::FaceInfo {
            surface: match *k {
                rcad_kernel::DirectFace::Plane { normal, .. } => rp::Surface::Plane { normal },
                rcad_kernel::DirectFace::Cylinder {
                    origin,
                    axis,
                    radius,
                    hole,
                } => rp::Surface::Cylinder {
                    point: origin,
                    axis,
                    radius,
                    outward: !hole,
                },
                rcad_kernel::DirectFace::Other => rp::Surface::Other,
            },
            area,
        })
        .collect();
    Ok(rp::BodyMesh {
        body: body.0,
        chord: mesh.chord_tolerance,
        faces,
        vertices: mesh.positions,
        triangles: mesh.triangles,
        tri_face: mesh.tri_face,
    })
}

/// Resolve analysis inputs against the current evaluation.
fn analysis_inputs(
    m: &Model,
    schema: &[rp::InputSchema],
    inputs: &BTreeMap<String, PluginInput>,
) -> Result<BTreeMap<String, rp::Resolved>, DocError> {
    let ev = m.evaluation();
    let mut out = BTreeMap::new();
    for s in schema {
        let Some(i) = inputs.get(&s.name) else {
            if s.optional {
                continue;
            }
            return Err(DocError::Validation(format!(
                "input '{}' is required",
                s.name
            )));
        };
        let v = match (s.kind, i) {
            (rp::InputKind::Body, PluginInput::Body { body }) => {
                let b = ev
                    .body_by_id(*body)
                    .ok_or_else(|| DocError::Validation(format!("body {body} does not exist")))?;
                let bb = b
                    .geometry
                    .brep()
                    .and_then(|x| rcad_kernel::bounds(x).ok())
                    .ok_or_else(|| DocError::Validation(format!("body {body} has no bounds")))?;
                rp::Resolved::Body(rp::BodyGeom {
                    body: body.0,
                    bounds: [bb.min, bb.max],
                    faces: vec![],
                })
            }
            (rp::InputKind::Face, PluginInput::Face { plane }) => {
                let r = plane.face().ok_or_else(|| {
                    DocError::Validation(format!("input '{}' must be a face", s.name))
                })?;
                let c = ev.faces.iter().find(|c| c.reference == r).ok_or_else(|| {
                    DocError::BrokenReference {
                        from: FeatureId(0),
                        detail: format!("input '{}': the face does not resolve", s.name),
                    }
                })?;
                let f = c.frame;
                rp::Resolved::Face(rp::FaceGeom {
                    frame: rp::Frame {
                        origin: f.origin,
                        x: f.x,
                        y: f.y,
                        z: f.z,
                    },
                    body: c.body.0,
                    loops: vec![],
                    body_span: [0.0, 0.0],
                    parallel: vec![],
                })
            }
            (rp::InputKind::Points, PluginInput::Points { points }) => rp::Resolved::Points {
                points: points.clone(),
            },
            (k, _) => {
                return Err(DocError::Validation(format!(
                    "input '{}' must be {k:?} (analyses read bodies, faces and points)",
                    s.name
                )));
            }
        };
        out.insert(s.name.clone(), v);
    }
    for n in inputs.keys() {
        if !schema.iter().any(|s| &s.name == n) {
            return Err(DocError::Validation(format!("unknown input '{n}'")));
        }
    }
    Ok(out)
}

pub fn run_analysis(m: &Model, c: &PluginAnalysisCmd) -> Result<serde_json::Value, DocError> {
    let p = find(&c.plugin)?;
    let schema = p.manifest().analysis(&c.analysis).cloned().ok_or_else(|| {
        DocError::Validation(format!(
            "plugin '{}' has no analysis '{}'",
            c.plugin, c.analysis
        ))
    })?;
    if !(c.chord.is_finite() && c.chord >= 1e-4 && c.chord <= 10.0) {
        return Err(DocError::Validation(format!(
            "chord must be within 0.0001..10 mm, got {}",
            c.chord
        )));
    }
    let inputs = analysis_inputs(m, &schema.inputs, &c.inputs)?;
    // Rule values for parameters not given: the templates evaluated now
    // (analyses keep no bindings).
    let mut given = c.params.clone();
    if c.use_rules {
        let body = feature_body(m.document(), &c.inputs, m);
        let sym = rule_symbols(p.as_ref(), m.document(), &c.plugin, body);
        if !sym.is_empty() {
            let env = rcad_document::params::evaluate_parameters(m.document())?;
            for q in &schema.params {
                if given.contains_key(&q.name) {
                    continue;
                }
                if let Some(e) = q.rule.as_deref().and_then(|t| rp::expand_rule(t, &sym)) {
                    let kind = match q.kind {
                        rp::ParamKind::Length => rcad_document::params::SlotKind::Length,
                        rp::ParamKind::Angle => rcad_document::params::SlotKind::Angle,
                        rp::ParamKind::Count => rcad_document::params::SlotKind::Count,
                        rp::ParamKind::Ratio => rcad_document::params::SlotKind::Ratio,
                    };
                    let v =
                        rcad_document::params::evaluate_for_slot(&e, &env, kind).map_err(|d| {
                            DocError::Expression {
                                name: q.name.clone(),
                                detail: format!("'{e}': {d}"),
                            }
                        })?;
                    given.insert(q.name.clone(), v);
                }
            }
        }
    }
    let (params, choices) = rp::schema::complete(
        &schema.id,
        &schema.params,
        &schema.choices,
        &given,
        &c.choices,
    )
    .map_err(plugin_err)?;
    let mut bodies = Vec::new();
    for v in inputs.values() {
        if let rp::Resolved::Body(b) = v {
            bodies.push(body_mesh(m, BodyId(b.body), c.chord)?);
        }
    }
    let req = rp::AnalysisRequest {
        analysis: schema.id.clone(),
        params,
        choices,
        inputs,
        data: m
            .document()
            .plugin_data
            .get(&c.plugin)
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        bodies,
    };
    p.analyze(&req).map_err(plugin_err)
}
