//! Draft of existing faces (fixed plane), exact against the prismatoid.
use super::*;

fn sides(e: FeatureId) -> PluginInput {
    PluginInput::Faces {
        faces: (1..=4)
            .map(|entity| FaceRef {
                feature: e,
                role: FaceRole::ExtrudeSide { entity },
            })
            .collect(),
    }
}

/// ∫ (a - 2tz)(b - 2tz) dz over [0, h] (Simpson is exact for the quadratic).
fn tapered_box(a: f64, b: f64, h: f64, t: f64) -> f64 {
    let area = |z: f64| (a - 2.0 * t * z) * (b - 2.0 * t * z);
    h / 6.0 * (area(0.0) + 4.0 * area(h / 2.0) + area(h))
}

#[test]
fn drafting_the_walls_of_a_box_makes_a_tapered_box() {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 40.0, 30.0, 20.0);
    let r = ok(
        &mut s,
        feature(
            "draft",
            &[
                ("faces", sides(e)),
                ("fixed", face_in(SketchPlane::Face(start_cap(e)))),
            ],
            &[("draft_deg", 3.0)],
            &[],
        ),
    );
    let t = 3f64.to_radians().tan();
    assert_close(
        vol(&s, BodyId(1)),
        tapered_box(40.0, 30.0, 20.0, t),
        1e-9,
        "drafted box",
    );
    // The top is inset by 20 tan 3° on each side; the bottom is unchanged.
    let inset = 20.0 * t;
    let pr = probe(
        &s,
        BodyId(1),
        &[
            [19.99, 0.0, 0.01],
            [20.0 - inset - 0.01, 0.0, 19.99],
            [20.0 - inset + 0.01, 0.0, 19.99],
        ],
    );
    assert_eq!(pr, [Some(true), Some(true), Some(false)]);
    // The angle is a bindable parameter; reverse leans the walls out.
    ok(
        &mut s,
        Command::SetParameter {
            feature: r.feature.unwrap(),
            name: "draft_deg".into(),
            value: 1.0,
        },
    );
    let t1 = 1f64.to_radians().tan();
    assert_close(
        vol(&s, BodyId(1)),
        tapered_box(40.0, 30.0, 20.0, t1),
        1e-9,
        "1 degree",
    );
}

#[test]
fn draft_follows_the_rule_and_refuses_parallel_faces() {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 40.0, 30.0, 20.0);
    ok(
        &mut s,
        Command::SetPluginData(rcad_commands::plugins::SetPluginDataCmd {
            plugin: "plastics".into(),
            data: serde_json::json!({"rules": [{"name": "ABS", "values": {"DA": "2"}}], "bodies": {"1": "ABS"}}),
        }),
    );
    ok(
        &mut s,
        feature(
            "draft",
            &[
                ("faces", sides(e)),
                ("fixed", face_in(SketchPlane::Face(start_cap(e)))),
            ],
            &[],
            &[],
        ),
    );
    let t = 2f64.to_radians().tan();
    assert_close(
        vol(&s, BodyId(1)),
        tapered_box(40.0, 30.0, 20.0, t),
        1e-9,
        "rule draft",
    );
    let top = PluginInput::Faces {
        faces: vec![FaceRef {
            feature: e,
            role: FaceRole::ExtrudeEndCap,
        }],
    };
    let m = err(
        &mut s,
        feature(
            "draft",
            &[
                ("faces", top),
                ("fixed", face_in(SketchPlane::Face(start_cap(e)))),
            ],
            &[],
            &[],
        ),
    );
    assert!(m.contains("parallel to the fixed plane"), "{m}");
}

#[test]
fn a_two_sided_draft_narrows_away_from_the_parting_plane() {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 40.0, 30.0, 20.0);
    // Parting plane 10 mm up (offset from the bottom, which faces -z: -10).
    let mid = SketchPlane::Offset {
        base: rcad_document::PlaneBase::Face(start_cap(e)),
        distance: -10.0,
        angle: 0.0,
    };
    ok(
        &mut s,
        feature(
            "draft",
            &[("faces", sides(e)), ("fixed", face_in(mid))],
            &[("draft_deg", 3.0)],
            &[("sides", "two_sides")],
        ),
    );
    let t = 3f64.to_radians().tan();
    assert_close(
        vol(&s, BodyId(1)),
        2.0 * tapered_box(40.0, 30.0, 10.0, t),
        1e-9,
        "two-sided draft",
    );
    // Widest at the parting plane, inset 10 tan 3° at top and bottom.
    let inset = 10.0 * t;
    let pr = probe(
        &s,
        BodyId(1),
        &[
            [19.99, 0.0, 10.0],
            [20.0 - inset + 0.01, 0.0, 0.01],
            [20.0 - inset + 0.01, 0.0, 19.99],
        ],
    );
    assert_eq!(pr, [Some(true), Some(false), Some(false)]);
}
