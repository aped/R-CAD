//! The generic plugin analysis window (Design Advice, Draft Analysis,
//! Accessibility, Minimum Radius, and any analysis of any plugin): inputs,
//! parameters and choices from the [`rcad_plugin::AnalysisSchema`], Run
//! sends `RunPluginAnalysis`, and the report is shown as its `summary`
//! values and its `alerts` list (the report layout plugins share, see
//! docs/PLUGINS.md), with a tailored results table for the analyses this
//! window knows ([`tailored`]). Values derived from a tessellation are
//! labelled approximate with the report's own note. Clicking an alert
//! selects the face it names. A report with per-triangle arrays can be
//! shown on the model as a colour map ([`super::colour_map`]). Read-only:
//! nothing here changes the document.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use rcad_commands::Command;
use rcad_commands::plugins::{PluginAnalysisCmd, PluginInfo};
use rcad_document::{BodyId, PluginInput, SketchPlane};
use rcad_plugin::{ChoiceSchema, InputKind, InputSchema};
use serde_json::Value;

use super::colour_map::ColourMap;
use super::feature::{ParamField, param_fields};
use super::{slot_of, words};
use crate::app::RcadApp;
use crate::probe::Probe as _;

/// The open analysis window.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisForm {
    pub plugin: String,
    pub analysis: String,
    /// Display name ("Design Advice"): the window title.
    pub name: String,
    pub description: String,
    pub schema_inputs: Vec<InputSchema>,
    pub inputs: BTreeMap<String, PluginInput>,
    pub params: Vec<ParamField>,
    pub choices: Vec<(ChoiceSchema, String)>,
    /// Waiting for the reply of Run.
    pub pending: bool,
    /// The last report, or why the analysis was refused.
    pub result: Option<Result<Value, String>>,
    /// The analysed body's tessellation for the report (colour map).
    pub mesh: Option<crate::engine::AnalysisMesh>,
    /// Why the colour map of the report is unavailable.
    pub map_error: Option<String>,
}

impl AnalysisForm {
    pub fn new(info: &PluginInfo, analysis: &str) -> Option<Self> {
        let a = info.manifest.analysis(analysis)?;
        Some(Self {
            plugin: info.manifest.id.clone(),
            analysis: a.id.clone(),
            name: a.name.clone(),
            description: a.description.clone(),
            schema_inputs: a.inputs.clone(),
            inputs: BTreeMap::new(),
            params: param_fields(&a.params),
            choices: a
                .choices
                .iter()
                .map(|c| (c.clone(), c.default.clone()))
                .collect(),
            pending: false,
            result: None,
            mesh: None,
            map_error: None,
        })
    }

    /// The Run command. Unchanged parameters are left to the body's rule
    /// (or the declared default), as for features.
    pub fn command(&self) -> Result<Command, String> {
        let mut params = BTreeMap::new();
        for p in self.params.iter().filter(|p| p.touched()) {
            params.insert(p.name.clone(), p.value()?);
        }
        for s in &self.schema_inputs {
            if !s.optional && !self.inputs.contains_key(&s.name) {
                return Err(format!("{}: required", s.label));
            }
        }
        Ok(Command::RunPluginAnalysis(PluginAnalysisCmd {
            plugin: self.plugin.clone(),
            analysis: self.analysis.clone(),
            inputs: self.inputs.clone(),
            params,
            choices: self
                .choices
                .iter()
                .map(|(c, v)| (c.name.clone(), v.clone()))
                .collect(),
            chord: 0.05,
            use_rules: true,
        }))
    }
}

/// Labels and units of the shared report summary keys.
const SUMMARY: &[(&str, &str, &str)] = &[
    ("min_thickness", "Minimum thickness", "mm"),
    ("max_thickness", "Maximum thickness", "mm"),
    ("mean_thickness", "Mean thickness", "mm"),
    ("too_thin_area", "Area too thin", "mm²"),
    ("too_thick_area", "Area too thick", "mm²"),
    ("variation_area", "Area with thickness variation", "mm²"),
    ("draft_area", "Area below minimum draft", "mm²"),
    ("undercut_area", "Undercut area", "mm²"),
    ("knife_edge_area", "Knife-edge area", "mm²"),
];

fn num(v: &Value) -> String {
    match v.as_f64() {
        Some(x) => crate::params_ui::fmt_value((x * 1000.0).round() / 1000.0),
        None if v.is_null() => "n/a".into(),
        None => v.to_string(),
    }
}

fn unit(u: &str) -> &str {
    match u {
        "mm2" => "mm²",
        "deg" => "°",
        "ratio" => "",
        u => u,
    }
}

impl RcadApp {
    /// Open the analysis window of `plugin`.`analysis` (the selected or
    /// last body preselected).
    pub(crate) fn open_plugin_analysis(&mut self, plugin: &str, analysis: &str) {
        let Some(info) = self.plugins.find(plugin).cloned() else {
            self.notify(format!("Plugin '{plugin}' is not installed"));
            return;
        };
        let Some(mut form) = AnalysisForm::new(&info, analysis) else {
            self.notify(format!("Plugin '{plugin}' has no analysis '{analysis}'"));
            return;
        };
        let body = self
            .selected_body
            .or_else(|| self.selected_face().and_then(|f| f.body))
            .or_else(|| self.solid_bodies().first().map(|b| b.0));
        for s in &form.schema_inputs {
            if s.kind == InputKind::Body
                && let Some(b) = body
            {
                form.inputs
                    .insert(s.name.clone(), PluginInput::Body { body: b });
            }
        }
        self.plugins.analysis = Some(form);
    }

    /// The analysis window.
    pub(crate) fn analysis_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.plugins.analysis.take() else {
            return;
        };
        crate::dialogs::set_expression_env(&self.summary.parameters);
        let mut open = true;
        let mut run = false;
        let mut select: Option<(BodyId, u32)> = None;
        let mut map_action: Option<bool> = None;
        let map_shown = self.plugins.colour_map.is_some();
        let bodies = self.solid_bodies();
        let picked = self.selected_face().and_then(|f| f.reference);
        egui::Window::new(form.name.clone())
            .id(egui::Id::new("rcad_plugin_analysis"))
            .default_pos(self.plugin_window_pos(ctx, 0.0))
            .open(&mut open)
            .collapsible(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                if !form.description.is_empty() {
                    ui.small(&form.description);
                }
                crate::dialogs::begin_fields();
                crate::form::form(ui, "plugin_analysis", |ui| {
                    for s in form.schema_inputs.clone() {
                        match s.kind {
                            InputKind::Body => {
                                let cur = match form.inputs.get(&s.name) {
                                    Some(PluginInput::Body { body }) => Some(*body),
                                    _ => None,
                                };
                                let name = cur
                                    .and_then(|c| bodies.iter().find(|b| b.0 == c))
                                    .map_or("(choose)".to_string(), |b| b.1.clone());
                                crate::form::combo(ui, &s.label, name, |ui| {
                                    for (id, n) in &bodies {
                                        if ui
                                            .selectable_label(cur == Some(*id), n)
                                            .named(&format!("{} {n}", s.label))
                                            .clicked()
                                        {
                                            form.inputs.insert(
                                                s.name.clone(),
                                                PluginInput::Body { body: *id },
                                            );
                                        }
                                    }
                                })
                                .response
                                .probe(&s.label);
                            }
                            InputKind::Face => {
                                let cur = match form.inputs.get(&s.name) {
                                    Some(PluginInput::Face { plane }) => plane.face(),
                                    _ => None,
                                };
                                crate::form::row(ui, &s.label, |ui| {
                                    match &cur {
                                        Some(r) => {
                                            ui.label(self.face_text(r));
                                            if ui
                                                .small_button("×")
                                                .named(&format!("Clear {}", s.label))
                                                .clicked()
                                            {
                                                form.inputs.remove(&s.name);
                                            }
                                        }
                                        None => {
                                            ui.weak("none");
                                        }
                                    }
                                    if ui
                                        .add_enabled(
                                            picked.is_some(),
                                            egui::Button::new("Use selected face").small(),
                                        )
                                        .named(&format!("Use selected face for {}", s.label))
                                        .on_disabled_hover_text("Select a planar face in the view")
                                        .clicked()
                                        && let Some(r) = picked
                                    {
                                        form.inputs.insert(
                                            s.name.clone(),
                                            PluginInput::Face {
                                                plane: SketchPlane::Face(r),
                                            },
                                        );
                                    }
                                });
                            }
                            _ => {
                                ui.weak(format!("{}: not offered here", s.label));
                            }
                        }
                    }
                    let pull_face = form.inputs.keys().any(|k| {
                        form.schema_inputs
                            .iter()
                            .any(|s| &s.name == k && s.kind == InputKind::Face)
                    });
                    for (c, v) in &mut form.choices {
                        ui.add_enabled_ui(!pull_face, |ui| {
                            crate::form::combo(ui, &c.label, words(v), |ui| {
                                for x in &c.values {
                                    ui.selectable_value(v, x.clone(), words(x)).named(&format!(
                                        "{} {}",
                                        c.label,
                                        words(x)
                                    ));
                                }
                            })
                            .response
                            .probe(&c.label);
                        });
                    }
                    if pull_face {
                        ui.small("The face's normal is used instead of the axis choice.");
                    }
                    egui::CollapsingHeader::new("Limits")
                        .id_salt("plugin_analysis_limits")
                        .show(ui, |ui| {
                            for p in &mut form.params {
                                crate::panels::num_field_as(
                                    ui,
                                    &p.label,
                                    &mut p.text,
                                    slot_of(p.kind),
                                );
                            }
                            ui.small(
                                "Unchanged limits follow the body's rule (when it has one), \
                                 else these defaults.",
                            );
                        });
                });
                let invalid = crate::dialogs::invalid_fields();
                if let Some(why) = &invalid {
                    ui.colored_label(crate::theme::of(ui).bad, why);
                }
                ui.horizontal(|ui| {
                    run = ui
                        .add_enabled(!form.pending && invalid.is_none(), egui::Button::new("Run"))
                        .named("Run analysis")
                        .clicked();
                    if form.pending {
                        ui.spinner();
                    }
                });
                match &form.result {
                    None => {}
                    Some(Err(e)) => {
                        ui.colored_label(crate::theme::of(ui).bad, e);
                    }
                    Some(Ok(rep)) => {
                        report_ui(ui, &form.analysis, rep);
                        if super::colour_map::classify(rep).is_some() {
                            ui.horizontal(|ui| {
                                if map_shown {
                                    if ui.button("Hide").named("Hide colour map").clicked() {
                                        map_action = Some(false);
                                    }
                                } else if ui
                                    .button("Show on model")
                                    .named("Show on model")
                                    .on_hover_text(
                                        "Colour the analysed body's triangles in the 3D view",
                                    )
                                    .clicked()
                                {
                                    map_action = Some(true);
                                }
                            });
                            if let Some(e) = &form.map_error {
                                ui.colored_label(
                                    crate::theme::of(ui).warn,
                                    format!("Colour map unavailable: {e}"),
                                );
                            }
                        }
                        alerts_ui(ui, rep, &mut select);
                    }
                }
            });
        let _ = crate::dialogs::take_left_invalid();
        match map_action {
            Some(true) => {
                if let Some(Ok(rep)) = &form.result {
                    match ColourMap::build(
                        rep,
                        form.mesh.as_ref(),
                        &form.name,
                        self.summary.doc_id,
                        self.summary.revision,
                    ) {
                        Ok(m) => {
                            form.map_error = None;
                            self.plugins.colour_map = Some(m);
                        }
                        Err(e) => form.map_error = Some(e),
                    }
                }
            }
            Some(false) => self.plugins.colour_map = None,
            None => {}
        }
        if run {
            match form.command() {
                Ok(c) => {
                    form.pending = true;
                    form.result = None;
                    self.request(c);
                }
                Err(e) => form.result = Some(Err(e)),
            }
        }
        if let Some((body, face)) = select {
            // Display groups of a B-rep body are its faces in order.
            if let Some(d) = self
                .scene
                .bodies
                .iter()
                .find(|d| d.id == body && !d.mesh_body)
            {
                let g = d.groups.start + face;
                if d.groups.contains(&g) {
                    self.select_face(Some(g));
                }
            }
        }
        if open {
            self.plugins.analysis = Some(form);
        }
    }
}

/// A report: approximation note, the tailored results of a known analysis
/// and the summary values.
fn report_ui(ui: &mut egui::Ui, analysis: &str, rep: &Value) {
    ui.separator();
    if let Some(note) = rep
        .get("approximation")
        .and_then(|a| a.get("note"))
        .and_then(Value::as_str)
    {
        let chord = rep
            .get("approximation")
            .and_then(|a| a.get("chord"))
            .and_then(Value::as_f64);
        let text = match chord {
            Some(c) => format!("Approximate (tessellation within {c} mm): {note}"),
            None => format!("Approximate: {note}"),
        };
        ui.small(RichText::new(text).weak());
    }
    let rows = tailored(analysis, rep);
    if !rows.is_empty() {
        ui.label(RichText::new("Results").strong());
        egui::Grid::new("plugin_report_results")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for (label, value) in &rows {
                    ui.label(*label);
                    ui.label(value);
                    ui.end_row();
                }
            });
    }
    let Some(sum) = rep.get("summary").and_then(Value::as_object) else {
        return;
    };
    let grid = |ui: &mut egui::Ui| {
        egui::Grid::new("plugin_report_summary")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for (k, label, u) in SUMMARY {
                    if let Some(v) = sum.get(*k) {
                        ui.label(*label);
                        ui.label(format!("{} {u}", num(v)));
                        ui.end_row();
                    }
                }
                // Keys this window has no label for are shown as they are
                // (a nested object one row per entry).
                for (k, v) in sum {
                    if k == "alerts" || SUMMARY.iter().any(|s| s.0 == k) {
                        continue;
                    }
                    match v.as_object() {
                        Some(o) => {
                            for (k2, v2) in o {
                                ui.label(format!("{} · {}", words(k), words(k2)));
                                ui.label(num(v2));
                                ui.end_row();
                            }
                        }
                        None => {
                            ui.label(words(k));
                            ui.label(num(v));
                            ui.end_row();
                        }
                    }
                }
            });
    };
    if rows.is_empty() {
        grid(ui);
    } else {
        egui::CollapsingHeader::new("All summary values")
            .id_salt("plugin_report_all")
            .default_open(true)
            .show(ui, grid);
    }
    if let Some(counts) = sum.get("alerts").and_then(Value::as_object) {
        let t: Vec<String> = counts
            .iter()
            .map(|(k, n)| format!("{} {}", words(k), num(n)))
            .collect();
        ui.label(format!(
            "Alerts: {}",
            if t.is_empty() {
                "none".to_string()
            } else {
                t.join(", ")
            }
        ));
    }
}

/// The report's alerts (a clicked alert sets `select` to its body and face).
fn alerts_ui(ui: &mut egui::Ui, rep: &Value, select: &mut Option<(BodyId, u32)>) {
    let body = rep
        .get("body")
        .and_then(Value::as_u64)
        .and_then(|b| u32::try_from(b).ok())
        .map(BodyId);
    let Some(alerts) = rep.get("alerts").and_then(Value::as_array) else {
        return;
    };
    if alerts.is_empty() {
        return;
    }
    ui.label(RichText::new(format!("Alerts ({})", alerts.len())).strong());
    egui::ScrollArea::vertical()
        .id_salt("plugin_report_alerts")
        .max_height(220.0)
        .show(ui, |ui| {
            for (i, a) in alerts.iter().enumerate() {
                let kind = a.get("kind").and_then(Value::as_str).unwrap_or("?");
                let face = a.get("face").and_then(Value::as_u64);
                let value = a.get("value").map(num).unwrap_or_default();
                let u = a.get("unit").and_then(Value::as_str).unwrap_or("");
                let msg = a.get("message").and_then(Value::as_str).unwrap_or("");
                let head = match face {
                    Some(f) => format!("{} · face {f} · {value} {}", words(kind), unit(u)),
                    None => format!("{} · {value} {}", words(kind), unit(u)),
                };
                let r = ui
                    .selectable_label(false, head)
                    .named(&format!("Alert {}", i + 1))
                    .on_hover_text("Select this face in the view");
                if !msg.is_empty() {
                    ui.small(msg);
                }
                if r.clicked()
                    && let (Some(b), Some(f)) = (body, face.and_then(|f| u32::try_from(f).ok()))
                {
                    *select = Some((b, f));
                }
            }
        });
}

fn get<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(v, |v, k| v.get(*k))
}

fn area(v: Option<&Value>) -> String {
    v.map_or("n/a".into(), |v| format!("{} mm²", num(v)))
}

fn direction(v: Option<&Value>) -> String {
    match v.and_then(Value::as_array) {
        Some(a) if a.len() == 3 => {
            let c: Vec<String> = a.iter().map(num).collect();
            format!("({})", c.join(", "))
        }
        _ => "n/a".into(),
    }
}

fn counts(rep: &Value, keys: &[&str]) -> String {
    keys.iter()
        .map(|k| get(rep, &["summary", "faces", k]).map_or("0".into(), num))
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Results table of the analyses this window knows, as (label, value);
/// empty for any other analysis (its summary is shown generically only).
pub fn tailored(analysis: &str, rep: &Value) -> Vec<(&'static str, String)> {
    let s = |k: &str| get(rep, &["summary", k]);
    let src = |k: &str| {
        rep.get(k)
            .and_then(Value::as_str)
            .map_or(String::new(), |s| format!(" from {}", words(s)))
    };
    match analysis {
        "draft_analysis" => vec![
            (
                "Pull direction",
                format!("{}{}", direction(rep.get("pull")), src("pull_source")),
            ),
            (
                "Minimum draft",
                rep.get("min_draft_deg")
                    .map_or("n/a".into(), |v| format!("{} °", num(v))),
            ),
            ("Positive draft area", area(s("positive_area"))),
            ("Negative draft area", area(s("negative_area"))),
            ("Insufficient draft area", area(s("insufficient_area"))),
            ("Vertical wall area", area(s("vertical_area"))),
            (
                "Draft range",
                match (s("min_deg"), s("max_deg")) {
                    (Some(a), Some(b)) => format!("{} ° … {} °", num(a), num(b)),
                    _ => "n/a".into(),
                },
            ),
            (
                "Faces positive / negative / insufficient / mixed",
                counts(rep, &["positive", "negative", "insufficient", "mixed"]),
            ),
        ],
        "accessibility" => vec![
            (
                "Direction",
                format!(
                    "{}{}{}",
                    direction(rep.get("direction")),
                    src("direction_source"),
                    if rep.get("directions").and_then(Value::as_str) == Some("both") {
                        " and its opposite"
                    } else {
                        ""
                    }
                ),
            ),
            ("Accessible area", area(s("accessible_area"))),
            ("Inaccessible area", area(s("inaccessible_area"))),
            ("Blocked area (undercuts)", area(s("blocked_area"))),
            ("Area facing away", area(s("away_area"))),
            ("Edge-on area", area(s("vertical_area"))),
            (
                "Faces accessible / inaccessible / partial",
                counts(rep, &["accessible", "inaccessible", "partial"]),
            ),
        ],
        "minimum_radius" => vec![
            (
                "Minimum radius limit",
                rep.get("min_radius")
                    .map_or("n/a".into(), |v| format!("{} mm", num(v))),
            ),
            (
                "Smallest radius",
                match s("min_radius") {
                    Some(v) if !v.is_null() => format!("{} mm", num(v)),
                    _ => "none (no curved faces)".into(),
                },
            ),
            (
                "Faces below the limit",
                s("flagged_faces").map_or("0".into(), num),
            ),
            ("Area below the limit", area(s("flagged_area"))),
            (
                "Faces exact / approximate",
                format!(
                    "{} / {}",
                    s("exact_faces").map_or("0".into(), num),
                    s("approximate_faces").map_or("0".into(), num)
                ),
            ),
        ],
        _ => vec![],
    }
}
