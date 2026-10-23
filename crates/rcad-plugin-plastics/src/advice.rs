//! Design Advice (docs/PLASTICS.md "Design advice"): wall thickness,
//! draft, undercuts and knife edges of a body for a pull direction, from the
//! exact face kinds plus a closed tessellation ([`rcad_plugin::BodyMesh`]).
//! Read-only; the report is JSON.
use crate::{choice, input, param};
use rcad_plugin::schema::AnalysisSchema;
use rcad_plugin::{AnalysisRequest, InputKind, ParamKind, PluginError};

pub fn schema() -> AnalysisSchema {
    AnalysisSchema {
        id: "design_advice".into(),
        name: "Design Advice".into(),
        description: "Wall thickness, draft, undercuts and knife edges for a pull direction".into(),
        inputs: vec![
            input("body", "Body", InputKind::Body, false),
            input(
                "pull",
                "Pull direction (face normal)",
                InputKind::Face,
                true,
            ),
        ],
        params: vec![
            param(
                "min_thickness",
                "Minimum thickness",
                ParamKind::Length,
                1.0,
                Some((0.0, true)),
                None,
                Some("TMIN"),
            ),
            param(
                "max_thickness",
                "Maximum thickness",
                ParamKind::Length,
                3.0,
                Some((0.0, true)),
                None,
                Some("TMAX"),
            ),
            param(
                "variation",
                "Allowed thickness variation",
                ParamKind::Ratio,
                0.15,
                Some((0.0, false)),
                None,
                Some("TV"),
            ),
            param(
                "min_draft_deg",
                "Minimum draft",
                ParamKind::Angle,
                0.5,
                Some((0.0, false)),
                Some(45.0),
                Some("MDA"),
            ),
            param(
                "knife_edge",
                "Knife edge threshold",
                ParamKind::Length,
                1.0,
                Some((0.0, true)),
                None,
                Some("KE"),
            ),
        ],
        choices: vec![choice(
            "pull_axis",
            "Pull axis",
            &["+z", "-z", "+x", "-x", "+y", "-y"],
        )],
    }
}

pub fn analyze(req: &AnalysisRequest) -> Result<serde_json::Value, PluginError> {
    let _ = req;
    Err(PluginError::Unsupported(
        "design advice: not implemented yet".into(),
    ))
}
