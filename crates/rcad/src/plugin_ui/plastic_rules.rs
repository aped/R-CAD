//! Plastic rules windows (PLASTIC ▸ SETUP; docs/PLASTICS.md "Rules"): the
//! one plugin-specific part of the plugin UI, since rules are the plastics
//! plugin's own document data. Manage Plastic Rules lists the rules, adds
//! one from a preset or blank, edits its values (expressions over the
//! rule's symbols, e.g. `0.5*T`) and material, and deletes; Assign Plastic
//! Rule puts a body under a rule (or none). Both store the whole data with
//! one `SetPluginData` (one undo step, its parameters and materials
//! applied with it).

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use rcad_commands::Command;
use rcad_commands::plugins::SetPluginDataCmd;
use rcad_document::BodyId;
use rcad_document::material::CATALOG;
use rcad_plugin::ParamKind;
use rcad_plugin_plastics::rules::{FIELDS, PRESETS, PlasticData, Rule};

use crate::app::RcadApp;
use crate::probe::Probe as _;

const PLUGIN: &str = rcad_plugin_plastics::ID;

/// One rule being edited.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleEdit {
    /// The name the rule had when loaded (assignments follow a rename).
    pub was: Option<String>,
    pub name: String,
    pub material: Option<String>,
    /// Symbol -> expression text, every field (defaults filled).
    pub values: BTreeMap<String, String>,
}

impl RuleEdit {
    fn from_rule(r: &Rule) -> Self {
        let values = FIELDS
            .iter()
            .map(|(s, _, d, _)| {
                (
                    s.to_string(),
                    r.values.get(*s).cloned().unwrap_or_else(|| d.to_string()),
                )
            })
            .collect();
        RuleEdit {
            was: Some(r.name.clone()),
            name: r.name.clone(),
            material: r.material.clone(),
            values,
        }
    }

    /// The stored rule: values that differ from the field defaults (the
    /// thickness and draft always, as the presets store them).
    fn to_rule(&self) -> Rule {
        let values = FIELDS
            .iter()
            .filter_map(|(s, _, d, _)| {
                let v = self.values.get(*s)?.trim();
                let keep = !v.is_empty() && (v != *d || matches!(*s, "T" | "DA"));
                keep.then(|| (s.to_string(), v.to_string()))
            })
            .collect();
        Rule {
            name: self.name.trim().to_string(),
            material: self.material.clone(),
            values,
        }
    }
}

/// Manage Plastic Rules.
#[derive(Debug, Clone, PartialEq)]
pub struct RulesForm {
    pub rules: Vec<RuleEdit>,
    /// Body id -> rule name, as loaded.
    pub bodies: BTreeMap<u32, String>,
    /// The data the form was loaded from.
    pub base: serde_json::Value,
    /// Changed since loaded or stored.
    pub edited: bool,
    /// Preset chosen for Add ("" = blank).
    pub preset: String,
    pub pending: bool,
    /// Last store: (error, text).
    pub status: Option<(bool, String)>,
}

impl RulesForm {
    pub fn load(data: Option<&serde_json::Value>) -> Self {
        let base = data.cloned().unwrap_or(serde_json::Value::Null);
        let d = PlasticData::parse(&base).unwrap_or_default();
        RulesForm {
            rules: d.rules.iter().map(RuleEdit::from_rule).collect(),
            bodies: d.bodies,
            base,
            edited: false,
            preset: PRESETS.first().map_or(String::new(), |p| p.0.to_string()),
            pending: false,
            status: None,
        }
    }

    /// Add a rule from the chosen preset, or a blank one. A name in use
    /// gets a numeric suffix.
    pub fn add(&mut self) {
        let mut rule = Rule::preset(&self.preset).unwrap_or(Rule {
            name: "Rule".into(),
            material: None,
            values: BTreeMap::new(),
        });
        let taken = |n: &str, rules: &[RuleEdit]| {
            rules.iter().any(|r| r.name.trim().eq_ignore_ascii_case(n))
        };
        if taken(&rule.name, &self.rules) || self.preset.is_empty() {
            let base = rule.name.clone();
            let mut i = 1;
            while taken(&format!("{base}{i}"), &self.rules) {
                i += 1;
            }
            rule.name = format!("{base}{i}");
        }
        let mut e = RuleEdit::from_rule(&rule);
        e.was = None;
        self.rules.push(e);
        self.edited = true;
    }

    /// The data to store: the rules, and the assignments of rules that
    /// still exist (renamed rules keep their bodies).
    pub fn data(&self) -> serde_json::Value {
        let mut bodies = BTreeMap::new();
        for (b, n) in &self.bodies {
            if let Some(r) = self.rules.iter().find(|r| r.was.as_deref() == Some(n)) {
                bodies.insert(*b, r.name.trim().to_string());
            }
        }
        serde_json::to_value(PlasticData {
            rules: self.rules.iter().map(RuleEdit::to_rule).collect(),
            bodies,
        })
        .unwrap_or(serde_json::Value::Null)
    }
}

/// Assign Plastic Rule.
#[derive(Debug, Clone, PartialEq)]
pub struct AssignForm {
    pub body: Option<BodyId>,
    /// None: no rule.
    pub rule: Option<String>,
    pub pending: bool,
    pub status: Option<(bool, String)>,
}

fn unit(kind: ParamKind) -> &'static str {
    match kind {
        ParamKind::Length => "mm",
        ParamKind::Angle => "deg",
        ParamKind::Count | ParamKind::Ratio => "ratio",
    }
}

fn material_name(key: &Option<String>) -> String {
    match key {
        None => "(none)".into(),
        Some(k) => CATALOG
            .iter()
            .find(|e| e.key == k)
            .map_or_else(|| k.clone(), |e| e.name.to_string()),
    }
}

fn store(data: serde_json::Value) -> Command {
    Command::SetPluginData(SetPluginDataCmd {
        plugin: PLUGIN.into(),
        data,
    })
}

fn status_ui(ui: &mut egui::Ui, status: &Option<(bool, String)>) {
    if let Some((err, text)) = status {
        if *err {
            ui.colored_label(crate::theme::of(ui).bad, text);
        } else {
            ui.small(text);
        }
    }
}

impl RcadApp {
    pub(crate) fn open_plastic_rules(&mut self) {
        self.plugins.rules = Some(RulesForm::load(self.plugins.data.get(PLUGIN)));
    }

    pub(crate) fn open_assign_rule(&mut self) {
        let d = PlasticData::parse(
            self.plugins
                .data
                .get(PLUGIN)
                .unwrap_or(&serde_json::Value::Null),
        )
        .unwrap_or_default();
        let body = self
            .selected_body
            .or_else(|| self.selected_face().and_then(|f| f.body))
            .or_else(|| self.solid_bodies().first().map(|b| b.0));
        let rule = body
            .and_then(|b| d.bodies.get(&b.0).cloned())
            .or_else(|| d.rules.first().map(|r| r.name.clone()));
        self.plugins.assign = Some(AssignForm {
            body,
            rule,
            pending: false,
            status: None,
        });
    }

    /// Manage Plastic Rules window.
    pub(crate) fn plastic_rules_window(&mut self, ctx: &egui::Context) {
        let Some(mut f) = self.plugins.rules.take() else {
            return;
        };
        // Follow the document (undo, another window) while nothing is edited.
        let current = self
            .plugins
            .data
            .get(PLUGIN)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if !f.edited && !f.pending && current != f.base {
            let status = f.status.take();
            f = RulesForm::load(Some(&current));
            f.status = status;
        }
        let mut open = true;
        let mut apply = false;
        let mut delete: Option<usize> = None;
        egui::Window::new("Plastic Rules")
            .default_pos(self.plugin_window_pos(ctx, 0.0))
            .open(&mut open)
            .collapsible(false)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.small(
                    "Each rule becomes parameters <Rule>_T, <Rule>_DA, … Features created on a \
                     body with a rule bind their defaults to them. Values are expressions over \
                     the rule's own symbols (T, DA, NR, …).",
                );
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        if f.rules.is_empty() {
                            ui.weak("No rules yet: add one from a preset.");
                        }
                        let n = f.rules.len();
                        for i in 0..n {
                            let r = &mut f.rules[i];
                            let title = if r.name.trim().is_empty() {
                                format!("Rule {}", i + 1)
                            } else {
                                r.name.clone()
                            };
                            egui::CollapsingHeader::new(RichText::new(title).strong())
                                .id_salt(("plastic_rule", i))
                                .default_open(true)
                                .show(ui, |ui| {
                                    crate::form::form(ui, "plastic_rule", |ui| {
                                        ui.horizontal(|ui| {
                                            let l = crate::form::label(ui, "Name");
                                            f.edited |= ui
                                                .add(
                                                    egui::TextEdit::singleline(&mut r.name)
                                                        .desired_width(120.0),
                                                )
                                                .labelled_by(l.id)
                                                .named(&format!("Rule {} name", i + 1))
                                                .changed();
                                        });
                                        let before = r.material.clone();
                                        crate::form::combo(
                                            ui,
                                            &format!("Rule {} material", i + 1),
                                            material_name(&r.material),
                                            |ui| {
                                                ui.selectable_value(
                                                    &mut r.material,
                                                    None,
                                                    "(none)",
                                                );
                                                for e in CATALOG {
                                                    ui.selectable_value(
                                                        &mut r.material,
                                                        Some(e.key.to_string()),
                                                        e.name,
                                                    )
                                                    .named(&format!(
                                                        "Rule {} material {}",
                                                        i + 1,
                                                        e.name
                                                    ));
                                                }
                                            },
                                        )
                                        .response
                                        .probe(&format!("Rule {} material", i + 1));
                                        f.edited |= before != r.material;
                                        for (sym, kind, _, meaning) in FIELDS {
                                            let buf = r.values.entry(sym.to_string()).or_default();
                                            ui.horizontal(|ui| {
                                                let l = crate::form::label(
                                                    ui,
                                                    format!("{sym} ({})", unit(*kind)),
                                                );
                                                f.edited |= ui
                                                    .add(
                                                        egui::TextEdit::singleline(buf)
                                                            .desired_width(90.0),
                                                    )
                                                    .labelled_by(l.id)
                                                    .named(&format!("Rule {} {sym}", i + 1))
                                                    .on_hover_text(*meaning)
                                                    .changed();
                                            });
                                        }
                                    });
                                    if ui
                                        .small_button("Delete rule")
                                        .named(&format!("Delete rule {}", i + 1))
                                        .on_hover_text(
                                            "Remove the rule (its bodies lose the assignment; \
                                             its parameters stay)",
                                        )
                                        .clicked()
                                    {
                                        delete = Some(i);
                                    }
                                });
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    let text = if f.preset.is_empty() {
                        "Blank".to_string()
                    } else {
                        f.preset.clone()
                    };
                    egui::ComboBox::from_id_salt("plastic_rule_preset")
                        .selected_text(text)
                        .show_ui(ui, |ui| {
                            for (n, m, t, da) in PRESETS {
                                ui.selectable_value(&mut f.preset, n.to_string(), *n)
                                    .named(&format!("Preset {n}"))
                                    .on_hover_text(format!(
                                        "{}: T = {t}, DA = {da}°",
                                        material_name(&m.map(str::to_string))
                                    ));
                            }
                            ui.selectable_value(&mut f.preset, String::new(), "Blank")
                                .named("Preset Blank");
                        })
                        .response
                        .named("Rule preset");
                    if ui.button("Add rule").named("Add rule").clicked() {
                        f.add();
                    }
                });
                ui.small("Presets are typical starting values, not supplier data.");
                ui.separator();
                ui.horizontal(|ui| {
                    apply = ui
                        .add_enabled(f.edited && !f.pending, egui::Button::new("Apply"))
                        .named("Apply rules")
                        .on_disabled_hover_text("Nothing changed")
                        .clicked();
                    if f.pending {
                        ui.spinner();
                    }
                });
                status_ui(ui, &f.status);
            });
        if let Some(i) = delete {
            f.rules.remove(i);
            f.edited = true;
        }
        if apply {
            f.pending = true;
            f.status = None;
            let data = f.data();
            f.base = data.clone();
            self.request(store(data));
        }
        if open {
            self.plugins.rules = Some(f);
        }
    }

    /// Assign Plastic Rule window.
    pub(crate) fn assign_rule_window(&mut self, ctx: &egui::Context) {
        let Some(mut f) = self.plugins.assign.take() else {
            return;
        };
        let data = self
            .plugins
            .data
            .get(PLUGIN)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let d = PlasticData::parse(&data).unwrap_or_default();
        let bodies = self.solid_bodies();
        let mut open = true;
        let mut assign = false;
        let mut manage = false;
        egui::Window::new("Assign Plastic Rule")
            .default_pos(self.plugin_window_pos(ctx, 1.0))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                crate::form::form(ui, "assign_rule", |ui| {
                    let name = f
                        .body
                        .and_then(|b| bodies.iter().find(|x| x.0 == b))
                        .map_or("(choose)".to_string(), |b| b.1.clone());
                    crate::form::combo(ui, "Body", name, |ui| {
                        for (id, n) in &bodies {
                            ui.selectable_value(&mut f.body, Some(*id), n)
                                .named(&format!("Assign body {n}"));
                        }
                    })
                    .response
                    .probe("Assign body");
                    let rule = f.rule.clone().unwrap_or_else(|| "(none)".into());
                    crate::form::combo(ui, "Rule", rule, |ui| {
                        ui.selectable_value(&mut f.rule, None, "(none)")
                            .named("Assign rule (none)");
                        for r in &d.rules {
                            ui.selectable_value(&mut f.rule, Some(r.name.clone()), &r.name)
                                .named(&format!("Assign rule {}", r.name));
                        }
                    })
                    .response
                    .probe("Assign rule");
                });
                if let Some(b) = f.body {
                    let now = d.bodies.get(&b.0).map_or("none", String::as_str);
                    ui.small(format!("Current rule of this body: {now}"));
                }
                if d.rules.is_empty() {
                    ui.weak("No rules yet.");
                    manage = ui.button("Manage Plastic Rules…").clicked();
                }
                ui.small(
                    "The rule's material is assigned to the body; new features on it bind \
                     to the rule.",
                );
                ui.horizontal(|ui| {
                    assign = ui
                        .add_enabled(f.body.is_some() && !f.pending, egui::Button::new("Assign"))
                        .named("Assign rule")
                        .clicked();
                    if f.pending {
                        ui.spinner();
                    }
                });
                status_ui(ui, &f.status);
            });
        if assign && let Some(b) = f.body {
            let mut d = d;
            match &f.rule {
                Some(r) => {
                    d.bodies.insert(b.0, r.clone());
                }
                None => {
                    d.bodies.remove(&b.0);
                }
            }
            f.pending = true;
            f.status = None;
            match serde_json::to_value(d) {
                Ok(v) => self.request(store(v)),
                Err(e) => {
                    f.pending = false;
                    f.status = Some((true, e.to_string()));
                }
            }
        }
        if manage {
            self.open_plastic_rules();
        }
        if open {
            self.plugins.assign = Some(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_store_their_thickness_and_draft_only() {
        let mut f = RulesForm::load(None);
        f.preset = "ABS".into();
        f.add();
        f.add();
        let v = f.data();
        let d = PlasticData::parse(&v).unwrap();
        assert_eq!(d.rules.len(), 2);
        assert_eq!(d.rules[0].name, "ABS");
        assert_eq!(d.rules[1].name, "ABS1");
        assert_eq!(d.rules[0].material.as_deref(), Some("abs"));
        assert_eq!(d.rules[0].values.len(), 2);
        d.validate().unwrap();
    }

    #[test]
    fn renamed_rules_keep_their_bodies_and_deleted_rules_drop_them() {
        let data = serde_json::json!({
            "rules": [{"name": "ABS", "values": {"T": "2 mm"}}, {"name": "PP"}],
            "bodies": {"1": "ABS", "2": "PP"}
        });
        let mut f = RulesForm::load(Some(&data));
        f.rules[0].name = "ABS2".into();
        f.rules.remove(1);
        let d = PlasticData::parse(&f.data()).unwrap();
        assert_eq!(d.bodies, BTreeMap::from([(1, "ABS2".to_string())]));
        d.validate().unwrap();
    }
}
