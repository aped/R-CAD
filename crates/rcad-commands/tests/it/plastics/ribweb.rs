//! Rib and Web inside an open box, against closed-form volumes.
use super::*;
use rcad_commands::Command;

/// 60×40×20 box (z 0..20) shelled open at the top, walls and floor 2 thick:
/// cavity x ±28, y ±18, z 2..20. Returns (session, extrusion).
fn open_box() -> (Session, FeatureId) {
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

const BOX: f64 = 60.0 * 40.0 * 20.0 - 56.0 * 36.0 * 18.0;

fn line_sketch(s: &mut Session, plane: SketchPlane, lines: &[([f64; 2], [f64; 2])]) -> FeatureId {
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

fn body_in() -> PluginInput {
    PluginInput::Body { body: BodyId(1) }
}

#[test]
fn a_rib_fills_down_from_its_line_to_the_floor() {
    let (mut s, _) = open_box();
    assert_close(vol(&s, BodyId(1)), BOX, 1e-9, "box");
    // XZ plane: sketch x = X, y = Z. Line at Z = 15 into both walls.
    let sk = line_sketch(
        &mut s,
        SketchPlane::WorldXZ,
        &[([-29.0, 15.0], [29.0, 15.0])],
    );
    let r = ok(
        &mut s,
        feature(
            "rib",
            &[("sketch", sketch_in(sk)), ("body", body_in())],
            &[("thickness", 2.0), ("depth", 13.5)],
            &[("side", "right")],
        ),
    );
    assert_close(vol(&s, BodyId(1)), BOX + 56.0 * 2.0 * 13.0, 1e-6, "rib");
    let pr = probe(
        &s,
        BodyId(1),
        &[[0.0, 0.9, 8.0], [0.0, 1.1, 8.0], [0.0, 0.0, 15.5]],
    );
    assert_eq!(pr, [Some(true), Some(false), Some(false)]);
    // Drafted: the section narrows by tan(1°) per mm on each side.
    let f = r.feature.unwrap();
    ok(
        &mut s,
        Command::SetParameter {
            feature: f,
            name: "draft_deg".into(),
            value: 1.0,
        },
    );
    let a = 1.0f64.to_radians();
    assert_close(
        vol(&s, BodyId(1)),
        BOX + 56.0 * (26.0 - 169.0 * a.tan()),
        1e-6,
        "drafted rib",
    );
}

#[test]
fn a_web_network_joins_lines_and_reaches_the_next_face() {
    let (mut s, e) = open_box();
    // On the rim plane (the cap's plane, now the rim face): a cross of two
    // lines through the cavity, each into the walls; to the floor (z = 2).
    let sk = line_sketch(
        &mut s,
        end_cap(e),
        &[([-29.0, 0.0], [29.0, 0.0]), ([0.0, -19.0], [0.0, 19.0])],
    );
    ok(
        &mut s,
        feature(
            "web",
            &[("sketch", sketch_in(sk))],
            &[("thickness", 2.0)],
            &[("direction", "flip"), ("extent", "to_next")],
        ),
    );
    // Two walls 18 high in the cavity, crossing in a 2×2 column.
    let want = BOX + (56.0 * 2.0 + 36.0 * 2.0 - 4.0) * 18.0;
    assert_close(vol(&s, BodyId(1)), want, 1e-6, "web");
    let pr = probe(
        &s,
        BodyId(1),
        &[[10.0, 0.5, 10.0], [0.5, 10.0, 19.5], [10.0, 10.0, 10.0]],
    );
    assert_eq!(pr, [Some(true), Some(true), Some(false)]);
}

#[test]
fn a_drafted_web_narrows_away_from_its_sketch() {
    let (mut s, _) = open_box();
    let sk = line_sketch(&mut s, offset_xy(20.0), &[([-29.0, 0.0], [29.0, 0.0])]);
    ok(
        &mut s,
        feature(
            "web",
            &[("sketch", sketch_in(sk)), ("body", body_in())],
            &[("thickness", 2.0), ("depth", 18.5), ("draft_deg", 2.0)],
            &[("direction", "flip")],
        ),
    );
    let a = 2.0f64.to_radians();
    // Width 2 - 2 x tan a at x mm below z = 20, over the cavity x in 0..18.
    let area = 2.0 * 18.0 - 18.0 * 18.0 * a.tan();
    assert_close(vol(&s, BodyId(1)), BOX + 56.0 * area, 1e-6, "drafted web");
}

#[test]
fn unsupported_rib_input_is_refused() {
    let (mut s, e) = open_box();
    let mut sk = rcad_sketch::Sketch::default();
    let c = sk.add_point(0.0, 0.0);
    sk.add_circle(c, 5.0);
    let circle = ok(
        &mut s,
        Command::CreateSketch {
            name: None,
            plane: end_cap(e),
            template: SketchTemplate::Custom { sketch: sk },
        },
    )
    .feature
    .unwrap();
    let m = err(
        &mut s,
        feature("web", &[("sketch", sketch_in(circle))], &[], &[]),
    );
    assert!(m.contains("only straight sketch lines"), "{m}");
    let world = line_sketch(&mut s, offset_xy(30.0), &[([0.0, 0.0], [5.0, 0.0])]);
    let m = err(
        &mut s,
        feature("rib", &[("sketch", sketch_in(world))], &[], &[]),
    );
    assert!(m.contains("choose the body"), "{m}");
    let m = err(
        &mut s,
        feature(
            "rib",
            &[("sketch", sketch_in(world)), ("body", body_in())],
            &[("thickness", 2.0), ("depth", 40.0), ("draft_deg", 5.0)],
            &[],
        ),
    );
    assert!(m.contains("draft closes"), "{m}");
    assert_close(vol(&s, BodyId(1)), BOX, 1e-9, "unchanged");
}
