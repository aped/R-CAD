//! Plastic rules (docs/PLASTICS.md "Rules"): named sets of nominal design
//! values (wall thickness, draft, nominal radius, lip clearance and reveal)
//! and design-advice limits, assigned to bodies.
//!
//! Like Fusion's rules, a rule becomes model parameters (`<Rule>_T`,
//! `<Rule>_DA`, ...) and features created on a body with a rule bind their
//! slots to expressions over them (`ABS_T/2`), so editing the rule updates
//! every bound feature. Rule fields are expressions over the rule's own
//! symbols (`NR = "0.5*T"`).
use rcad_plugin::{DataEffects, DataParameter, ParamKind, PluginError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The plugin's document data.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PlasticData {
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Body id → rule name.
    #[serde(default)]
    pub bodies: BTreeMap<u32, String>,
    /// Assembly component id → rule name (Fusion assigns rules to
    /// components); a body's own assignment wins over its component's.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<u32, String>,
}

/// One rule. Lengths in mm, angles in degrees; every value is an expression
/// over this rule's symbols (see [`FIELDS`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// Identifier prefix of the rule's parameters (`[A-Za-z][A-Za-z0-9_]*`).
    pub name: String,
    /// Material catalog key assigned to the rule's bodies (e.g. `abs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Symbol → expression; missing symbols take [`FIELDS`] defaults.
    #[serde(default)]
    pub values: BTreeMap<String, String>,
}

/// (symbol, parameter suffix, kind, default expression, meaning).
pub const FIELDS: &[(&str, ParamKind, &str, &str)] = &[
    ("T", ParamKind::Length, "2 mm", "nominal wall thickness"),
    ("DA", ParamKind::Angle, "1", "nominal draft angle (degrees)"),
    ("NR", ParamKind::Length, "0.5*T", "nominal inner radius"),
    (
        "CL",
        ParamKind::Length,
        "0.1 mm",
        "clearance between mating features",
    ),
    ("RH", ParamKind::Length, "T", "lip height (reveal)"),
    (
        "TMIN",
        ParamKind::Length,
        "0.5*T",
        "design advice: minimum thickness",
    ),
    (
        "TMAX",
        ParamKind::Length,
        "1.5*T",
        "design advice: maximum thickness",
    ),
    (
        "TV",
        ParamKind::Ratio,
        "0.15",
        "design advice: allowed thickness variation (ratio)",
    ),
    (
        "MDA",
        ParamKind::Angle,
        "0.5",
        "design advice: minimum draft angle (degrees)",
    ),
    (
        "KE",
        ParamKind::Length,
        "0.5*T",
        "design advice: knife-edge thickness threshold",
    ),
];

/// Built-in starting rules ("typical guidance", not supplier data; see
/// docs/PLASTICS.md): (name, material key, thickness, draft).
pub const PRESETS: &[(&str, Option<&str>, &str, &str)] = &[
    ("ABS", Some("abs"), "2 mm", "1"),
    ("ASA", Some("asa"), "2 mm", "1"),
    ("PC", Some("pc"), "2 mm", "1"),
    ("PC_ABS", Some("pc_abs"), "2 mm", "1"),
    ("PP", Some("pp"), "1.5 mm", "1"),
    ("PA66", Some("pa66"), "1.5 mm", "0.5"),
    ("POM", Some("pom"), "1.5 mm", "0.5"),
    ("HDPE", Some("hdpe"), "2 mm", "1.5"),
];

impl Rule {
    /// A preset rule by name.
    pub fn preset(name: &str) -> Option<Rule> {
        PRESETS
            .iter()
            .find(|p| p.0.eq_ignore_ascii_case(name))
            .map(|(n, m, t, da)| Rule {
                name: (*n).into(),
                material: m.map(Into::into),
                values: [
                    ("T".to_string(), (*t).to_string()),
                    ("DA".into(), (*da).into()),
                ]
                .into(),
            })
    }

    /// Parameter name of `symbol` (`ABS_T`).
    pub fn param(&self, symbol: &str) -> String {
        format!("{}_{symbol}", self.name)
    }

    /// Symbol → parameter name, for every field.
    pub fn symbols(&self) -> BTreeMap<String, String> {
        FIELDS
            .iter()
            .map(|(s, ..)| (s.to_string(), self.param(s)))
            .collect()
    }

    fn expression(&self, symbol: &str) -> String {
        self.values.get(symbol).cloned().unwrap_or_else(|| {
            FIELDS
                .iter()
                .find(|f| f.0 == symbol)
                .map(|f| f.2.to_string())
                .unwrap_or_default()
        })
    }
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl PlasticData {
    pub fn parse(v: &serde_json::Value) -> Result<PlasticData, PluginError> {
        if v.is_null() {
            return Ok(PlasticData::default());
        }
        serde_json::from_value(v.clone())
            .map_err(|e| PluginError::Input(format!("plastic rules: {e}")))
    }

    pub fn validate(&self) -> Result<(), PluginError> {
        let bad = |m: String| Err(PluginError::Input(format!("plastic rules: {m}")));
        let mut names = std::collections::BTreeSet::new();
        for r in &self.rules {
            if !valid_name(&r.name) {
                return bad(format!(
                    "rule name '{}' must be 1-32 letters, digits or _ starting with a letter",
                    r.name
                ));
            }
            if !names.insert(r.name.to_ascii_lowercase()) {
                return bad(format!("rule '{}' is defined twice", r.name));
            }
            for k in r.values.keys() {
                if !FIELDS.iter().any(|f| f.0 == k) {
                    return bad(format!("rule '{}': unknown value '{k}'", r.name));
                }
            }
            for (k, e) in &r.values {
                if rcad_plugin::expand_rule(e, &r.symbols()).is_none() {
                    return bad(format!(
                        "rule '{}': {k} = '{e}' uses a name that is not a rule value",
                        r.name
                    ));
                }
            }
        }
        for (b, n) in &self.bodies {
            if !self.rules.iter().any(|r| &r.name == n) {
                return bad(format!("body {b} is assigned to unknown rule '{n}'"));
            }
        }
        for (c, n) in &self.components {
            if !self.rules.iter().any(|r| &r.name == n) {
                return bad(format!("component {c} is assigned to unknown rule '{n}'"));
            }
        }
        Ok(())
    }

    pub fn rule(&self, name: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.name == name)
    }

    /// The rule assigned to `body`.
    pub fn rule_of(&self, body: Option<u32>) -> Option<&Rule> {
        self.rule(self.bodies.get(&body?)?)
    }

    /// The rule of `body`, else of its component.
    pub fn rule_at(&self, at: &rcad_plugin::RuleContext) -> Option<&Rule> {
        self.rule_of(at.body)
            .or_else(|| self.rule(self.components.get(&at.component?)?))
    }

    /// [`PlasticData::effects`], with each assigned component's rule
    /// material on its bodies (a body's own assignment wins).
    pub fn effects_in(&self, doc: &rcad_plugin::DocContext) -> DataEffects {
        let mut e = self.effects();
        for (c, n) in &self.components {
            let Some(m) = self.rule(n).and_then(|r| r.material.clone()) else {
                continue;
            };
            for b in doc.components.get(c).into_iter().flatten() {
                if !self.bodies.contains_key(b) {
                    e.materials.push((*b, m.clone()));
                }
            }
        }
        e
    }

    pub fn effects(&self) -> DataEffects {
        let mut e = DataEffects::default();
        for r in &self.rules {
            let sym = r.symbols();
            for (s, kind, _, meaning) in FIELDS {
                let expr = rcad_plugin::expand_rule(&r.expression(s), &sym)
                    .unwrap_or_else(|| r.expression(s));
                e.parameters.push(DataParameter {
                    name: r.param(s),
                    expression: expr,
                    kind: *kind,
                    comment: format!("plastic rule {}: {meaning}", r.name),
                });
            }
        }
        for (b, n) in &self.bodies {
            if let Some(m) = self.rule(n).and_then(|r| r.material.clone()) {
                e.materials.push((*b, m));
            }
        }
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_become_parameters_over_their_own_symbols() {
        let d = PlasticData {
            rules: vec![Rule::preset("abs").unwrap()],
            bodies: [(1, "ABS".to_string())].into(),
            ..Default::default()
        };
        d.validate().unwrap();
        let e = d.effects();
        let get = |n: &str| {
            e.parameters
                .iter()
                .find(|p| p.name == n)
                .unwrap()
                .expression
                .clone()
        };
        assert_eq!(get("ABS_T"), "2 mm");
        assert_eq!(get("ABS_NR"), "0.5*ABS_T");
        assert_eq!(get("ABS_RH"), "ABS_T");
        assert_eq!(e.materials, vec![(1, "abs".to_string())]);
        assert_eq!(d.rule_of(Some(1)).unwrap().param("T"), "ABS_T");
        assert!(d.rule_of(Some(2)).is_none());
    }

    #[test]
    fn invalid_rules_are_refused() {
        let mut d = PlasticData {
            rules: vec![Rule::preset("abs").unwrap()],
            bodies: [(1, "PC".to_string())].into(),
            ..Default::default()
        };
        assert!(d.validate().is_err());
        d.bodies.clear();
        d.rules[0].values.insert("T".into(), "X*2".into());
        assert!(d.validate().is_err());
        d.rules[0].values.insert("T".into(), "2 mm".into());
        d.rules[0].values.insert("ZZ".into(), "1".into());
        assert!(d.validate().is_err());
        d.rules[0].values.remove("ZZ");
        d.rules.push(Rule::preset("abs").unwrap());
        assert!(d.validate().is_err());
    }
}
