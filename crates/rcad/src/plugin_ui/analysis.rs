//! The generic plugin analysis window (Design Advice is the first): inputs,
//! parameters and choices from the [`rcad_plugin::AnalysisSchema`], Run
//! sends `RunPluginAnalysis`, and the report is shown as its `summary`
//! values and its `alerts` list (the report layout plugins share, see
//! docs/PLUGINS.md). Values derived from a tessellation are labelled
//! approximate with the report's own note. Clicking an alert selects the
//! face it names. Read-only: nothing here changes the document.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use rcad_commands::Command;
use rcad_commands::plugins::{PluginAnalysisCmd, PluginInfo};
use rcad_document::{BodyId, PluginInput, SketchPlane};
use rcad_plugin::{ChoiceSchema, InputKind, InputSchema};
use serde_json::Value;

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
                        select = report_ui(ui, rep);
                    }
                }
            });
        let _ = crate::dialogs::take_left_invalid();
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

/// A report: approximation note, summary values, alerts (a clicked alert
/// returns its body and face).
fn report_ui(ui: &mut egui::Ui, rep: &Value) -> Option<(BodyId, u32)> {
    let mut select = None;
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
    if let Some(sum) = rep.get("summary").and_then(Value::as_object) {
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
                // Keys this window has no label for are shown as they are.
                for (k, v) in sum {
                    if k == "alerts" || SUMMARY.iter().any(|s| s.0 == k) {
                        continue;
                    }
                    ui.label(words(k));
                    ui.label(num(v));
                    ui.end_row();
                }
            });
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
    let body = rep
        .get("body")
        .and_then(Value::as_u64)
        .and_then(|b| u32::try_from(b).ok())
        .map(BodyId);
    if let Some(alerts) = rep.get("alerts").and_then(Value::as_array)
        && !alerts.is_empty()
    {
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
                        select = Some((b, f));
                    }
                }
            });
    }
    select
}
