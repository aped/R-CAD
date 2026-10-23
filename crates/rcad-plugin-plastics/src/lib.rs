//! Plastic part design (docs/PLASTICS.md): the first R-CAD plugin and the
//! reference for writing others. Features follow Autodesk Fusion's Plastic
//! tab (Boss, Rib, Web, Snap Fit, Lip, Rest, plastic rules, Design Advice) as
//! the behavioural reference; every feature is built from checked prism
//! operations ([`rcad_plugin::Op::Prism`]), and what that cannot express
//! exactly is refused or documented as a deviation, never approximated.
pub mod advice;
pub mod analyses;
mod boss;
mod lip;
mod mesh_query;
mod rest;
mod ribweb;
pub mod rules;
mod snap;
mod util;

use rcad_plugin::schema::{ChoiceSchema, InputSchema, ParamSchema};
use rcad_plugin::{
    AnalysisRequest, BuildRequest, DataEffects, InputKind, Manifest, Op, ParamKind, Plugin,
    PluginError,
};
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub const ID: &str = "plastics";

/// The plastics plugin.
#[derive(Debug, Default)]
pub struct Plastics;

pub(crate) fn input(name: &str, label: &str, kind: InputKind, optional: bool) -> InputSchema {
    InputSchema {
        name: name.into(),
        label: label.into(),
        kind,
        optional,
    }
}

/// A parameter: `min` exclusive when `strict`; `rule`: default template.
pub(crate) fn param(
    name: &str,
    label: &str,
    kind: ParamKind,
    default: f64,
    min: Option<(f64, bool)>,
    max: Option<f64>,
    rule: Option<&str>,
) -> ParamSchema {
    ParamSchema {
        name: name.into(),
        label: label.into(),
        kind,
        default,
        min: min.map(|m| m.0),
        exclusive_min: min.is_some_and(|m| m.1),
        max,
        rule: rule.map(Into::into),
    }
}

pub(crate) fn choice(name: &str, label: &str, values: &[&str]) -> ChoiceSchema {
    ChoiceSchema {
        name: name.into(),
        label: label.into(),
        values: values.iter().map(|v| v.to_string()).collect(),
        default: values[0].into(),
    }
}

fn manifest() -> Manifest {
    Manifest {
        id: ID.into(),
        name: "Plastics".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        api_version: rcad_plugin::API_VERSION,
        description: "Plastic part design: rules, boss, rib, web, snap fit, lip/groove, rest, \
                      design advice, draft, accessibility and minimum radius analyses"
            .into(),
        features: vec![
            boss::schema(),
            ribweb::rib_schema(),
            ribweb::web_schema(),
            snap::schema(),
            lip::schema(),
            rest::schema(),
        ],
        analyses: vec![
            advice::schema(),
            analyses::draft_schema(),
            analyses::accessibility_schema(),
            analyses::minimum_radius_schema(),
        ],
    }
}

impl Plugin for Plastics {
    fn manifest(&self) -> &Manifest {
        static M: OnceLock<Manifest> = OnceLock::new();
        M.get_or_init(manifest)
    }

    fn build(&self, req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
        match req.feature.as_str() {
            "boss" => boss::build(req),
            "rib" => ribweb::build_rib(req),
            "web" => ribweb::build_web(req),
            "snap_fit" => snap::build(req),
            "lip" => lip::build(req),
            "rest" => rest::build(req),
            f => Err(PluginError::NotFound(format!("plastics: no feature '{f}'"))),
        }
    }

    fn analyze(&self, req: &AnalysisRequest) -> Result<serde_json::Value, PluginError> {
        match req.analysis.as_str() {
            "design_advice" => advice::analyze(req),
            "draft_analysis" => analyses::draft_analysis(req),
            "accessibility" => analyses::accessibility(req),
            "minimum_radius" => analyses::minimum_radius(req),
            a => Err(PluginError::NotFound(format!(
                "plastics: no analysis '{a}'"
            ))),
        }
    }

    fn validate_data(&self, data: &serde_json::Value) -> Result<(), PluginError> {
        rules::PlasticData::parse(data)?.validate()
    }

    fn data_effects(&self, data: &serde_json::Value) -> DataEffects {
        rules::PlasticData::parse(data)
            .map(|d| d.effects())
            .unwrap_or_default()
    }

    fn rule_symbols(
        &self,
        data: &serde_json::Value,
        body: Option<u32>,
    ) -> BTreeMap<String, String> {
        rules::PlasticData::parse(data)
            .ok()
            .and_then(|d| d.rule_of(body).map(rules::Rule::symbols))
            .unwrap_or_default()
    }

    fn core_rule_templates(&self, kind: &str) -> BTreeMap<String, String> {
        let t: &[(&str, &str)] = match kind {
            // Wall thickness, nominal radius, draft (docs/PLASTICS.md).
            "shell" | "shell_face" => &[("thickness", "T")],
            "fillet" | "fillet_loop" => &[("radius", "NR")],
            "extrude" => &[("taper_deg", "DA"), ("thin", "T")],
            "thicken" => &[("distance", "T")],
            _ => &[],
        };
        t.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn rule_symbols_in(
        &self,
        data: &serde_json::Value,
        at: &rcad_plugin::RuleContext,
    ) -> BTreeMap<String, String> {
        rules::PlasticData::parse(data)
            .ok()
            .and_then(|d| d.rule_at(at).map(rules::Rule::symbols))
            .unwrap_or_default()
    }

    fn data_effects_in(
        &self,
        data: &serde_json::Value,
        doc: &rcad_plugin::DocContext,
    ) -> DataEffects {
        rules::PlasticData::parse(data)
            .map(|d| d.effects_in(doc))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid() {
        Plastics.manifest().validate().unwrap();
        let ids: Vec<&str> = Plastics
            .manifest()
            .features
            .iter()
            .map(|f| f.id.as_str())
            .collect();
        assert_eq!(ids, ["boss", "rib", "web", "snap_fit", "lip", "rest"]);
    }

    #[test]
    fn rule_templates_use_rule_symbols_only() {
        let sym = rules::Rule::preset("abs").unwrap().symbols();
        let m = Plastics.manifest();
        let all = m
            .features
            .iter()
            .flat_map(|f| f.params.iter())
            .chain(m.analyses.iter().flat_map(|a| a.params.iter()));
        for p in all {
            if let Some(t) = &p.rule {
                assert!(
                    rcad_plugin::expand_rule(t, &sym).is_some(),
                    "{}: template '{t}' uses an unknown symbol",
                    p.name
                );
            }
        }
    }
}
