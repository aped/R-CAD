//! Plastics plugin features through the public command path
//! (docs/PLASTICS.md): closed-form volumes, point probes, parameter edits,
//! rule bindings, refusals, undo and save/reload.
mod boss;
mod lip;
mod rest;
mod ribweb;
mod snap;

use rcad_commands::plugins::PluginFeatureCmd;
use rcad_commands::{Command, Response, Session, SketchTemplate};
use rcad_document::{BodyId, FaceRef, FaceRole, FeatureId, PlaneBase, PluginInput, SketchPlane};
use rcad_kernel::VolumeResult;
use std::collections::BTreeMap;

pub fn ok(s: &mut Session, c: Command) -> Response {
    let r = s.execute(&c);
    assert!(r.ok, "{c:?}: {:?}", r.error);
    r
}

pub fn err(s: &mut Session, c: Command) -> String {
    let r = s.execute(&c);
    assert!(!r.ok, "expected a refusal: {c:?}");
    format!("{}", r.error.unwrap())
}

pub fn vol(s: &Session, body: BodyId) -> f64 {
    let b = s
        .summary()
        .bodies
        .into_iter()
        .find(|b| b.id == body)
        .unwrap_or_else(|| panic!("no body {body}"));
    match b.measurements.unwrap().volume {
        VolumeResult::Value { volume, .. } => volume,
        v => panic!("{v:?}"),
    }
}

pub fn probe(s: &Session, body: BodyId, pts: &[[f64; 3]]) -> Vec<Option<bool>> {
    let m = s.model().unwrap();
    let b = m
        .evaluation()
        .body_by_id(body)
        .unwrap()
        .geometry
        .brep()
        .unwrap();
    rcad_kernel::point_membership(b, pts).unwrap()
}

pub fn assert_close(got: f64, want: f64, rel: f64, what: &str) {
    assert!(
        (got - want).abs() <= rel * want.abs().max(1.0),
        "{what}: got {got}, want {want} (diff {})",
        got - want
    );
}

/// A centred `w` × `h` plate `t` thick on `plane` (New Body); returns the extrusion.
pub fn plate(s: &mut Session, plane: SketchPlane, w: f64, h: f64, t: f64) -> FeatureId {
    let sk = ok(
        s,
        Command::CreateSketch {
            name: None,
            plane,
            template: SketchTemplate::CenteredRectangle {
                width: w,
                height: h,
                center: None,
            },
        },
    )
    .feature
    .unwrap();
    ok(s, Command::extrude(None, sk, t)).feature.unwrap()
}

/// A sketch on `plane` holding only fixed points.
pub fn points_sketch(s: &mut Session, plane: SketchPlane, pts: &[[f64; 2]]) -> FeatureId {
    let mut sk = rcad_sketch::Sketch::default();
    for p in pts {
        let id = sk.add_point(p[0], p[1]);
        sk.constraints.push(rcad_sketch::Constraint::FixedPoint {
            p: id,
            x: p[0],
            y: p[1],
        });
    }
    ok(
        s,
        Command::CreateSketch {
            name: None,
            plane,
            template: SketchTemplate::Custom { sketch: sk },
        },
    )
    .feature
    .unwrap()
}

pub fn end_cap(f: FeatureId) -> SketchPlane {
    SketchPlane::Face(FaceRef {
        feature: f,
        role: FaceRole::ExtrudeEndCap,
    })
}

pub fn start_cap(f: FeatureId) -> FaceRef {
    FaceRef {
        feature: f,
        role: FaceRole::ExtrudeStartCap,
    }
}

pub fn offset_xy(d: f64) -> SketchPlane {
    SketchPlane::Offset {
        base: PlaneBase::WorldXY,
        distance: d,
        angle: 0.0,
    }
}

/// A plastics feature command.
pub fn feature(
    feature: &str,
    inputs: &[(&str, PluginInput)],
    params: &[(&str, f64)],
    choices: &[(&str, &str)],
) -> Command {
    Command::PluginFeature(PluginFeatureCmd {
        name: None,
        plugin: "plastics".into(),
        feature: feature.into(),
        inputs: inputs
            .iter()
            .map(|(n, i)| (n.to_string(), i.clone()))
            .collect(),
        params: params.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        choices: choices
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect(),
        expressions: BTreeMap::new(),
        use_rules: true,
    })
}

pub fn sketch_in(f: FeatureId) -> PluginInput {
    PluginInput::Sketch { sketch: f }
}

pub fn face_in(plane: SketchPlane) -> PluginInput {
    PluginInput::Face { plane }
}

/// 60×40×20 box (z 0..20) shelled open at the top, walls and floor 2 thick:
/// cavity x ±28, y ±18, z 2..20. Returns (session, extrusion).
pub fn open_box() -> (Session, FeatureId) {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 60.0, 40.0, 20.0);
    ok(
        &mut s,
        Command::ShellFace {
            name: None,
            face: FaceRef {
                feature: e,
                role: FaceRole::ExtrudeEndCap,
            },
            thickness: 2.0,
        },
    );
    (s, e)
}

pub const BOX: f64 = 60.0 * 40.0 * 20.0 - 56.0 * 36.0 * 18.0;

pub fn line_sketch(
    s: &mut Session,
    plane: SketchPlane,
    lines: &[([f64; 2], [f64; 2])],
) -> FeatureId {
    let mut sk = rcad_sketch::Sketch::default();
    for (a, b) in lines {
        let p = sk.add_point(a[0], a[1]);
        let q = sk.add_point(b[0], b[1]);
        sk.add_line(p, q);
        for (id, v) in [(p, a), (q, b)] {
            sk.constraints.push(rcad_sketch::Constraint::FixedPoint {
                p: id,
                x: v[0],
                y: v[1],
            });
        }
    }
    ok(
        s,
        Command::CreateSketch {
            name: None,
            plane,
            template: SketchTemplate::Custom { sketch: sk },
        },
    )
    .feature
    .unwrap()
}
