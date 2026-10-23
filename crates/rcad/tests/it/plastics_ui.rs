//! The PLASTIC tab and the generic plugin dialogs through the real widgets:
//! the tab's groups and buttons, a Boss from the generic plugin feature
//! dialog (closed-form volume) and Edit Feature on it, plastic rules from a
//! preset assigned to a body (rule parameters, material, a new feature bound
//! to the rule with the hint shown), Design Advice's summary, the Plugins
//! window and plugin menu, Draft / Accessibility / Minimum Radius analyses
//! (closed-form areas and radii) with their colour map on the model, a core
//! Shell bound to the body's rule from its dialog, and a rule assigned to an
//! assembly component.
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

/// A `w`×`d`×`t` plate centred on the origin, pulled +z (Extrude1, body 1).
fn plate(h: &mut Harness<'_, RcadApp>, w: f64, d: f64, t: f64) {
    click(h, "New");
    h.state_mut().request(Command::CreateSketch {
        name: None,
        plane: SketchPlane::WorldXY,
        template: SketchTemplate::CenteredRectangle {
            width: w,
            height: d,
            center: None,
        },
    });
    h.state_mut()
        .request(Command::extrude(None, FeatureId(1), t));
    settle(h);
    assert_close(volume(h), w * d * t, "plate");
}

fn report(h: &Harness<'_, RcadApp>) -> serde_json::Value {
    h.state().plugin_report().expect("report").clone()
}

fn sum(rep: &serde_json::Value, k: &str) -> f64 {
    rep["summary"][k]
        .as_f64()
        .unwrap_or_else(|| panic!("summary.{k}: {rep}"))
}

/// ABS from its preset, assigned to body 1.
fn abs_rule_on_the_plate(h: &mut Harness<'_, RcadApp>) {
    click(h, "PLASTIC tab");
    click(h, "Manage Plastic Rules…");
    click(h, "Add rule");
    click(h, "Apply rules");
    click(h, "Assign Plastic Rule…");
    click(h, "Assign rule");
    assert_eq!(
        h.state().plugin_data("plastics").unwrap()["bodies"]["1"],
        "ABS"
    );
}

#[test]
fn inspect_and_the_plugin_menu_list_every_analysis() {
    let mut h = harness();
    plate(&mut h, 40.0, 30.0, 4.0);
    let plastics = h
        .state()
        .plugins()
        .iter()
        .find(|p| p.manifest.id == "plastics")
        .cloned()
        .expect("plastics plugin");
    assert!(plastics.manifest.analyses.len() >= 4);
    click(&mut h, "PLASTIC tab");
    for a in &plastics.manifest.analyses {
        let l = format!("{}…", a.name);
        let n = h.get_by_label(&l);
        assert!(!n.accesskit_node().is_disabled(), "{l} is disabled");
    }
    click(&mut h, "UTILITIES tab");
    click(&mut h, "Plugin features");
    for p in h.state().plugins().to_vec() {
        for a in &p.manifest.analyses {
            h.get_by_label(&format!("{}: {}…", p.manifest.name, a.name));
        }
    }
    click(&mut h, "Plastics: Minimum Radius Analysis…");
    h.get_by_label("Minimum Radius Analysis");
}

#[test]
fn draft_analysis_of_a_plate_and_its_colour_map() {
    let mut h = harness();
    plate(&mut h, 40.0, 30.0, 4.0);
    click(&mut h, "PLASTIC tab");
    click(&mut h, "Draft Analysis…");
    click(&mut h, "Run analysis");
    let rep = report(&h);
    // Pulled +z: the top opens towards the pull, the bottom away from it,
    // the four vertical walls (2·(40+30)·4) have no draft.
    assert_close(sum(&rep, "positive_area"), 1200.0, "positive");
    assert_close(sum(&rep, "negative_area"), 1200.0, "negative");
    assert_close(sum(&rep, "insufficient_area"), 560.0, "insufficient");
    h.get_by_label("Insufficient draft area");
    // (also the vertical wall area)
    assert_eq!(h.query_all_by_label("560 mm²").count(), 2);
    h.get_by_label("All summary values");
    // Show on model: every triangle of the body's tessellation coloured.
    click(&mut h, "Show on model");
    let tris = rep["mesh"]["triangles"].as_u64().unwrap() as usize;
    {
        use rcad::plugin_ui::colour_map::Class;
        let m = h.state().colour_map().expect("colour map");
        assert_eq!(m.triangles, tris);
        assert_eq!(m.tris.len(), tris);
        assert!(m.count(Class::Good) >= 2 && m.count(Class::Bad) >= 2);
        assert!(m.count(Class::Insufficient) >= 8);
    }
    assert!(h.query_by_label_contains("unavailable").is_none());
    click(&mut h, "Hide colour map");
    assert!(h.state().colour_map().is_none());
    // Shown again, then dropped when the model changes.
    click(&mut h, "Show on model");
    assert!(h.state().colour_map().is_some());
    h.state_mut().request(Command::SetParameter {
        feature: FeatureId(2),
        name: "distance".into(),
        value: 5.0,
    });
    settle(&mut h);
    assert!(h.state().colour_map().is_none());
}

#[test]
fn accessibility_from_above_reaches_the_top_face() {
    let mut h = harness();
    plate(&mut h, 40.0, 30.0, 4.0);
    click(&mut h, "PLASTIC tab");
    click(&mut h, "Accessibility Analysis…");
    click(&mut h, "Run analysis");
    let rep = report(&h);
    assert_close(sum(&rep, "accessible_area"), 1200.0, "accessible");
    assert_close(sum(&rep, "away_area"), 1200.0, "away");
    assert_close(sum(&rep, "vertical_area"), 560.0, "edge-on");
    assert_close(sum(&rep, "blocked_area"), 0.0, "blocked");
    h.get_by_label("Accessible area");
    // (also the area facing away)
    assert_eq!(h.query_all_by_label("1200 mm²").count(), 2);
    click(&mut h, "Show on model");
    use rcad::plugin_ui::colour_map::Class;
    let m = h.state().colour_map().expect("colour map");
    assert_eq!(
        m.triangles,
        rep["mesh"]["triangles"].as_u64().unwrap() as usize
    );
    assert_eq!(m.count(Class::Bad), 0);
    assert!(m.count(Class::Good) >= 2);
}

#[test]
fn minimum_radius_of_a_thin_pin() {
    let mut h = harness();
    click(&mut h, "New");
    h.state_mut().request(Command::CreateSketch {
        name: None,
        plane: SketchPlane::WorldXY,
        template: SketchTemplate::CenteredCircle {
            diameter: 1.0,
            center: None,
        },
    });
    h.state_mut()
        .request(Command::extrude(None, FeatureId(1), 4.0));
    settle(&mut h);
    click(&mut h, "PLASTIC tab");
    click(&mut h, "Minimum Radius Analysis…");
    click(&mut h, "Run analysis");
    let rep = report(&h);
    // Ø1 pin: its side (one or more cylinder faces) has radius 0.5, below
    // the 1 mm default; the flat ends have none.
    assert_close(sum(&rep, "min_radius"), 0.5, "smallest radius");
    let faces = rep["faces"].as_array().unwrap();
    let flagged: Vec<_> = faces.iter().filter(|f| f["below"] == true).collect();
    assert_eq!(flagged.len() as f64, sum(&rep, "flagged_faces"));
    assert!(!flagged.is_empty());
    for f in &flagged {
        assert_eq!(f["surface"], "cylinder", "{f}");
        assert_close(f["radius"].as_f64().unwrap(), 0.5, "face radius");
    }
    // The side's area 4π within the tessellation's approximation.
    let side = sum(&rep, "flagged_area");
    let want = PI * 4.0;
    assert!(
        (side - want).abs() < 0.02 * want,
        "side area {side} vs {want}"
    );
    h.get_by_label("Smallest radius");
    h.get_by_label("0.5 mm");
    click(&mut h, "Show on model");
    use rcad::plugin_ui::colour_map::Class;
    let m = h.state().colour_map().expect("colour map");
    let below = rep["triangles"]["radius"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r.as_f64().is_some_and(|r| r < 1.0))
        .count();
    assert!(below > 0);
    assert_eq!(m.count(Class::Bad), below);
    assert_eq!(m.tris.len(), below);
}

/// The top face (z = `t`) of the plate, selected.
fn select_top(h: &mut Harness<'_, RcadApp>, t: f64) {
    let top = h
        .state()
        .faces()
        .iter()
        .find(|f| {
            f.plane
                .is_some_and(|p| p.normal[2] > 0.99 && (p.point[2] - t).abs() < 1e-9)
        })
        .map(|f| f.group)
        .expect("top face");
    h.state_mut().select_face(Some(top));
    let _ = h.try_run();
}

#[test]
fn shell_with_the_rule_checkbox_binds_its_thickness() {
    let mut h = harness();
    plate(&mut h, 60.0, 40.0, 4.0);
    abs_rule_on_the_plate(&mut h);
    click(&mut h, "SOLID tab");
    select_top(&mut h, 4.0);
    click(&mut h, "Shell");
    h.get_by_label_contains("Open face: Extrude1 end cap");
    h.get_by_label("Use plastic rule (ABS_T)");
    click(&mut h, "OK");
    let f = h.state().feature_by_name("Shell1").expect("shell");
    assert_eq!(
        f.expressions.get("thickness").map(String::as_str),
        Some("ABS_T")
    );
    // 2 mm walls and floor (ABS_T = 2), top open.
    assert_close(volume(&h), 60.0 * 40.0 * 4.0 - 56.0 * 36.0 * 2.0, "shelled");
    // The rule's thickness drives the shell.
    click(&mut h, "PLASTIC tab");
    click(&mut h, "Manage Plastic Rules…");
    type_into(&mut h, "Rule 1 T", "1.5");
    click(&mut h, "Apply rules");
    assert_close(
        volume(&h),
        60.0 * 40.0 * 4.0 - 57.0 * 37.0 * 2.5,
        "shelled 1.5",
    );
}

#[test]
fn a_typed_shell_thickness_stays_a_value() {
    let mut h = harness();
    plate(&mut h, 60.0, 40.0, 4.0);
    abs_rule_on_the_plate(&mut h);
    click(&mut h, "SOLID tab");
    select_top(&mut h, 4.0);
    click(&mut h, "Shell");
    h.get_by_label("Use plastic rule (ABS_T)");
    // (the face inspector has a thickness field of the same name)
    h.state_mut().select_face(None);
    let _ = h.try_run();
    type_into(&mut h, "Shell thickness (mm)", "1");
    click(&mut h, "OK");
    let f = h.state().feature_by_name("Shell1").expect("shell");
    assert!(
        !f.expressions.contains_key("thickness"),
        "{:?}",
        f.expressions
    );
    assert_close(
        volume(&h),
        60.0 * 40.0 * 4.0 - 58.0 * 38.0 * 3.0,
        "1 mm shell",
    );
}

#[test]
fn a_rule_assigned_to_a_component() {
    let mut h = harness();
    plate(&mut h, 40.0, 30.0, 4.0);
    h.state_mut().request(Command::CreateComponent {
        name: "Cover".into(),
        bodies: vec![BodyId(1)],
        part_number: None,
        occurrences: vec![],
    });
    settle(&mut h);
    click(&mut h, "PLASTIC tab");
    click(&mut h, "Manage Plastic Rules…");
    click(&mut h, "Add rule");
    click(&mut h, "Apply rules");
    click(&mut h, "Assign Plastic Rule…");
    click(&mut h, "Assign to");
    click(&mut h, "Assign component Cover");
    h.get_by_label_contains("Current rule of this component: none");
    click(&mut h, "Assign rule");
    let d = h.state().plugin_data("plastics").unwrap().clone();
    assert_eq!(d["components"]["1"], "ABS", "{d}");
    assert!(d["bodies"].get("1").is_none(), "{d}");
    // The component's bodies get the rule's material.
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
}
