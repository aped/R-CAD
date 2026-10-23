//! The PLASTIC tab and the generic plugin dialogs through the real widgets:
//! the tab's groups and buttons, a Boss from the generic plugin feature
//! dialog (closed-form volume) and Edit Feature on it, plastic rules from a
//! preset assigned to a body (rule parameters, material, a new feature bound
//! to the rule with the hint shown), Design Advice's summary, and the
//! Plugins window and plugin feature menu.
use std::f64::consts::PI;
use std::time::{Duration, Instant};

use eframe::egui::{Key, Modifiers};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use rcad::shell::registry::{self, Group, Tab};
use rcad::{ExecMode, RcadApp};
use rcad_commands::{Command, SketchTemplate, end_cap_of};
use rcad_document::{BodyId, FeatureId, FeatureKind, SketchPlane};

fn harness() -> Harness<'static, RcadApp> {
    Harness::builder()
        .with_size([1400.0, 900.0])
        .with_max_steps(64)
        .build_ui_state(
            |ui, app: &mut RcadApp| app.ui(ui),
            RcadApp::new(ExecMode::Sync, None),
        )
}

fn settle(h: &mut Harness<'_, RcadApp>) {
    let start = Instant::now();
    loop {
        let _ = h.try_run();
        if h.state().idle() {
            let _ = h.try_run();
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(120), "not idle");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn click(h: &mut Harness<'_, RcadApp>, label: &str) {
    h.get_by_label(label).click();
    settle(h);
}

fn type_into(h: &mut Harness<'_, RcadApp>, label: &str, text: &str) {
    h.get_by_label(label).focus();
    let _ = h.try_run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    let _ = h.try_run();
    h.get_by_label(label).type_text(text);
    let _ = h.try_run();
}

fn volume(h: &Harness<'_, RcadApp>) -> f64 {
    h.state().volume().expect("volume")
}

fn assert_close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() <= 1e-6 * want.max(1.0),
        "{what}: {got} vs {want}"
    );
}

const PLATE: f64 = 60.0 * 40.0 * 4.0;

/// Plate 60×40×4 (Extrude1, body 1) with a sketch of fixed points at
/// (±15, 0) on its top face (Sketch2).
fn plate_with_points(h: &mut Harness<'_, RcadApp>) -> FeatureId {
    click(h, "New");
    h.state_mut().request(Command::CreateSketch {
        name: None,
        plane: SketchPlane::WorldXY,
        template: SketchTemplate::CenteredRectangle {
            width: 60.0,
            height: 40.0,
            center: None,
        },
    });
    h.state_mut()
        .request(Command::extrude(None, FeatureId(1), 4.0));
    settle(h);
    let plate = h.state().feature_by_name("Extrude1").unwrap().id;
    let mut sk = rcad_sketch::Sketch::default();
    for p in [[-15.0, 0.0], [15.0, 0.0]] {
        let id = sk.add_point(p[0], p[1]);
        sk.constraints.push(rcad_sketch::Constraint::FixedPoint {
            p: id,
            x: p[0],
            y: p[1],
        });
    }
    h.state_mut().request(Command::CreateSketch {
        name: None,
        plane: end_cap_of(plate),
        template: SketchTemplate::Custom { sketch: sk },
    });
    settle(h);
    assert_close(volume(h), PLATE, "plate");
    h.state().summary().features.last().unwrap().id
}

/// Two Ø6 pilot-hole bosses `height` tall without draft (Ø2.5 × 6 holes).
fn bosses(height: f64) -> f64 {
    PLATE + 2.0 * (PI * 9.0 * height - PI * 1.25f64.powi(2) * 6.0)
}

fn plugin_def(h: &Harness<'_, RcadApp>, name: &str) -> FeatureKind {
    h.state()
        .feature_by_name(name)
        .and_then(|f| f.definition.clone())
        .unwrap_or_else(|| panic!("{name} has no definition"))
}

fn param(def: &FeatureKind, n: &str) -> f64 {
    match def {
        FeatureKind::Plugin { params, .. } => params[n].value,
        k => panic!("not a plugin feature: {k:?}"),
    }
}

#[test]
fn plastic_tab_shows_setup_create_and_inspect() {
    assert_eq!(
        registry::groups(Tab::Plastic),
        [Group::Setup, Group::Create, Group::Inspect]
    );
    let mut h = harness();
    plate_with_points(&mut h);
    click(&mut h, "PLASTIC tab");
    for l in [
        "SETUP",
        "CREATE",
        "INSPECT",
        "Manage Plastic Rules…",
        "Assign Plastic Rule…",
        "Boss…",
        "Rib…",
        "Web…",
        "Snap Fit…",
        "Lip…",
        "Rest…",
        "Design Advice…",
    ] {
        let n = h.get_by_label(l);
        assert!(!n.accesskit_node().is_disabled(), "{l} is disabled");
    }
    // The SOLID ribbon gives way.
    assert!(h.query_by_label("Extrude…").is_none());
    click(&mut h, "SOLID tab");
}

#[test]
fn boss_from_the_plugin_dialog_and_edit_feature() {
    let mut h = harness();
    let sk = plate_with_points(&mut h);
    click(&mut h, "PLASTIC tab");
    click(&mut h, "Boss…");
    // The dialog is built from the manifest: inputs, unit fields, choices.
    h.get_by_label("Boss");
    h.get_by_label("Height (mm)");
    h.get_by_label("Draft (deg)");
    // The last sketch is preselected; choose it explicitly anyway.
    click(&mut h, "Sketch points");
    click(&mut h, "Sketch points Sketch2");
    type_into(&mut h, "Height (mm)", "8");
    type_into(&mut h, "Draft (deg)", "0");
    click(&mut h, "OK");
    let def = plugin_def(&h, "Boss1");
    let FeatureKind::Plugin {
        plugin,
        feature,
        inputs,
        ..
    } = &def
    else {
        panic!()
    };
    assert_eq!((plugin.as_str(), feature.as_str()), ("plastics", "boss"));
    assert_eq!(
        inputs.get("sketch"),
        Some(&rcad_document::PluginInput::Sketch { sketch: sk })
    );
    assert_eq!(param(&def, "height"), 8.0);
    assert_eq!(param(&def, "draft_deg"), 0.0);
    assert_close(volume(&h), bosses(8.0), "bosses 8 high");
    // The browser shows the feature type's name.
    h.get_by_label("Boss1");
    // Edit Feature reopens the dialog prefilled; OK redefines in place.
    let n = h.state().summary().features.len();
    h.get_by_label("Boss1").click_secondary();
    settle(&mut h);
    click(&mut h, "Edit Feature Boss1");
    h.get_by_label("Edit Boss1");
    let field = format!("{:?}", h.get_by_label("Height (mm)").accesskit_node());
    assert!(field.contains("\"8\""), "{field}");
    type_into(&mut h, "Height (mm)", "10");
    click(&mut h, "OK");
    assert_eq!(param(&plugin_def(&h, "Boss1"), "height"), 10.0);
    assert_eq!(h.state().summary().features.len(), n);
    assert_close(volume(&h), bosses(10.0), "bosses 10 high");
    // One undo step back to 8.
    h.state_mut().request(Command::Undo);
    settle(&mut h);
    assert_close(volume(&h), bosses(8.0), "undone");
}

#[test]
fn rules_from_a_preset_assigned_to_a_body_bind_new_features() {
    let mut h = harness();
    plate_with_points(&mut h);
    click(&mut h, "PLASTIC tab");
    click(&mut h, "Manage Plastic Rules…");
    // ABS is the first preset.
    click(&mut h, "Add rule");
    let t = format!("{:?}", h.get_by_label("Rule 1 T").accesskit_node());
    assert!(t.contains("2 mm"), "{t}");
    click(&mut h, "Apply rules");
    let p = |h: &Harness<'_, RcadApp>, n: &str| {
        h.state()
            .summary()
            .parameters
            .iter()
            .find(|p| p.name == n)
            .and_then(|p| p.value)
    };
    assert_eq!(p(&h, "ABS_T"), Some(2.0));
    assert_eq!(p(&h, "ABS_DA"), Some(1.0));
    // Assign the rule to the plate (preselected: the first body and rule).
    click(&mut h, "Assign Plastic Rule…");
    click(&mut h, "Assign rule");
    let body = h
        .state()
        .summary()
        .bodies
        .iter()
        .find(|b| b.id == BodyId(1))
        .cloned()
        .unwrap();
    assert_eq!(
        body.material.as_ref().and_then(|m| m.catalog.as_deref()),
        Some("abs")
    );
    assert_eq!(
        h.state().plugin_data("plastics").unwrap()["bodies"]["1"],
        "ABS"
    );
    // A boss on the plate: the untouched draft follows the rule.
    click(&mut h, "Boss…");
    assert!(
        h.query_by_label_contains("rule: DA = ABS_DA").is_some(),
        "rule hint not shown"
    );
    click(&mut h, "OK");
    let f = h.state().feature_by_name("Boss1").unwrap();
    assert_eq!(
        f.expressions.get("draft_deg").map(String::as_str),
        Some("ABS_DA")
    );
    assert!(!f.expressions.contains_key("height"));
    // Editing the rule's value in the window updates the bound feature.
    click(&mut h, "Manage Plastic Rules…");
    type_into(&mut h, "Rule 1 DA", "2");
    click(&mut h, "Apply rules");
    assert_eq!(p(&h, "ABS_DA"), Some(2.0));
    let def = plugin_def(&h, "Boss1");
    assert_eq!(param(&def, "draft_deg"), 2.0);
}

#[test]
fn design_advice_runs_and_shows_the_summary() {
    let mut h = harness();
    plate_with_points(&mut h);
    click(&mut h, "PLASTIC tab");
    click(&mut h, "Design Advice…");
    click(&mut h, "Run analysis");
    // A 4 mm plate: 4 thick across its faces (wider across the sides),
    // so the default 3 mm maximum flags area too thick.
    let rep = h.state().plugin_report().expect("report").clone();
    let min = rep["summary"]["min_thickness"].as_f64().unwrap();
    assert!((min - 4.0).abs() < 1e-6, "min thickness {min}");
    assert!(rep["summary"]["too_thick_area"].as_f64().unwrap() > 0.0);
    h.get_by_label("Minimum thickness");
    h.get_by_label("Area too thick");
    assert!(h.query_by_label_contains("Approximate").is_some());
    // Alerts list the faces; clicking one selects it in the view.
    click(&mut h, "Alert 1");
    assert!(h.state().selected_face().is_some());
}

#[test]
fn plugins_window_and_feature_menu_list_the_installed_plugins() {
    let mut h = harness();
    plate_with_points(&mut h);
    click(&mut h, "UTILITIES tab");
    click(&mut h, "Plugin features");
    click(&mut h, "Plastics: Rib…");
    h.get_by_label("Rib");
    click(&mut h, "Cancel");
    click(&mut h, "Plugins…");
    assert!(h.query_by_label_contains("plastics · ").is_some());
    assert!(h.query_by_label_contains("built-in").is_some());
    assert!(h.query_by_label_contains("Boss, Rib, Web").is_some());
}
