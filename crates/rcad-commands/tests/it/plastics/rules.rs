//! Plastic rules (parameters, bindings, propagation, materials), Design
//! Advice through the command path, references to plugin faces, save and
//! reload, and documents naming a plugin that is not installed.
use super::*;
use rcad_commands::plugins::{PluginAnalysisCmd, SetPluginDataCmd};
use serde_json::json;

fn rules(t: &str) -> Command {
    Command::SetPluginData(SetPluginDataCmd {
        plugin: "plastics".into(),
        data: json!({
            "rules": [{"name": "ABS", "material": "abs", "values": {"T": t, "DA": "1"}}],
            "bodies": {"1": "ABS"}
        }),
    })
}

fn web_cross(s: &mut Session, e: FeatureId) -> Response {
    let sk = line_sketch(s, end_cap(e), &[([-29.0, 0.0], [29.0, 0.0])]);
    ok(
        s,
        feature(
            "web",
            &[("sketch", sketch_in(sk))],
            &[("draft_deg", 0.0)],
            &[("direction", "flip"), ("extent", "to_next")],
        ),
    )
}

#[test]
fn features_bind_to_the_rule_and_follow_its_edits() {
    let (mut s, e) = open_box();
    ok(&mut s, rules("2 mm"));
    let sum = s.summary();
    let p = |n: &str| {
        sum.parameters
            .iter()
            .find(|p| p.name == n)
            .and_then(|p| p.value)
    };
    assert_eq!(p("ABS_T"), Some(2.0));
    assert_eq!(p("ABS_NR"), Some(1.0));
    assert_eq!(p("ABS_DA"), Some(1.0));
    let b = sum.bodies.iter().find(|b| b.id == BodyId(1)).unwrap();
    assert_eq!(
        b.material.as_ref().and_then(|m| m.catalog.as_deref()),
        Some("abs")
    );
    // The web's thickness is bound to ABS_T (not given in the command).
    let f = web_cross(&mut s, e).feature.unwrap();
    let fs = s
        .summary()
        .features
        .into_iter()
        .find(|x| x.id == f)
        .unwrap();
    assert_eq!(
        fs.expressions.get("thickness").map(String::as_str),
        Some("ABS_T")
    );
    assert!(
        !fs.expressions.contains_key("draft_deg"),
        "given values are not bound"
    );
    assert_close(
        vol(&s, BodyId(1)),
        BOX + 56.0 * 2.0 * 18.0,
        1e-6,
        "web at T = 2",
    );
    // Editing the rule regenerates the bound feature; undo restores.
    ok(&mut s, rules("3 mm"));
    assert_close(
        vol(&s, BodyId(1)),
        BOX + 56.0 * 3.0 * 18.0,
        1e-6,
        "web at T = 3",
    );
    ok(&mut s, Command::Undo);
    assert_close(vol(&s, BodyId(1)), BOX + 56.0 * 2.0 * 18.0, 1e-6, "undone");
    // Opting out of rules: plain defaults.
    let sk = line_sketch(&mut s, end_cap(e), &[([0.0, -19.0], [0.0, 19.0])]);
    let mut c = feature(
        "web",
        &[("sketch", sketch_in(sk))],
        &[],
        &[("direction", "flip")],
    );
    if let Command::PluginFeature(x) = &mut c {
        x.use_rules = false;
    }
    let g = ok(&mut s, c).feature.unwrap();
    let gs = s
        .summary()
        .features
        .into_iter()
        .find(|x| x.id == g)
        .unwrap();
    assert!(gs.expressions.is_empty());
}

#[test]
fn invalid_rule_data_is_refused_whole() {
    let (mut s, _) = open_box();
    for data in [
        json!({"rules": [{"name": "1abs"}]}),
        json!({"rules": [{"name": "ABS", "values": {"Q": "1"}}]}),
        json!({"rules": [{"name": "ABS", "values": {"T": "X*2"}}]}),
        json!({"rules": [], "bodies": {"1": "PC"}}),
        json!({"rules": [{"name": "ABS", "material": "unobtainium"}], "bodies": {"1": "ABS"}}),
        json!({"rules": "nope"}),
    ] {
        let r = s.execute(&Command::SetPluginData(SetPluginDataCmd {
            plugin: "plastics".into(),
            data: data.clone(),
        }));
        assert!(!r.ok, "{data}");
    }
    assert!(s.model().unwrap().document().plugin_data.is_empty());
    assert!(s.summary().parameters.is_empty());
}

#[test]
fn design_advice_reads_rule_limits_and_reports_walls_and_draft() {
    let (mut s, _) = open_box();
    ok(&mut s, rules("2 mm"));
    let r = ok(
        &mut s,
        Command::RunPluginAnalysis(PluginAnalysisCmd {
            plugin: "plastics".into(),
            analysis: "design_advice".into(),
            inputs: [("body".to_string(), PluginInput::Body { body: BodyId(1) })].into(),
            params: BTreeMap::new(),
            choices: BTreeMap::new(),
            chord: 0.05,
            use_rules: true,
        }),
    );
    let rep = r.plugin_report.unwrap();
    // Walls and floor are 2 thick; the rule's minimum draft (0.5°) flags
    // the vertical walls; nothing is too thin for TMIN = 1.
    assert_close(
        rep["thickness"]["min"].as_f64().unwrap(),
        2.0,
        1e-9,
        "min thickness",
    );
    assert_eq!(rep["draft"]["min_draft_deg"].as_f64(), Some(0.5));
    assert!(rep["summary"]["draft_area"].as_f64().unwrap() > 0.0);
    assert_eq!(rep["thickness"]["too_thin_area"].as_f64(), Some(0.0));
    assert_eq!(rep["undercut"]["area"].as_f64(), Some(0.0));
}

#[test]
fn later_features_reference_plugin_faces_by_operation() {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 60.0, 40.0, 4.0);
    let sk = points_sketch(&mut s, end_cap(e), &[[0.0, 0.0]]);
    let boss = ok(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[("height", 8.0), ("draft_deg", 0.0)],
            &[("hole", "none")],
        ),
    )
    .feature
    .unwrap();
    // A core Hole on the boss top (operation 0's end cap), 2 mm, 5 deep.
    let top = SketchPlane::Face(FaceRef {
        feature: boss,
        role: FaceRole::Plugin {
            op: 0,
            of: rcad_document::CopiedRole::EndCap,
        },
    });
    ok(
        &mut s,
        Command::Hole {
            name: None,
            plane: top,
            center: [0.0, 0.0],
            diameter: 2.0,
            depth: Some(5.0),
            counterbore: None,
            target: None,
        },
    );
    let pi = std::f64::consts::PI;
    let want = |h: f64| 60.0 * 40.0 * 4.0 + pi * 9.0 * h - pi * 5.0;
    assert_close(vol(&s, BodyId(1)), want(8.0), 1e-6, "hole on boss");
    // The hole follows the boss top when the boss grows.
    ok(
        &mut s,
        Command::SetParameter {
            feature: boss,
            name: "height".into(),
            value: 12.0,
        },
    );
    assert_close(
        vol(&s, BodyId(1)),
        want(12.0),
        1e-6,
        "after the height edit",
    );
    assert_eq!(
        probe(&s, BodyId(1), &[[0.0, 0.0, 15.0], [0.0, 0.0, 10.0]]),
        [Some(false), Some(true)]
    );
    // An operation the boss no longer makes is a broken reference, not another face.
    let bad = SketchPlane::Face(FaceRef {
        feature: boss,
        role: FaceRole::Plugin {
            op: 32,
            of: rcad_document::CopiedRole::EndCap,
        },
    });
    let r = s.execute(&Command::Hole {
        name: None,
        plane: bad,
        center: [0.0, 0.0],
        diameter: 1.0,
        depth: Some(1.0),
        counterbore: None,
        target: Some(BodyId(1)),
    });
    assert!(!r.ok);
    assert!(format!("{}", r.error.unwrap()).contains("no longer exists"));
}

#[test]
fn plugin_documents_save_as_v10_and_reload_identically() {
    let (mut s, e) = open_box();
    ok(&mut s, rules("2 mm"));
    web_cross(&mut s, e);
    let v = vol(&s, BodyId(1));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plastic.rcad.json");
    ok(&mut s, Command::Save { path: path.clone() });
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(raw["version"], 10);
    assert_eq!(
        raw["document"]["plugin_data"]["plastics"]["bodies"]["1"],
        "ABS"
    );
    let mut t = Session::new();
    ok(
        &mut t,
        Command::Open {
            path,
            safe_mode: false,
        },
    );
    assert_close(vol(&t, BodyId(1)), v, 1e-9, "reloaded");
    assert_eq!(
        t.model().unwrap().document().plugin_data,
        s.model().unwrap().document().plugin_data
    );
}

#[test]
fn a_feature_of_a_missing_plugin_fails_by_name_and_keeps_its_data() {
    let (mut s, e) = open_box();
    web_cross(&mut s, e);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.rcad.json");
    ok(&mut s, Command::Save { path: path.clone() });
    // Rename the plugin in the file: as if it were not installed.
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\"plastics\"", "\"not-installed\"");
    std::fs::write(&path, &text).unwrap();
    let mut t = Session::new();
    let r = t.execute(&Command::Open {
        path: path.clone(),
        safe_mode: false,
    });
    assert!(r.ok, "{:?}", r.error);
    let fail = t
        .model()
        .unwrap()
        .evaluation()
        .failures()
        .map(|f| format!("{:?}", f.health))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(fail.contains("not installed"), "{fail}");
    assert_close(vol(&t, BodyId(1)), BOX, 1e-9, "the body without the web");
    // Saving keeps the feature as it was.
    let out = dir.path().join("again.rcad.json");
    ok(&mut t, Command::Save { path: out.clone() });
    let a: serde_json::Value = serde_json::from_str(&text).unwrap();
    let b: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(a["document"]["features"], b["document"]["features"]);
}

#[test]
fn every_preset_rule_assigns_a_catalog_material() {
    for (name, mat, ..) in rcad_plugin_plastics::rules::PRESETS {
        let (mut s, _) = open_box();
        let rule = rcad_plugin_plastics::rules::Rule::preset(name).unwrap();
        ok(
            &mut s,
            Command::SetPluginData(SetPluginDataCmd {
                plugin: "plastics".into(),
                data: json!({"rules": [rule], "bodies": {"1": name}}),
            }),
        );
        let b = s
            .summary()
            .bodies
            .into_iter()
            .find(|b| b.id == BodyId(1))
            .unwrap();
        assert_eq!(
            b.material.and_then(|m| m.catalog).as_deref(),
            *mat,
            "{name}"
        );
    }
}

#[test]
fn redefining_a_plugin_feature_edits_choices_and_inputs_in_one_step() {
    let (mut s, e) = open_box();
    let f = web_cross(&mut s, e).feature.unwrap();
    let mut kind = s
        .model()
        .unwrap()
        .document()
        .feature(f)
        .unwrap()
        .kind
        .clone();
    if let rcad_document::FeatureKind::Plugin {
        params, choices, ..
    } = &mut kind
    {
        params.get_mut("thickness").unwrap().value = 4.0;
        choices.insert("thickness_side".into(), "one_side".into());
    }
    ok(
        &mut s,
        Command::RedefineFeature {
            feature: f,
            kind: Box::new(kind),
            name: None,
            expressions: None,
        },
    );
    assert_close(
        vol(&s, BodyId(1)),
        BOX + 56.0 * 4.0 * 18.0,
        1e-6,
        "redefined web",
    );
    // One-sided: the web lies on the left of the line (+y), not across it.
    assert_eq!(
        probe(&s, BodyId(1), &[[0.0, 3.5, 10.0], [0.0, -0.5, 10.0]]),
        [Some(true), Some(false)]
    );
    ok(&mut s, Command::Undo);
    assert_close(vol(&s, BodyId(1)), BOX + 56.0 * 2.0 * 18.0, 1e-6, "undone");
}

#[test]
fn rules_assigned_to_a_component_reach_its_bodies() {
    let (mut s, e) = open_box();
    let c: Command =
        serde_json::from_value(json!({"cmd": "create_component", "name": "Base", "bodies": [1]}))
            .unwrap();
    ok(&mut s, c);
    let cid = s.summary().assembly.components[0].id.0;
    ok(
        &mut s,
        Command::SetPluginData(SetPluginDataCmd {
            plugin: "plastics".into(),
            data: json!({
                "rules": [{"name": "PP", "material": "pp", "values": {"T": "1.5 mm"}}],
                "components": {cid.to_string(): "PP"}
            }),
        }),
    );
    let b = s
        .summary()
        .bodies
        .into_iter()
        .find(|b| b.id == BodyId(1))
        .unwrap();
    assert_eq!(b.material.and_then(|m| m.catalog).as_deref(), Some("pp"));
    // A web on the component's body binds to the component's rule.
    let f = web_cross(&mut s, e).feature.unwrap();
    let fs = s
        .summary()
        .features
        .into_iter()
        .find(|x| x.id == f)
        .unwrap();
    assert_eq!(
        fs.expressions.get("thickness").map(String::as_str),
        Some("PP_T")
    );
    assert_close(
        vol(&s, BodyId(1)),
        BOX + 56.0 * 1.5 * 18.0,
        1e-6,
        "web at PP_T",
    );
    // A body's own assignment wins over its component's.
    ok(
        &mut s,
        Command::SetPluginData(SetPluginDataCmd {
            plugin: "plastics".into(),
            data: json!({
                "rules": [{"name": "PP", "material": "pp", "values": {"T": "1.5 mm"}},
                          {"name": "ABS", "material": "abs", "values": {"T": "2 mm"}}],
                "components": {cid.to_string(): "PP"},
                "bodies": {"1": "ABS"}
            }),
        }),
    );
    let b = s
        .summary()
        .bodies
        .into_iter()
        .find(|b| b.id == BodyId(1))
        .unwrap();
    assert_eq!(b.material.and_then(|m| m.catalog).as_deref(), Some("abs"));
}
