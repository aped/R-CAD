//! Rule binding in the core feature dialogs (docs/UI.md "Plugins and the
//! PLASTIC tab"): when the body a new Shell, Fillet, Fillet edge loop,
//! Extrude (thin or tapered) or Thicken works on is under a plugin rule, the
//! dialog offers "Use plastic rule (ABS_T)" (on by default). On OK the
//! feature is created as typed and, once that succeeds, bound to the rule
//! with `BindRuleParameters` (a second undo step). Typing into a
//! rule-driven field turns the choice off, so typed values stay values
//! unless the user turns it back on.
//!
//! Which slots a core feature kind binds, and to what, comes from the
//! installed plugins (`core_rule_templates` and their rule symbols), the
//! same data the command path uses; nothing here names a rule symbol.

use std::collections::BTreeMap;

use eframe::egui;
use rcad_commands::ExtrudeOp;
use rcad_commands::plugins::BindRulesCmd;
use rcad_commands::{Command, Response};
use rcad_document::{BodyId, FaceRef};

use crate::app::{Dialog, RcadApp};
use crate::probe::Probe as _;

/// The choice of the open dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreRuleUse {
    /// Title of the dialog it belongs to.
    pub dialog: &'static str,
    pub on: bool,
    /// Rule-driven field texts as last seen (an edit turns `on` off).
    pub seen: Vec<String>,
}

/// What a new-feature dialog would bind.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreTarget {
    /// Core kind name of `core_rule_templates` ("shell", "fillet", …).
    pub kind: &'static str,
    /// Slots of the new feature that exist (e.g. `thin` only when thin).
    pub slots: Vec<&'static str>,
    /// Rule context: the body worked on and its component (a new body has
    /// only the component it is made in).
    pub body: Option<BodyId>,
    pub component: Option<u32>,
    /// Texts of the rule-driven fields (fixed order per dialog).
    pub fields: Vec<String>,
}

/// One binding: (plugin name, slot, expression).
pub type Binding = (String, String, String);

/// "Use plastic rule (ABS_T)": the checkbox label of these bindings.
pub fn label(bindings: &[Binding]) -> String {
    let mut plugins: Vec<&str> = bindings.iter().map(|b| b.0.as_str()).collect();
    plugins.dedup();
    let who = match plugins.as_slice() {
        ["Plastics"] => "plastic rule".to_string(),
        [one] => format!("{one} rule"),
        _ => "plugin rules".to_string(),
    };
    let exprs: Vec<&str> = bindings.iter().map(|b| b.2.as_str()).collect();
    format!("Use {who} ({})", exprs.join(", "))
}

impl RcadApp {
    fn face_body(&self, r: &FaceRef) -> Option<BodyId> {
        self.summary
            .features
            .iter()
            .flat_map(|f| f.faces.iter())
            .find(|f| f.reference == *r)
            .map(|f| f.body)
    }

    fn component_of(&self, b: BodyId) -> Option<u32> {
        self.summary
            .assembly
            .components
            .iter()
            .find(|c| c.bodies.contains(&b))
            .map(|c| c.id.0)
    }

    /// The rule target of a new-feature dialog (None: the dialog's feature
    /// has no rule-driven slots, or it redefines an existing feature).
    pub(crate) fn core_target(&self, dlg: &Dialog) -> Option<CoreTarget> {
        if self.shell.editing.is_some() {
            return None;
        }
        let on_body = |kind, slots: Vec<&'static str>, body: Option<BodyId>, fields| {
            Some(CoreTarget {
                kind,
                slots,
                body,
                component: body.and_then(|b| self.component_of(b)),
                fields,
            })
        };
        match dlg {
            Dialog::Shell {
                extrude, thickness, ..
            } => {
                let body = self
                    .summary
                    .features
                    .iter()
                    .find(|f| f.id == *extrude)
                    .and_then(|f| f.creates.first().copied().or(f.body));
                on_body("shell", vec!["thickness"], body, vec![thickness.clone()])
            }
            Dialog::ShellFace { face, thickness } => on_body(
                "shell_face",
                vec!["thickness"],
                self.face_body(face),
                vec![thickness.clone()],
            ),
            Dialog::Chamfer {
                a,
                distance,
                fillet: true,
                ..
            } => on_body(
                "fillet",
                vec!["radius"],
                self.face_body(a),
                vec![distance.clone()],
            ),
            Dialog::FilletLoop {
                face,
                radius,
                chamfer: false,
                ..
            } => on_body(
                "fillet_loop",
                vec!["radius"],
                self.face_body(face),
                vec![radius.clone()],
            ),
            Dialog::Extrude(f) => {
                let body = match f.op {
                    ExtrudeOp::NewBody => None,
                    _ => f.target,
                };
                let mut slots = vec![];
                if f.more.thin {
                    slots.push("thin");
                }
                let taper = crate::app::parse_num("taper", &f.more.taper_deg) != Ok(0.0);
                if taper {
                    slots.push("taper_deg");
                }
                if slots.is_empty() {
                    return None;
                }
                on_body(
                    "extrude",
                    slots,
                    body,
                    vec![f.more.thin_t.clone(), f.more.taper_deg.clone()],
                )
            }
            _ => None,
        }
    }

    /// The bindings `t` would get from the installed plugins' rules.
    pub(crate) fn core_bindings(&mut self, t: &CoreTarget) -> Vec<Binding> {
        let list = self.plugins.list.clone();
        let mut out: BTreeMap<String, Binding> = BTreeMap::new();
        for p in &list {
            let Some(plugin) = rcad_plugin::find(&p.manifest.id) else {
                continue;
            };
            let templates = plugin.core_rule_templates(t.kind);
            if templates.is_empty() {
                continue;
            }
            let sym =
                self.plugins
                    .rule_symbols_at(&p.manifest.id, t.body, t.component, &self.summary);
            if sym.is_empty() {
                continue;
            }
            for (slot, tpl) in templates {
                if !t.slots.contains(&slot.as_str()) || out.contains_key(&slot) {
                    continue;
                }
                if let Some(e) = rcad_plugin::expand_rule(&tpl, &sym) {
                    out.insert(slot.clone(), (p.manifest.name.clone(), slot, e));
                }
            }
        }
        out.into_values().collect()
    }

    /// The "Use plastic rule" row of a new-feature dialog (nothing when no
    /// rule applies). `opened`: the dialog opened this frame.
    pub(crate) fn core_rule_ui(&mut self, ui: &mut egui::Ui, dlg: &Dialog, opened: bool) {
        let Some(t) = self.core_target(dlg) else {
            self.plugins.core_rule = None;
            return;
        };
        let bindings = self.core_bindings(&t);
        if bindings.is_empty() {
            self.plugins.core_rule = None;
            return;
        }
        let fresh = opened
            || self
                .plugins
                .core_rule
                .as_ref()
                .is_none_or(|c| c.dialog != dlg.title());
        if fresh {
            self.plugins.core_rule = Some(CoreRuleUse {
                dialog: dlg.title(),
                on: true,
                seen: t.fields.clone(),
            });
        }
        let Some(c) = self.plugins.core_rule.as_mut() else {
            return;
        };
        if c.seen != t.fields {
            // A typed value stays a value.
            c.on = false;
            c.seen = t.fields.clone();
        }
        let text = label(&bindings);
        crate::form::check(ui, &mut c.on, &text).probe("Use plastic rule");
        let what: Vec<String> = bindings
            .iter()
            .map(|(_, s, e)| format!("{s} = {e}"))
            .collect();
        ui.small(format!(
            "On OK the new feature is bound to the body's rule ({}); a second undo step. \
             Typing a value turns this off.",
            what.join(", ")
        ));
    }

    /// After OK sent the creating command with `token`: bind its feature
    /// once it succeeds, when the dialog's choice is on.
    pub(crate) fn core_rule_ok(&mut self, dlg: &Dialog, token: Option<u64>) {
        let on = self
            .plugins
            .core_rule
            .take()
            .is_some_and(|c| c.on && c.dialog == dlg.title());
        if on && self.core_target(dlg).is_some() {
            self.plugins.bind_after = token;
        }
    }

    /// Reply hook: the awaited creation succeeded, so bind its feature.
    pub(crate) fn bind_rules_reply(&mut self, response: &Response) {
        let Some(token) = self.plugins.bind_after else {
            return;
        };
        if self.replies_seen() < token {
            return;
        }
        self.plugins.bind_after = None;
        if self.replies_seen() == token
            && response.ok
            && let Some(feature) = response.feature
        {
            self.request(Command::BindRuleParameters(BindRulesCmd {
                feature,
                plugin: None,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_names_the_rule_expressions() {
        let b = vec![("Plastics".to_string(), "thickness".into(), "ABS_T".into())];
        assert_eq!(label(&b), "Use plastic rule (ABS_T)");
    }
}
