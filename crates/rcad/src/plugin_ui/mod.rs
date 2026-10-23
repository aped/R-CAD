//! Plugins in the app (docs/PLUGINS.md, docs/UI.md "Plugins"): the generic
//! plugin feature dialog ([`feature`]), the generic analysis window
//! ([`analysis`]) with its viewport colour map ([`colour_map`]), the
//! Plugins window listing what is installed, the plastic rules windows
//! ([`plastic_rules`], the one plugin-specific part), and the rule binding
//! of core feature dialogs ([`core_rules`]).
//!
//! Everything here reads plugin manifests; no feature is known by name
//! except through the PLASTIC ribbon tab's commands. The installed plugins
//! are read from the process-wide registry the session uses
//! (`rcad_commands::plugins::list`), and the open document's plugin data
//! arrives with every engine reply ([`crate::engine::Reply::plugin_data`]).

pub mod analysis;
pub mod colour_map;
pub mod core_rules;
pub mod feature;
pub mod plastic_rules;

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use rcad_commands::plugins::PluginInfo;
use rcad_commands::{Command, FeatureSummary, Response};
use rcad_document::{BodyId, FeatureKind};
use rcad_plugin::ParamKind;

use crate::app::RcadApp;
use crate::engine::{AnalysisMesh, PluginData};
use crate::probe::Probe as _;

/// Rule symbols of one (plugin, body, revision), so a dialog does not ask
/// the plugin (possibly another process) every frame.
#[derive(Debug, Clone, PartialEq)]
struct RuleCache {
    plugin: String,
    body: Option<BodyId>,
    component: Option<u32>,
    revision: u64,
    doc_id: u64,
    symbols: BTreeMap<String, String>,
}

/// Plugin view state (never part of the document).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PluginUi {
    /// Installed plugins (built-in first, as the registry lists them).
    pub list: Vec<PluginInfo>,
    /// The open document's plugin data (from the last reply).
    pub data: PluginData,
    /// Errors of loading the installed external plugins.
    pub load_errors: Vec<String>,
    /// Installed external plugins were loaded (once per process).
    pub loaded_installed: bool,
    /// The Plugins window is open.
    pub show_list: bool,
    pub rules: Option<plastic_rules::RulesForm>,
    pub assign: Option<plastic_rules::AssignForm>,
    pub analysis: Option<analysis::AnalysisForm>,
    /// The analysis colour map shown on the model (a derived display).
    pub colour_map: Option<colour_map::ColourMap>,
    /// The "Use plastic rule" choice of the open core feature dialog.
    pub core_rule: Option<core_rules::CoreRuleUse>,
    /// Request token of a core feature creation to bind to its body's rule
    /// once it succeeds (`BindRuleParameters`).
    pub bind_after: Option<u64>,
    rule_cache: Option<RuleCache>,
}

impl PluginUi {
    /// The built-in plugins registered and listed.
    pub fn new() -> Self {
        rcad_commands::plugins::register_builtin();
        Self {
            list: rcad_commands::plugins::list(),
            ..Default::default()
        }
    }

    pub fn find(&self, plugin: &str) -> Option<&PluginInfo> {
        self.list.iter().find(|p| p.manifest.id == plugin)
    }

    /// Reply bookkeeping: the document's plugin data, the plugin list, and
    /// the results the open windows wait for.
    pub(crate) fn on_reply(
        &mut self,
        cmd: &Command,
        r: &Response,
        data: PluginData,
        mesh: Option<AnalysisMesh>,
    ) {
        self.data = data;
        match cmd {
            Command::ListPlugins => {
                if let Some(v) = &r.plugins {
                    self.list = v.clone();
                }
            }
            Command::SetPluginData(_) => {
                let status = if r.ok {
                    (false, r.message.clone().unwrap_or_else(|| "stored".into()))
                } else {
                    (true, error_text(r))
                };
                if let Some(f) = &mut self.rules
                    && f.pending
                {
                    f.pending = false;
                    if !status.0 {
                        f.edited = false;
                    }
                    f.status = Some(status.clone());
                }
                if let Some(f) = &mut self.assign
                    && f.pending
                {
                    f.pending = false;
                    f.status = Some(status);
                }
            }
            Command::RunPluginAnalysis(_) => {
                if let Some(f) = &mut self.analysis
                    && f.pending
                {
                    f.pending = false;
                    f.result = Some(match (&r.plugin_report, r.ok) {
                        (Some(rep), true) => Ok(rep.clone()),
                        _ => Err(error_text(r)),
                    });
                    f.mesh = mesh;
                    f.map_error = None;
                    // A new report replaces the map of the previous one.
                    self.colour_map = None;
                }
            }
            _ => {}
        }
    }

    /// A new document: windows that hold its ids close.
    pub(crate) fn document_changed(&mut self) {
        self.rules = None;
        self.assign = None;
        self.analysis = None;
        self.colour_map = None;
        self.core_rule = None;
        self.bind_after = None;
        self.rule_cache = None;
    }

    /// The colour map belongs to one model state: dropped when the model
    /// changes (any revision or document change).
    pub(crate) fn model_state(&mut self, doc_id: u64, revision: u64) {
        if self
            .colour_map
            .as_ref()
            .is_some_and(|m| m.doc_id != doc_id || m.revision != revision)
        {
            self.colour_map = None;
        }
    }

    /// Rule symbols (symbol -> parameter name) `plugin` applies to a new
    /// feature on `body`, limited to parameters that exist (as the command
    /// path does). Cached per body and model revision.
    pub(crate) fn rule_symbols(
        &mut self,
        plugin: &str,
        body: Option<BodyId>,
        summary: &rcad_commands::DocSummary,
    ) -> BTreeMap<String, String> {
        let component = body.and_then(|b| {
            summary
                .assembly
                .components
                .iter()
                .find(|c| c.bodies.contains(&b))
                .map(|c| c.id.0)
        });
        self.rule_symbols_at(plugin, body, component, summary)
    }

    /// [`PluginUi::rule_symbols`] for an explicit rule context (a body
    /// and/or a component; a new body has only its component).
    pub(crate) fn rule_symbols_at(
        &mut self,
        plugin: &str,
        body: Option<BodyId>,
        component: Option<u32>,
        summary: &rcad_commands::DocSummary,
    ) -> BTreeMap<String, String> {
        if let Some(c) = &self.rule_cache
            && c.plugin == plugin
            && c.body == body
            && c.component == component
            && c.revision == summary.revision
            && c.doc_id == summary.doc_id
        {
            return c.symbols.clone();
        }
        let data = self
            .data
            .get(plugin)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let symbols: BTreeMap<String, String> = match rcad_plugin::find(plugin) {
            Some(p) if !data.is_null() => p
                .rule_symbols_in(
                    &data,
                    &rcad_plugin::RuleContext {
                        body: body.map(|b| b.0),
                        component,
                    },
                )
                .into_iter()
                .filter(|(_, n)| summary.parameters.iter().any(|q| &q.name == n))
                .collect(),
            _ => BTreeMap::new(),
        };
        self.rule_cache = Some(RuleCache {
            plugin: plugin.into(),
            body,
            component,
            revision: summary.revision,
            doc_id: summary.doc_id,
            symbols: symbols.clone(),
        });
        symbols
    }
}

fn error_text(r: &Response) -> String {
    r.error
        .as_ref()
        .map_or_else(|| "refused".to_string(), |e| e.to_string())
}

/// Unit text of a parameter kind (field labels and the inspector).
pub fn unit_text(kind: ParamKind) -> &'static str {
    match kind {
        ParamKind::Length => "mm",
        ParamKind::Angle => "deg",
        ParamKind::Count => "count",
        ParamKind::Ratio => "ratio",
    }
}

/// Expression slot of a parameter kind.
pub fn slot_of(kind: ParamKind) -> rcad_document::SlotKind {
    use rcad_document::SlotKind;
    match kind {
        ParamKind::Length => SlotKind::Length,
        ParamKind::Angle => SlotKind::Angle,
        ParamKind::Count => SlotKind::Count,
        ParamKind::Ratio => SlotKind::Ratio,
    }
}

/// "snap_fit" -> "snap fit" (choice values and ids shown as words).
pub fn words(id: &str) -> String {
    id.replace('_', " ")
}

/// The display name of a plugin feature's type ("Boss"), from its manifest
/// when the plugin is installed, else "<plugin>.<feature>".
pub fn feature_type_name(f: &FeatureSummary) -> Option<String> {
    let Some(FeatureKind::Plugin {
        plugin, feature, ..
    }) = &f.definition
    else {
        return None;
    };
    Some(
        rcad_plugin::find(plugin)
            .and_then(|p| p.manifest().feature(feature).map(|s| s.name.clone()))
            .unwrap_or_else(|| format!("{plugin}.{feature}")),
    )
}

impl RcadApp {
    /// Installed plugins (the Plugins window, the plugin feature menu).
    pub fn plugins(&self) -> &[PluginInfo] {
        &self.plugins.list
    }

    /// The open document's data of `plugin` (e.g. the plastic rules).
    pub fn plugin_data(&self, plugin: &str) -> Option<&serde_json::Value> {
        self.plugins.data.get(plugin)
    }

    /// Load the external plugins installed in the default directories
    /// (`RCAD_PLUGIN_PATH`, then `$XDG_DATA_HOME/r-cad/plugins`), once.
    /// Interactive runs call this at start (`main`); tests and scripted
    /// scenes do not, so they never load a user's plugins. Load errors go to
    /// the message log.
    pub fn load_installed_plugins(&mut self) {
        if std::mem::replace(&mut self.plugins.loaded_installed, true) {
            return;
        }
        let (ok, errors) = rcad_commands::plugins::load_installed();
        for e in &errors {
            self.diag(false, "Plugin", format!("plugin not loaded: {e}"));
        }
        if !ok.is_empty() {
            self.diag(false, "info", format!("plugins loaded: {}", ok.join(", ")));
        }
        if !errors.is_empty() {
            self.notify(format!(
                "{} plugin(s) could not be loaded; see Plugins… (UTILITIES) and the messages",
                errors.len()
            ));
        }
        self.plugins.load_errors = errors;
        self.plugins.list = rcad_commands::plugins::list();
    }

    /// Open the Plugins window (re-lists the registry).
    pub(crate) fn open_plugins_window(&mut self) {
        self.plugins.list = rcad_commands::plugins::list();
        self.plugins.show_list = true;
    }

    /// The plugin menu: every feature and every analysis of every installed
    /// plugin.
    pub(crate) fn plugin_features_menu(&mut self, ui: &mut egui::Ui) {
        crate::shell::ribbon::menu_min_width(ui);
        let list = self.plugins.list.clone();
        if list
            .iter()
            .all(|p| p.manifest.features.is_empty() && p.manifest.analyses.is_empty())
        {
            ui.weak("No plugin features or analyses installed");
        }
        let solid = !self.solid_bodies().is_empty();
        for p in &list {
            if p.manifest.features.is_empty() && p.manifest.analyses.is_empty() {
                continue;
            }
            ui.label(RichText::new(&p.manifest.name).small().strong());
            for f in &p.manifest.features {
                let label = format!("{}: {}…", p.manifest.name, f.name);
                let r = ui
                    .add_enabled(self.summary.open, egui::Button::new(&label))
                    .named(&label)
                    .on_hover_text(&f.description)
                    .on_disabled_hover_text("Open or create a document first");
                if r.clicked() {
                    self.open_plugin_feature(&p.manifest.id, &f.id);
                    ui.close();
                }
            }
            if !p.manifest.analyses.is_empty() && !p.manifest.features.is_empty() {
                ui.separator();
            }
            for a in &p.manifest.analyses {
                let label = format!("{}: {}…", p.manifest.name, a.name);
                let r = ui
                    .add_enabled(solid, egui::Button::new(&label))
                    .named(&label)
                    .on_hover_text(&a.description)
                    .on_disabled_hover_text("Create a solid body first");
                if r.clicked() {
                    self.open_plugin_analysis(&p.manifest.id, &a.id);
                    ui.close();
                }
            }
        }
        ui.separator();
        if ui.button("Plugins…").named("Plugins window").clicked() {
            self.open_plugins_window();
            ui.close();
        }
    }

    /// The Plugins window: id, name, version, built-in or external,
    /// features and analyses of every installed plugin, and load errors.
    pub(crate) fn plugins_window(&mut self, ctx: &egui::Context) {
        if !self.plugins.show_list {
            return;
        }
        let mut open = true;
        let list = self.plugins.list.clone();
        let errors = self.plugins.load_errors.clone();
        egui::Window::new("Plugins")
            .default_pos(self.plugin_window_pos(ctx, 0.0))
            .open(&mut open)
            .collapsible(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        for p in &list {
                            let m = &p.manifest;
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&m.name).strong());
                                ui.weak(format!(
                                    "{} · {} · {}",
                                    m.id,
                                    m.version,
                                    if p.builtin { "built-in" } else { "external" }
                                ));
                            });
                            if !m.description.is_empty() {
                                ui.small(&m.description);
                            }
                            let names = |v: Vec<&str>| {
                                if v.is_empty() {
                                    "none".to_string()
                                } else {
                                    v.join(", ")
                                }
                            };
                            ui.small(format!(
                                "Features: {}",
                                names(m.features.iter().map(|f| f.name.as_str()).collect())
                            ));
                            ui.small(format!(
                                "Analyses: {}",
                                names(m.analyses.iter().map(|a| a.name.as_str()).collect())
                            ));
                            ui.separator();
                        }
                        if list.is_empty() {
                            ui.weak("No plugins are installed.");
                        }
                        if !errors.is_empty() {
                            ui.label(RichText::new("Not loaded").strong());
                            for e in &errors {
                                ui.colored_label(crate::theme::of(ui).bad, e);
                            }
                        }
                        ui.small(
                            "External plugins load at start from RCAD_PLUGIN_PATH and \
                         <data dir>/r-cad/plugins (docs/PLUGINS.md).",
                        );
                    });
            });
        self.plugins.show_list = open;
    }

    /// The plugin windows (drawn every frame; each returns when closed).
    pub(crate) fn plugin_windows(&mut self, ctx: &egui::Context) {
        self.plugins_window(ctx);
        self.plastic_rules_window(ctx);
        self.assign_rule_window(ctx);
        self.analysis_window(ctx);
    }

    /// The body a plugin feature or analysis with these inputs works on (for
    /// its rule), as the command path decides it: a face's body, a sketch's
    /// face body, else the first body input.
    pub(crate) fn plugin_inputs_body(
        &self,
        inputs: &BTreeMap<String, rcad_document::PluginInput>,
    ) -> Option<BodyId> {
        use rcad_document::PluginInput as I;
        let face_body = |r: rcad_document::FaceRef| {
            self.summary
                .features
                .iter()
                .flat_map(|f| f.faces.iter())
                .find(|f| f.reference == r)
                .map(|f| f.body)
        };
        for i in inputs.values() {
            if let I::Face { plane } = i
                && let Some(b) = plane.face().and_then(face_body)
            {
                return Some(b);
            }
        }
        for i in inputs.values() {
            if let I::Sketch { sketch } = i
                && let Some(b) = self
                    .summary
                    .features
                    .iter()
                    .find(|f| f.id == *sketch)
                    .and_then(|f| f.plane.as_ref())
                    .and_then(|p| p.face())
                    .and_then(face_body)
            {
                return Some(b);
            }
        }
        inputs.values().find_map(|i| match i {
            I::Body { body } => Some(*body),
            _ => None,
        })
    }

    /// Where a plugin window opens: the top left of the 3D view (never over
    /// the ribbon buttons that open the others, nor over the command dialog
    /// docked at the view's right).
    pub(crate) fn plugin_window_pos(&self, ctx: &egui::Context, slot: f32) -> egui::Pos2 {
        let view = self.view_rect.unwrap_or_else(|| ctx.content_rect());
        view.min + egui::vec2(8.0 + 24.0 * slot, 8.0 + 24.0 * slot)
    }

    /// The last analysis report (None: not run, running, or refused).
    pub fn plugin_report(&self) -> Option<&serde_json::Value> {
        self.plugins
            .analysis
            .as_ref()
            .and_then(|f| f.result.as_ref())
            .and_then(|r| r.as_ref().ok())
    }

    /// Solid (B-rep) bodies, for body inputs.
    pub(crate) fn solid_bodies(&self) -> Vec<(BodyId, String)> {
        self.summary
            .bodies
            .iter()
            .filter(|b| b.kind == "brep")
            .map(|b| (b.id, b.name.clone()))
            .collect()
    }
}
