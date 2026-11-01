//! The generic plugin feature dialog: one docked command dialog for every
//! feature type any plugin declares, built from its [`FeatureSchema`].
//!
//! Inputs by kind: a Face is picked in the view (the dialog's face input
//! takes the next pick, like Hole's), Faces collect picks, a Body or a
//! Sketch is chosen from a list, Points are typed (`x, y; x, y` in the
//! face's plane). Parameters are value fields of their declared unit that
//! take expressions; choices are combo boxes.
//!
//! Rules: for a new feature, a field the user did not change is left out of
//! the command, so when the feature's body has a rule the command binds that
//! slot to the rule (rule-driven defaults); the field shows the
//! rule template it will follow. Edit Feature reopens the dialog with the
//! stored values (bound slots show their expression) and redefines the
//! feature in place; expressions are kept unless a field now holds a number.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use rcad_commands::Command;
use rcad_commands::plugins::{PluginFeatureCmd, PluginInfo};
use rcad_document::{
    BodyId, FaceRef, FeatureId, FeatureKind, PluginInput, PluginParam, SketchPlane,
};
use rcad_plugin::{ChoiceSchema, InputKind, InputSchema, ParamKind};

use super::{slot_of, unit_text, words};
use crate::app::{Dialog, RcadApp};
use crate::params_ui::fmt_value;
use crate::probe::Probe as _;

/// One parameter field.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamField {
    pub name: String,
    /// Label with unit ("Height (mm)"): the field's accessible name.
    pub label: String,
    pub kind: ParamKind,
    pub text: String,
    /// The text the dialog opened with: an unchanged field of a new feature
    /// is left to the plugin's rule or default.
    pub initial: String,
    /// Rule template (`T/2`), if the plugin declares one.
    pub rule: Option<String>,
}

impl ParamField {
    pub fn touched(&self) -> bool {
        self.text.trim() != self.initial.trim()
    }

    /// The value: a number, or an expression evaluated against the
    /// document's parameters (lengths mm, angles degrees).
    pub fn value(&self) -> Result<f64, String> {
        crate::dialogs::check_value_as(slot_of(self.kind), &self.text)
            .map_err(|e| format!("{}: '{}' {e}", self.label, self.text))
    }

    /// The expression the field holds (None: a plain number).
    pub fn expression(&self) -> Option<String> {
        let t = self.text.trim();
        (!t.is_empty() && t.parse::<f64>().is_err()).then(|| t.to_string())
    }
}

/// Label of a parameter field.
pub fn param_label(label: &str, kind: ParamKind) -> String {
    format!("{label} ({})", unit_text(kind))
}

/// Parameter fields of a schema at their defaults.
pub fn param_fields(params: &[rcad_plugin::ParamSchema]) -> Vec<ParamField> {
    params
        .iter()
        .map(|p| ParamField {
            name: p.name.clone(),
            label: param_label(&p.label, p.kind),
            kind: p.kind,
            text: fmt_value(p.default),
            initial: fmt_value(p.default),
            rule: p.rule.clone(),
        })
        .collect()
}

/// The open plugin feature dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginForm {
    pub plugin: String,
    pub feature: String,
    /// The plugin's version (stored with a redefined feature).
    pub version: String,
    /// The feature type's display name ("Boss"): the dialog title.
    pub name: String,
    pub description: String,
    pub schema_inputs: Vec<InputSchema>,
    /// Chosen inputs (Points are kept as text in `points`).
    pub inputs: BTreeMap<String, PluginInput>,
    /// Points input text by input name ("x, y; x, y").
    pub points: BTreeMap<String, String>,
    pub params: Vec<ParamField>,
    /// (declaration, chosen value).
    pub choices: Vec<(ChoiceSchema, String)>,
    /// The Face/Faces input the next pick in the view goes to.
    pub active_face: Option<String>,
}

impl PluginForm {
    /// A new-feature dialog for `feature` of `info`, at the defaults.
    pub fn new(info: &PluginInfo, feature: &str) -> Option<Self> {
        let s = info.manifest.feature(feature)?;
        let mut f = PluginForm {
            plugin: info.manifest.id.clone(),
            feature: s.id.clone(),
            version: info.manifest.version.clone(),
            name: s.name.clone(),
            description: s.description.clone(),
            schema_inputs: s.inputs.clone(),
            inputs: BTreeMap::new(),
            points: BTreeMap::new(),
            params: param_fields(&s.params),
            choices: s
                .choices
                .iter()
                .map(|c| (c.clone(), c.default.clone()))
                .collect(),
            active_face: None,
        };
        f.active_face = f.next_empty_face();
        Some(f)
    }

    /// The dialog for a stored plugin feature (Edit Feature): stored inputs,
    /// values (bound slots show their expression) and choices.
    pub fn from_definition(
        info: &PluginInfo,
        def: &FeatureKind,
        expressions: &BTreeMap<String, String>,
    ) -> Option<Self> {
        let FeatureKind::Plugin {
            feature,
            inputs,
            params,
            choices,
            ..
        } = def
        else {
            return None;
        };
        let mut f = Self::new(info, feature)?;
        for (n, i) in inputs {
            match i {
                PluginInput::Points { points } => {
                    f.points.insert(n.clone(), points_text(points));
                }
                i => {
                    f.inputs.insert(n.clone(), i.clone());
                }
            }
        }
        for p in &mut f.params {
            let text = expressions
                .get(&p.name)
                .cloned()
                .or_else(|| params.get(&p.name).map(|v| fmt_value(v.value)));
            if let Some(t) = text {
                p.text = t.clone();
                p.initial = t;
            }
        }
        for (c, v) in &mut f.choices {
            if let Some(x) = choices.get(&c.name) {
                *v = x.clone();
            }
        }
        f.active_face = None;
        Some(f)
    }

    fn next_empty_face(&self) -> Option<String> {
        self.schema_inputs
            .iter()
            .find(|i| {
                matches!(i.kind, InputKind::Face | InputKind::Faces)
                    && !i.optional
                    && !self.inputs.contains_key(&i.name)
            })
            .map(|i| i.name.clone())
    }

    /// A face picked in the view: it fills the active face input. False when
    /// no face input is waiting.
    pub fn take_pick(&mut self, r: FaceRef) -> bool {
        let Some(name) = self.active_face.clone() else {
            return false;
        };
        let Some(s) = self.schema_inputs.iter().find(|i| i.name == name) else {
            return false;
        };
        match s.kind {
            InputKind::Face => {
                self.inputs.insert(
                    name,
                    PluginInput::Face {
                        plane: SketchPlane::Face(r),
                    },
                );
                self.active_face = self.next_empty_face();
            }
            InputKind::Faces => {
                // Picks add faces until the input is cleared or another
                // input is made active.
                match self.inputs.get_mut(&name) {
                    Some(PluginInput::Faces { faces }) => {
                        if !faces.contains(&r) {
                            faces.push(r);
                        }
                    }
                    _ => {
                        self.inputs
                            .insert(name, PluginInput::Faces { faces: vec![r] });
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// The command names faces picked in the view.
    pub fn uses_faces(&self) -> bool {
        self.inputs
            .values()
            .any(|i| matches!(i, PluginInput::Face { .. } | PluginInput::Faces { .. }))
    }

    /// Features and bodies the dialog refers to (closed when one goes).
    pub fn refs(&self) -> (Vec<FeatureId>, Vec<BodyId>) {
        let mut f = vec![];
        let mut b = vec![];
        for i in self.inputs.values() {
            match i {
                PluginInput::Face { plane } => f.extend(plane.face().map(|r| r.feature)),
                PluginInput::Faces { faces } => f.extend(faces.iter().map(|r| r.feature)),
                PluginInput::Sketch { sketch } => f.push(*sketch),
                PluginInput::Body { body } => b.push(*body),
                PluginInput::Points { .. } => {}
            }
        }
        (f, b)
    }

    /// Inputs of the command: every required one given, Points parsed.
    pub fn command_inputs(&self) -> Result<BTreeMap<String, PluginInput>, String> {
        let mut out = BTreeMap::new();
        for s in &self.schema_inputs {
            let v = match s.kind {
                InputKind::Points => match self.points.get(&s.name).map(|t| t.trim()) {
                    Some(t) if !t.is_empty() => Some(PluginInput::Points {
                        points: parse_points(t).map_err(|e| format!("{}: {e}", s.label))?,
                    }),
                    _ => None,
                },
                _ => self.inputs.get(&s.name).cloned(),
            };
            match v {
                Some(v) => {
                    out.insert(s.name.clone(), v);
                }
                None if s.optional => {}
                None => return Err(format!("{}: required", s.label)),
            }
        }
        Ok(out)
    }

    fn choice_map(&self) -> BTreeMap<String, String> {
        self.choices
            .iter()
            .map(|(c, v)| (c.name.clone(), v.clone()))
            .collect()
    }

    /// The create command. Fields left as they opened are omitted, so the
    /// plugin's rule (when the body has one) or default applies; a field
    /// holding an expression is bound to it.
    pub fn command(&self) -> Result<Command, String> {
        let mut params = BTreeMap::new();
        let mut expressions = BTreeMap::new();
        for p in self.params.iter().filter(|p| p.touched()) {
            params.insert(p.name.clone(), p.value()?);
            if let Some(e) = p.expression() {
                expressions.insert(p.name.clone(), e);
            }
        }
        Ok(Command::PluginFeature(PluginFeatureCmd {
            name: None,
            plugin: self.plugin.clone(),
            feature: self.feature.clone(),
            inputs: self.command_inputs()?,
            params,
            choices: self.choice_map(),
            expressions,
            use_rules: true,
        }))
    }

    /// The redefined feature (Edit Feature): every parameter from its field,
    /// stamped with the installed plugin's version.
    pub fn definition(&self) -> Result<FeatureKind, String> {
        let params = self
            .params
            .iter()
            .map(|p| {
                Ok((
                    p.name.clone(),
                    PluginParam {
                        value: p.value()?,
                        kind: p.kind,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        Ok(FeatureKind::Plugin {
            plugin: self.plugin.clone(),
            feature: self.feature.clone(),
            plugin_version: self.version.clone(),
            inputs: self.command_inputs()?,
            params,
            choices: self.choice_map(),
        })
    }
}

fn points_text(points: &[[f64; 2]]) -> String {
    points
        .iter()
        .map(|p| format!("{}, {}", fmt_value(p[0]), fmt_value(p[1])))
        .collect::<Vec<_>>()
        .join("; ")
}

/// "x, y; x, y" -> points (mm).
pub fn parse_points(t: &str) -> Result<Vec<[f64; 2]>, String> {
    t.split(';')
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            let v: Vec<&str> = s.split(',').map(str::trim).collect();
            match v.as_slice() {
                [x, y] => {
                    let x: f64 = x.parse().map_err(|_| format!("'{x}' is not a number"))?;
                    let y: f64 = y.parse().map_err(|_| format!("'{y}' is not a number"))?;
                    if x.is_finite() && y.is_finite() {
                        Ok([x, y])
                    } else {
                        Err("points must be finite".into())
                    }
                }
                _ => Err(format!("'{}' is not 'x, y'", s.trim())),
            }
        })
        .collect()
}

impl RcadApp {
    /// Open the plugin feature dialog for `plugin`.`feature` (a sketch or a
    /// face selected now fills the first matching input).
    pub(crate) fn open_plugin_feature(&mut self, plugin: &str, feature: &str) {
        if let Err(e) = self.need_open() {
            self.notify(e);
            return;
        }
        let Some(info) = self.plugins.find(plugin).cloned() else {
            self.notify(format!("Plugin '{plugin}' is not installed"));
            return;
        };
        let Some(mut form) = PluginForm::new(&info, feature) else {
            self.notify(format!("Plugin '{plugin}' has no feature '{feature}'"));
            return;
        };
        let sketch = self
            .selected
            .filter(|id| {
                self.summary
                    .features
                    .iter()
                    .any(|f| f.id == *id && f.kind == "sketch")
            })
            .or_else(|| {
                // Else the last sketch (the one usually just drawn).
                self.summary
                    .features
                    .iter()
                    .rev()
                    .find(|f| f.kind == "sketch" && !f.rolled_back)
                    .map(|f| f.id)
            });
        let face = self.selected_face().and_then(|f| f.reference);
        let body = self.selected_body.or_else(|| {
            self.selected_face()
                .and_then(|f| f.body)
                .or_else(|| self.solid_bodies().last().map(|b| b.0))
        });
        for s in form.schema_inputs.clone() {
            match s.kind {
                InputKind::Sketch if !s.optional => {
                    if let Some(id) = sketch {
                        form.inputs
                            .insert(s.name.clone(), PluginInput::Sketch { sketch: id });
                    }
                }
                InputKind::Body if !s.optional => {
                    if let Some(b) = body {
                        form.inputs
                            .insert(s.name.clone(), PluginInput::Body { body: b });
                    }
                }
                _ => {}
            }
        }
        if let Some(r) = face {
            form.take_pick(r);
        }
        self.shell.editing = None;
        self.last_dialog = None;
        self.dialog = Some(Dialog::PluginFeature(form));
    }

    /// The dialog body; false while OK must stay disabled.
    pub(crate) fn plugin_form_ui(&mut self, ui: &mut egui::Ui, f: &mut PluginForm) -> bool {
        let mut ok = true;
        if !f.description.is_empty() {
            ui.small(&f.description);
        }
        let sketches: Vec<(FeatureId, String)> = self
            .summary
            .features
            .iter()
            .filter(|x| x.kind == "sketch")
            .map(|x| (x.id, x.name.clone()))
            .collect();
        let bodies = self.solid_bodies();
        for s in f.schema_inputs.clone() {
            let label = if s.optional {
                format!("{} (optional)", s.label)
            } else {
                s.label.clone()
            };
            match s.kind {
                InputKind::Face | InputKind::Faces => {
                    let active = f.active_face.as_deref() == Some(s.name.as_str());
                    let text = match f.inputs.get(&s.name) {
                        Some(PluginInput::Face { plane }) => {
                            plane.face().map(|r| self.face_text(&r))
                        }
                        Some(PluginInput::Faces { faces }) => Some(format!(
                            "{} face(s): {}",
                            faces.len(),
                            faces
                                .iter()
                                .map(|r| self.face_text(r))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                        _ => None,
                    };
                    crate::form::row(ui, &label, |ui| {
                        if active {
                            ui.colored_label(
                                crate::theme::of(ui).info,
                                "Select a face in the view",
                            );
                        } else if let Some(t) = &text {
                            ui.label(RichText::new("selected").strong())
                                .on_hover_text(t);
                        } else {
                            ui.weak("none");
                        }
                        if !active
                            && ui
                                .small_button("Select")
                                .named(&format!("Select {}", s.label))
                                .on_hover_text("The next face picked in the view fills this input")
                                .clicked()
                        {
                            if s.kind == InputKind::Face {
                                f.inputs.remove(&s.name);
                            }
                            f.active_face = Some(s.name.clone());
                        }
                        if (text.is_some() || active)
                            && ui
                                .small_button("×")
                                .named(&format!("Clear {}", s.label))
                                .on_hover_text("Clear this input")
                                .clicked()
                        {
                            f.inputs.remove(&s.name);
                            if active {
                                f.active_face = None;
                            }
                        }
                    });
                    if let Some(t) = &text {
                        ui.small(t);
                    }
                    ok &= s.optional || f.inputs.contains_key(&s.name);
                }
                InputKind::Body => {
                    let cur = match f.inputs.get(&s.name) {
                        Some(PluginInput::Body { body }) => Some(*body),
                        _ => None,
                    };
                    let name = cur
                        .and_then(|c| bodies.iter().find(|b| b.0 == c))
                        .map_or("(none)".to_string(), |b| b.1.clone());
                    let mut pick: Option<Option<BodyId>> = None;
                    crate::form::combo(ui, &label, name, |ui| {
                        if s.optional && ui.selectable_label(cur.is_none(), "(none)").clicked() {
                            pick = Some(None);
                        }
                        for (id, n) in &bodies {
                            if ui
                                .selectable_label(cur == Some(*id), n)
                                .named(&format!("{} {n}", s.label))
                                .clicked()
                            {
                                pick = Some(Some(*id));
                            }
                        }
                    })
                    .response
                    .probe(&label);
                    match pick {
                        Some(Some(b)) => {
                            f.inputs
                                .insert(s.name.clone(), PluginInput::Body { body: b });
                        }
                        Some(None) => {
                            f.inputs.remove(&s.name);
                        }
                        None => {}
                    }
                    ok &= s.optional || f.inputs.contains_key(&s.name);
                }
                InputKind::Sketch => {
                    let cur = match f.inputs.get(&s.name) {
                        Some(PluginInput::Sketch { sketch }) => Some(*sketch),
                        _ => None,
                    };
                    let name = cur
                        .and_then(|c| sketches.iter().find(|x| x.0 == c))
                        .map_or("(choose)".to_string(), |x| x.1.clone());
                    let mut pick: Option<Option<FeatureId>> = None;
                    crate::form::combo(ui, &label, name, |ui| {
                        if s.optional && ui.selectable_label(cur.is_none(), "(none)").clicked() {
                            pick = Some(None);
                        }
                        for (id, n) in &sketches {
                            if ui
                                .selectable_label(cur == Some(*id), n)
                                .named(&format!("{} {n}", s.label))
                                .clicked()
                            {
                                pick = Some(Some(*id));
                            }
                        }
                    })
                    .response
                    .probe(&label);
                    match pick {
                        Some(Some(id)) => {
                            f.inputs
                                .insert(s.name.clone(), PluginInput::Sketch { sketch: id });
                        }
                        Some(None) => {
                            f.inputs.remove(&s.name);
                        }
                        None => {}
                    }
                    ok &= s.optional || f.inputs.contains_key(&s.name);
                }
                InputKind::Points => {
                    let buf = f.points.entry(s.name.clone()).or_default();
                    crate::panels::text_field(ui, &format!("{label} (x, y; …)"), buf);
                    let t = buf.trim().to_string();
                    if !t.is_empty()
                        && let Err(e) = parse_points(&t)
                    {
                        ui.colored_label(crate::theme::of(ui).bad, e);
                        ok = false;
                    }
                    ok &= s.optional || !t.is_empty();
                }
            }
        }
        // Rule hints: a new feature's untouched fields follow the body's rule.
        let editing = self.shell.editing.is_some();
        let symbols = if editing {
            BTreeMap::new()
        } else {
            let body = self.plugin_inputs_body(&f.inputs);
            let summary = self.summary.clone();
            self.plugins.rule_symbols(&f.plugin, body, &summary)
        };
        for p in &mut f.params {
            crate::panels::num_field_as(ui, &p.label, &mut p.text, slot_of(p.kind));
            if let Some(t) = &p.rule
                && let Some(e) = rcad_plugin::expand_rule(t, &symbols)
            {
                let note = if p.touched() {
                    format!("rule: {t} = {e} (overridden)")
                } else {
                    format!("rule: {t} = {e}")
                };
                crate::form::unlabelled(ui, |ui| ui.small(note));
            }
        }
        for (c, v) in &mut f.choices {
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
        }
        if !editing && !symbols.is_empty() {
            ui.small(
                "Fields left unchanged follow the body's rule (bound to its parameters); \
                 a typed value or expression replaces it.",
            );
        }
        ok
    }

    /// "Extrude1 end cap".
    pub(crate) fn face_text(&self, r: &FaceRef) -> String {
        format!(
            "{} {}",
            self.feature_name(r.feature),
            crate::face_role_label(&r.role)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plastics() -> PluginInfo {
        rcad_commands::plugins::register_builtin();
        rcad_commands::plugins::list()
            .into_iter()
            .find(|p| p.manifest.id == "plastics")
            .unwrap()
    }

    #[test]
    fn untouched_fields_are_left_to_the_rule() {
        let mut f = PluginForm::new(&plastics(), "boss").unwrap();
        f.inputs.insert(
            "sketch".into(),
            PluginInput::Sketch {
                sketch: FeatureId(3),
            },
        );
        let h = f.params.iter_mut().find(|p| p.name == "height").unwrap();
        h.text = "12".into();
        let Command::PluginFeature(c) = f.command().unwrap() else {
            panic!()
        };
        assert_eq!(c.params, BTreeMap::from([("height".to_string(), 12.0)]));
        assert!(c.expressions.is_empty());
        assert!(c.use_rules);
    }

    #[test]
    fn missing_required_inputs_and_bad_points_are_refused() {
        let f = PluginForm::new(&plastics(), "boss").unwrap();
        assert!(f.command().is_err());
        assert_eq!(
            parse_points("1, 2; -3,4.5").unwrap(),
            [[1.0, 2.0], [-3.0, 4.5]]
        );
        assert!(parse_points("1 2").is_err());
        assert!(parse_points("1, x").is_err());
    }

    #[test]
    fn a_stored_definition_reopens_and_redefines_unchanged() {
        let info = plastics();
        let mut f = PluginForm::new(&info, "rib").unwrap();
        f.inputs.insert(
            "sketch".into(),
            PluginInput::Sketch {
                sketch: FeatureId(3),
            },
        );
        let def = f.definition().unwrap();
        let back = PluginForm::from_definition(&info, &def, &BTreeMap::new()).unwrap();
        assert_eq!(back.definition().unwrap(), def);
    }
}
