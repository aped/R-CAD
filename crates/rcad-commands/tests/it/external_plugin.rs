//! An out-of-process plugin (examples/plugins/vent-slots, Python) used
//! through the document: its operations are applied and checked like any
//! feature's, regenerate on edits, and save/reload.
use rcad_commands::plugins::PluginFeatureCmd;
use rcad_commands::{Command, Session, SketchTemplate};
use rcad_document::{BodyId, FaceRef, FaceRole, PluginInput, SketchPlane};
use rcad_kernel::VolumeResult;
use std::f64::consts::PI;

fn python() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn vol(s: &Session) -> f64 {
    match s.summary().bodies[0].measurements.clone().unwrap().volume {
        VolumeResult::Value { volume, .. } => volume,
        v => panic!("{v:?}"),
    }
}

#[test]
fn vent_slots_from_an_external_plugin_cut_the_plate() {
    if !python() {
        eprintln!("python3 not found; skipping the external plugin test");
        return;
    }
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/vent-slots");
    let p = rcad_plugin::ProcessPlugin::load(&dir).unwrap();
    rcad_plugin::register(std::sync::Arc::new(p)).unwrap();
    let mut s = Session::new();
    let run = |s: &mut Session, c: Command| {
        let r = s.execute(&c);
        assert!(r.ok, "{c:?}: {:?}", r.error);
        r
    };
    run(&mut s, Command::NewDocument);
    let sk = run(
        &mut s,
        Command::CreateSketch {
            name: None,
            plane: SketchPlane::WorldXY,
            template: SketchTemplate::CenteredRectangle {
                width: 60.0,
                height: 40.0,
                center: None,
            },
        },
    )
    .feature
    .unwrap();
    let e = run(&mut s, Command::extrude(None, sk, 3.0))
        .feature
        .unwrap();
    let face = PluginInput::Face {
        plane: SketchPlane::Face(FaceRef {
            feature: e,
            role: FaceRole::ExtrudeEndCap,
        }),
    };
    let f = run(
        &mut s,
        Command::PluginFeature(PluginFeatureCmd {
            name: None,
            plugin: "vent-slots".into(),
            feature: "vent_slots".into(),
            inputs: [("face".to_string(), face)].into(),
            params: [
                ("count".to_string(), 5.0),
                ("slot_width".into(), 2.0),
                ("slot_length".into(), 20.0),
                ("pitch".into(), 4.0),
            ]
            .into(),
            choices: Default::default(),
            expressions: Default::default(),
            use_rules: true,
        }),
    )
    .feature
    .unwrap();
    // Through slots: n · (w·(L − w) + π w²/4) · thickness.
    let slot = |n: f64| n * (2.0 * 18.0 + PI) * 3.0;
    let plate = 60.0 * 40.0 * 3.0;
    let got = vol(&s);
    assert!((got - (plate - slot(5.0))).abs() < 1e-6, "{got}");
    run(
        &mut s,
        Command::SetParameter {
            feature: f,
            name: "count".into(),
            value: 3.0,
        },
    );
    let got = vol(&s);
    assert!((got - (plate - slot(3.0))).abs() < 1e-6, "{got}");
    // Save and reload through the external plugin again.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("vents.rcad.json");
    run(&mut s, Command::Save { path: path.clone() });
    let mut t = Session::new();
    run(
        &mut t,
        Command::Open {
            path,
            safe_mode: false,
        },
    );
    assert!((vol(&t) - (plate - slot(3.0))).abs() < 1e-6);
    let _ = BodyId(1);
}

#[test]
fn standoffs_written_with_the_python_helper_read_points() {
    if !python() {
        eprintln!("python3 not found; skipping the external plugin test");
        return;
    }
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/standoffs");
    let p = rcad_plugin::ProcessPlugin::load(&dir).unwrap();
    rcad_plugin::register(std::sync::Arc::new(p)).unwrap();
    let mut s = Session::new();
    let run = |s: &mut Session, c: Command| {
        let r = s.execute(&c);
        assert!(r.ok, "{c:?}: {:?}", r.error);
        r
    };
    run(&mut s, Command::NewDocument);
    let sk = run(
        &mut s,
        Command::CreateSketch {
            name: None,
            plane: SketchPlane::WorldXY,
            template: SketchTemplate::CenteredRectangle {
                width: 60.0,
                height: 40.0,
                center: None,
            },
        },
    )
    .feature
    .unwrap();
    let e = run(&mut s, Command::extrude(None, sk, 3.0))
        .feature
        .unwrap();
    let face = PluginInput::Face {
        plane: SketchPlane::Face(FaceRef {
            feature: e,
            role: FaceRole::ExtrudeEndCap,
        }),
    };
    let cmd = |pts: Vec<[f64; 2]>| {
        Command::PluginFeature(PluginFeatureCmd {
            name: None,
            plugin: "standoffs".into(),
            feature: "standoffs".into(),
            inputs: [
                ("face".to_string(), face.clone()),
                ("points".into(), PluginInput::Points { points: pts }),
            ]
            .into(),
            params: Default::default(),
            choices: Default::default(),
            expressions: Default::default(),
            use_rules: true,
        })
    };
    run(&mut s, cmd(vec![[-10.0, 0.0], [10.0, 0.0]]));
    let want = 60.0 * 40.0 * 3.0 + 2.0 * (PI * 6.25 * 6.0 - PI * 1.5625 * 5.0);
    assert!((vol(&s) - want).abs() < 1e-6, "{}", vol(&s));
    // A refusal from the plugin reaches the user and changes nothing.
    let r = s.execute(&cmd(vec![]));
    assert!(!r.ok);
    assert!(format!("{}", r.error.unwrap()).contains("at least one point"));
    assert!((vol(&s) - want).abs() < 1e-6);
}
