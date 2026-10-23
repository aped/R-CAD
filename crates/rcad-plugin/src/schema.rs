//! What a plugin declares: its features, their inputs, numeric parameters and
//! choices, and its analyses. The document, the command API, the CLI and the
//! generic plugin dialog read only this; none of them knows a plugin's
//! features by name.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::PluginError;

/// Version of the plugin interface this build implements. A manifest naming
/// another version is refused when the plugin is registered.
pub const API_VERSION: u32 = 1;

/// A plugin's declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Stable identifier, stored in documents (`[a-z0-9_.-]`, e.g. `plastics`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// The plugin's own version (informational; stored with each feature).
    pub version: String,
    /// Interface version ([`API_VERSION`]).
    pub api_version: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub features: Vec<FeatureSchema>,
    #[serde(default)]
    pub analyses: Vec<AnalysisSchema>,
}

/// One feature type of a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSchema {
    /// Identifier within the plugin (`[a-z0-9_]`).
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub inputs: Vec<InputSchema>,
    #[serde(default)]
    pub params: Vec<ParamSchema>,
    #[serde(default)]
    pub choices: Vec<ChoiceSchema>,
}

/// A selection the feature needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputSchema {
    pub name: String,
    pub label: String,
    pub kind: InputKind,
    #[serde(default)]
    pub optional: bool,
}

/// Kind of a feature input. The document stores the persistent reference
/// (a face reference, a body id, a sketch feature id, plane points) and
/// resolves it on every regeneration into [`crate::Resolved`] geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    /// One planar face (the feature's placement plane).
    Face,
    /// One or more planar faces.
    Faces,
    /// One body.
    Body,
    /// A sketch (its solved curves in its plane).
    Sketch,
    /// Points in the plane of the feature's `Face` input (plane coordinates, mm).
    Points,
}

/// Unit class of a numeric parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    /// Millimetres.
    Length,
    /// Degrees (parameter names end in `_deg`, like every angle slot).
    Angle,
    /// Whole number.
    Count,
    /// Unitless.
    Ratio,
}

/// A numeric, expression-bindable parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamSchema {
    pub name: String,
    pub label: String,
    pub kind: ParamKind,
    pub default: f64,
    /// Inclusive bounds; a value outside is refused with the parameter named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Strictly greater than `min` (e.g. a thickness may not be 0).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclusive_min: bool,
    /// Default expression over rule symbols (e.g. `T/2`), used instead of
    /// `default` when a rule applies to the new feature
    /// ([`crate::Plugin::rule_symbols`], [`crate::expand_rule`]). The
    /// feature's slot is then bound to that expression, so editing the rule
    /// updates the feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
}

/// A named option (not bindable to expressions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceSchema {
    pub name: String,
    pub label: String,
    pub values: Vec<String>,
    pub default: String,
}

/// A read-only analysis (it returns a report, it changes nothing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisSchema {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub inputs: Vec<InputSchema>,
    #[serde(default)]
    pub params: Vec<ParamSchema>,
    #[serde(default)]
    pub choices: Vec<ChoiceSchema>,
}

fn valid_id(s: &str, extra: &[char]) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || extra.contains(&c))
}

fn valid_param_name(s: &str) -> bool {
    // Same shape as feature parameter slots: identifier characters only.
    !s.is_empty()
        && s.len() <= 64
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Manifest {
    /// Check the declaration itself: identifiers, unique names, sane
    /// defaults inside their bounds, angle names, choice defaults.
    pub fn validate(&self) -> Result<(), PluginError> {
        let bad = |m: String| Err(PluginError::Manifest(format!("{}: {m}", self.id)));
        if !valid_id(&self.id, &['.', '-']) {
            return bad("plugin id must be 1-64 of [a-z0-9_.-]".into());
        }
        if self.api_version != API_VERSION {
            return bad(format!(
                "plugin interface version {} is not supported (this build implements {API_VERSION})",
                self.api_version
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for f in &self.features {
            if !valid_id(&f.id, &[]) || !seen.insert(("f", f.id.clone())) {
                return bad(format!("feature id '{}' is invalid or repeated", f.id));
            }
            check_fields(&f.id, &f.inputs, &f.params, &f.choices).or_else(bad)?;
        }
        for a in &self.analyses {
            if !valid_id(&a.id, &[]) || !seen.insert(("a", a.id.clone())) {
                return bad(format!("analysis id '{}' is invalid or repeated", a.id));
            }
            check_fields(&a.id, &a.inputs, &a.params, &a.choices).or_else(bad)?;
        }
        Ok(())
    }

    pub fn feature(&self, id: &str) -> Option<&FeatureSchema> {
        self.features.iter().find(|f| f.id == id)
    }

    pub fn analysis(&self, id: &str) -> Option<&AnalysisSchema> {
        self.analyses.iter().find(|a| a.id == id)
    }
}

fn check_fields(
    owner: &str,
    inputs: &[InputSchema],
    params: &[ParamSchema],
    choices: &[ChoiceSchema],
) -> Result<(), String> {
    let mut names = std::collections::BTreeSet::new();
    for i in inputs {
        if !valid_param_name(&i.name) || !names.insert(i.name.clone()) {
            return Err(format!(
                "{owner}: input '{}' is invalid or repeated",
                i.name
            ));
        }
    }
    if inputs.iter().any(|i| i.kind == InputKind::Points)
        && !inputs.iter().any(|i| i.kind == InputKind::Face)
    {
        return Err(format!(
            "{owner}: a Points input needs a Face input for its plane"
        ));
    }
    for p in params {
        if !valid_param_name(&p.name) || !names.insert(p.name.clone()) {
            return Err(format!(
                "{owner}: parameter '{}' is invalid or repeated",
                p.name
            ));
        }
        if (p.kind == ParamKind::Angle) != p.name.ends_with("_deg") {
            return Err(format!(
                "{owner}: parameter '{}': angle parameters, and only they, end in _deg",
                p.name
            ));
        }
        check_value(p, p.default).map_err(|e| format!("{owner}: default of {e}"))?;
    }
    for c in choices {
        if !valid_param_name(&c.name) || !names.insert(c.name.clone()) {
            return Err(format!(
                "{owner}: choice '{}' is invalid or repeated",
                c.name
            ));
        }
        if c.values.is_empty() || !c.values.contains(&c.default) {
            return Err(format!(
                "{owner}: choice '{}' needs values including its default",
                c.name
            ));
        }
    }
    Ok(())
}

/// Check one value against its parameter's declaration; the error names the
/// parameter and the bound.
pub fn check_value(p: &ParamSchema, v: f64) -> Result<(), String> {
    if !v.is_finite() {
        return Err(format!("{}: value must be finite, got {v}", p.name));
    }
    if p.kind == ParamKind::Count && v.fract() != 0.0 {
        return Err(format!("{}: must be a whole number, got {v}", p.name));
    }
    if let Some(lo) = p.min {
        if p.exclusive_min && v <= lo {
            return Err(format!("{}: must be > {lo}, got {v}", p.name));
        }
        if v < lo {
            return Err(format!("{}: must be >= {lo}, got {v}", p.name));
        }
    }
    if let Some(hi) = p.max
        && v > hi
    {
        return Err(format!("{}: must be <= {hi}, got {v}", p.name));
    }
    Ok(())
}

/// Complete parameters and choices of a request.
pub type Completed = (BTreeMap<String, f64>, BTreeMap<String, String>);

/// Fill defaults and check given values against a feature's or analysis's
/// declaration: every given name must be declared, every value in range,
/// every choice one of its values. Returns the complete maps.
pub fn complete(
    owner: &str,
    params: &[ParamSchema],
    choices: &[ChoiceSchema],
    given: &BTreeMap<String, f64>,
    given_choices: &BTreeMap<String, String>,
) -> Result<Completed, PluginError> {
    for n in given.keys() {
        if !params.iter().any(|p| &p.name == n) {
            return Err(PluginError::Input(format!(
                "{owner}: unknown parameter '{n}'"
            )));
        }
    }
    for n in given_choices.keys() {
        if !choices.iter().any(|c| &c.name == n) {
            return Err(PluginError::Input(format!("{owner}: unknown choice '{n}'")));
        }
    }
    let mut out = BTreeMap::new();
    for p in params {
        let v = given.get(&p.name).copied().unwrap_or(p.default);
        check_value(p, v).map_err(|e| PluginError::Input(format!("{owner}.{e}")))?;
        out.insert(p.name.clone(), v);
    }
    let mut outc = BTreeMap::new();
    for c in choices {
        let v = given_choices
            .get(&c.name)
            .cloned()
            .unwrap_or(c.default.clone());
        if !c.values.contains(&v) {
            return Err(PluginError::Input(format!(
                "{owner}.{}: '{v}' is not one of {}",
                c.name,
                c.values.join(", ")
            )));
        }
        outc.insert(c.name.clone(), v);
    }
    Ok((out, outc))
}
