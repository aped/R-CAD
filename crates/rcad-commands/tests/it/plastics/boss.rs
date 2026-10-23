//! Boss: shanks, pilot/through holes, draft, gusset ribs and a mating boss,
//! against closed-form volumes.
use super::*;
use std::f64::consts::PI;

const PLATE: f64 = 60.0 * 40.0 * 4.0;

/// Plate 60×40×4 (z 0..4) with a points sketch on its top at (±15, 0).
fn setup() -> (Session, FeatureId, FeatureId) {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 60.0, 40.0, 4.0);
    let sk = points_sketch(&mut s, end_cap(e), &[[-15.0, 0.0], [15.0, 0.0]]);
    (s, e, sk)
}

fn frustum(r0: f64, r1: f64, h: f64) -> f64 {
    PI * h / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1)
}

#[test]
fn pilot_hole_bosses_have_the_closed_form_volume() {
    let (mut s, _, sk) = setup();
    let r = ok(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[("height", 8.0), ("draft_deg", 0.0), ("hole_depth", 6.0)],
            &[],
        ),
    );
    let want = PLATE + 2.0 * (PI * 9.0 * 8.0 - PI * 1.25f64.powi(2) * 6.0);
    assert_close(vol(&s, BodyId(1)), want, 1e-6, "volume");
    // Shank wall, hole, under the hole bottom, beside the shank.
    let pr = probe(
        &s,
        BodyId(1),
        &[
            [-15.0 + 2.6, 0.0, 11.0],
            [-15.0, 0.0, 9.0],
            [-15.0, 0.0, 5.5],
            [-15.0 + 3.5, 0.0, 6.0],
            [15.0, 0.0, 11.9],
        ],
    );
    assert_eq!(
        pr,
        [
            Some(true),
            Some(false),
            Some(true),
            Some(false),
            Some(false)
        ]
    );
    // Edit the height through the generic parameter path; undo restores.
    let f = r.feature.unwrap();
    ok(
        &mut s,
        Command::SetParameter {
            feature: f,
            name: "height".into(),
            value: 10.0,
        },
    );
    let want10 = PLATE + 2.0 * (PI * 9.0 * 10.0 - PI * 1.25f64.powi(2) * 6.0);
    assert_close(vol(&s, BodyId(1)), want10, 1e-6, "volume after edit");
    ok(&mut s, Command::Undo);
    assert_close(vol(&s, BodyId(1)), want, 1e-6, "volume after undo");
}

#[test]
fn drafted_bosses_are_frustums_and_through_holes_pass_the_plate() {
    let (mut s, _, sk) = setup();
    let a = 2.0f64.to_radians();
    ok(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[("height", 8.0), ("draft_deg", 2.0), ("hole_diameter", 2.0)],
            &[("hole", "through")],
        ),
    );
    let shank = frustum(3.0, 3.0 - 8.0 * a.tan(), 8.0);
    let want = PLATE + 2.0 * (shank - PI * 1.0 * 12.0);
    assert_close(vol(&s, BodyId(1)), want, 1e-6, "volume");
    // The hole goes through the plate.
    let pr = probe(&s, BodyId(1), &[[-15.0, 0.0, 0.5], [-15.0, 1.2, 0.5]]);
    assert_eq!(pr, [Some(false), Some(true)]);
}

/// Volume of one rib outside a shank of radius `r`: the slab |y| < t/2,
/// x from the cylinder to `l`, flat at `h0` up to `l - ch`, then sloping
/// linearly to `h1` at `l` (Simpson over y; the x integral is closed-form;
/// the slope starts outside the cylinder).
fn rib_outside(r: f64, t: f64, l: f64, ch: f64, h0: f64, h1: f64) -> f64 {
    let xs = l - ch;
    let f = |y: f64| {
        let a = (r * r - y * y).sqrt();
        h0 * (xs - a) + ch * (h0 + h1) / 2.0
    };
    let n = 2000;
    let dy = t / n as f64;
    let mut acc = f(-t / 2.0) + f(t / 2.0);
    for i in 1..n {
        let y = -t / 2.0 + i as f64 * dy;
        acc += if i % 2 == 1 { 4.0 } else { 2.0 } * f(y);
    }
    acc * dy / 3.0
}

#[test]
fn gusset_ribs_add_their_sections_outside_the_shank() {
    let (mut s, _, sk) = setup();
    ok(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[
                ("height", 8.0),
                ("draft_deg", 0.0),
                ("rib_count", 4.0),
                ("rib_thickness", 1.2),
                ("rib_length", 7.0),
                ("rib_height", 6.0),
                ("rib_tip_height", 2.0),
                ("rib_angle_deg", 45.0),
            ],
            &[],
        ),
    );
    let rib = rib_outside(3.0, 1.2, 7.0, 3.0, 6.0, 2.0);
    let want = PLATE + 2.0 * (PI * 9.0 * 8.0 + 4.0 * rib - PI * 1.25f64.powi(2) * 6.0);
    assert_close(vol(&s, BodyId(1)), want, 1e-6, "volume");
    // A rib at 45°: inside under its flat top, outside above its chamfer,
    // inside under it.
    let d = std::f64::consts::FRAC_1_SQRT_2;
    let at = |x: f64, z: f64| [-15.0 + x * d, x * d, z];
    let pr = probe(
        &s,
        BodyId(1),
        &[at(4.0, 4.0 + 3.0), at(6.5, 4.0 + 3.0), at(6.5, 4.0 + 2.0)],
    );
    assert_eq!(pr, [Some(true), Some(false), Some(true)]);
    // Gusset/shank edges are exact (lines along the shank, arcs under the
    // flat tops), so the body exports to STEP.
    let dir = tempfile::tempdir().unwrap();
    ok(
        &mut s,
        Command::ExportStep {
            path: dir.path().join("boss.step"),
            bodies: None,
        },
    );
}

#[test]
fn a_mating_boss_meets_the_lid_with_clearance_hole_and_counterbore() {
    let (mut s, _, sk) = setup();
    // Lid: z 14..17 (body 2); its underside faces the bosses.
    let lid = plate(&mut s, offset_xy(14.0), 60.0, 40.0, 3.0);
    let mate = SketchPlane::Face(start_cap(lid));
    ok(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk)), ("mate", face_in(mate))],
            &[
                ("height", 8.0),
                ("draft_deg", 0.0),
                ("gap", 0.5),
                ("mate_hole_diameter", 3.4),
                ("cbore_diameter", 6.0),
                ("cbore_depth", 1.0),
            ],
            &[],
        ),
    );
    let base = PLATE + 2.0 * (PI * 9.0 * 8.0 - PI * 1.25f64.powi(2) * 6.0);
    assert_close(vol(&s, BodyId(1)), base, 1e-6, "base volume");
    // Mating shank 14 - 12 - 0.5 = 1.5 long; hole through 3 + 1.5; counterbore 1 deep.
    let rh = 1.7;
    let lid_v = 60.0 * 40.0 * 3.0
        + 2.0 * (PI * 9.0 * 1.5 - PI * rh * rh * 4.5 - PI * (9.0 - rh * rh) * 1.0);
    assert_close(vol(&s, BodyId(2)), lid_v, 1e-6, "lid volume");
    let pr = probe(
        &s,
        BodyId(2),
        &[
            [15.0, 0.0, 15.0],
            [15.0, 2.5, 16.5],
            [15.0, 2.5, 15.5],
            [15.0, 2.5, 13.0],
        ],
    );
    assert_eq!(pr, [Some(false), Some(false), Some(true), Some(true)]);
}

#[test]
fn invalid_bosses_are_refused_and_leave_the_document_unchanged() {
    let (mut s, e, sk) = setup();
    let before = vol(&s, BodyId(1));
    // Hole wider than the drafted top.
    let m = err(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[("hole_diameter", 6.5)],
            &[],
        ),
    );
    assert!(m.contains("not wider than"), "{m}");
    // Pilot deeper than the material.
    let m = err(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[("height", 5.0), ("hole_depth", 9.5)],
            &[],
        ),
    );
    assert!(m.contains("deeper than the material"), "{m}");
    // Parameter out of range, unknown parameter, unknown choice value.
    let m = err(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[("height", 0.0)],
            &[],
        ),
    );
    assert!(m.contains("height"), "{m}");
    let m = err(
        &mut s,
        feature("boss", &[("sketch", sketch_in(sk))], &[("nope", 1.0)], &[]),
    );
    assert!(m.contains("unknown parameter"), "{m}");
    let m = err(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[],
            &[("hole", "drill")],
        ),
    );
    assert!(m.contains("not one of"), "{m}");
    // A sketch with no points; a sketch on a world plane (no body).
    let empty = points_sketch(&mut s, end_cap(e), &[]);
    let m = err(
        &mut s,
        feature("boss", &[("sketch", sketch_in(empty))], &[], &[]),
    );
    assert!(m.contains("no points"), "{m}");
    let world = points_sketch(&mut s, offset_xy(30.0), &[[0.0, 0.0]]);
    let m = err(
        &mut s,
        feature("boss", &[("sketch", sketch_in(world))], &[], &[]),
    );
    assert!(m.contains("must lie on a face"), "{m}");
    assert_close(vol(&s, BodyId(1)), before, 1e-12, "unchanged");
}

#[test]
fn a_tip_fillet_rounds_the_shank_top_exactly() {
    let (mut s, _, sk) = setup();
    ok(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[("height", 8.0), ("draft_deg", 0.0), ("tip_radius", 0.5)],
            &[],
        ),
    );
    // Removed per boss (docs/FILLETS.md): A·P − M·Σφ, a full circle of R = 3.
    let r: f64 = 0.5;
    let (a, m) = (r * r * (1.0 - PI / 4.0), r.powi(3) * (5.0 / 6.0 - PI / 4.0));
    let removed = a * 2.0 * PI * 3.0 - m * 2.0 * PI;
    let want = PLATE + 2.0 * (PI * 9.0 * 8.0 - PI * 1.25f64.powi(2) * 6.0 - removed);
    assert_close(vol(&s, BodyId(1)), want, 1e-6, "volume");
    // The sharp top corner is gone; the wall below the fillet is kept.
    let pr = probe(
        &s,
        BodyId(1),
        &[[-15.0 + 2.95, 0.0, 11.95], [-15.0 + 2.95, 0.0, 11.0]],
    );
    assert_eq!(pr, [Some(false), Some(true)]);
}

#[test]
fn alignment_steps_mate_out_with_in() {
    for step in ["out", "in"] {
        let (mut s, _, sk) = setup();
        let lid = plate(&mut s, offset_xy(14.0), 60.0, 40.0, 3.0);
        let mate = SketchPlane::Face(start_cap(lid));
        ok(
            &mut s,
            feature(
                "boss",
                &[("sketch", sketch_in(sk)), ("mate", face_in(mate))],
                &[
                    ("height", 8.0),
                    ("draft_deg", 0.0),
                    ("gap", 0.5),
                    ("cbore_depth", 1.0),
                    ("step_diameter", 4.5),
                    ("step_depth", 0.5),
                ],
                &[("step", step)],
            ),
        );
        let rh: f64 = 1.7;
        let base = PLATE + 2.0 * (PI * 9.0 * 8.0 - PI * 1.25f64.powi(2) * 6.0);
        let lid_v = 60.0 * 40.0 * 3.0
            + 2.0 * (PI * 9.0 * 1.5 - PI * rh * rh * 4.5 - PI * (9.0 - rh * rh) * 1.0);
        let ring = |r0: f64, r1: f64, h: f64| PI * (r1 * r1 - r0 * r0) * h;
        let (want_base, want_lid) = if step == "out" {
            // Ring 2.25 x 0.5 on each boss (pilot through it); the lid's
            // recess is 0.5 wider in radius and 0.5 deeper.
            (
                base + 2.0 * ring(1.25, 2.25, 0.5),
                lid_v - 2.0 * ring(rh, 2.75, 1.0),
            )
        } else {
            (
                base - 2.0 * ring(1.25, 2.75, 1.0),
                lid_v + 2.0 * ring(rh, 2.25, 0.5),
            )
        };
        assert_close(
            vol(&s, BodyId(1)),
            want_base,
            1e-6,
            &format!("base, step {step}"),
        );
        assert_close(
            vol(&s, BodyId(2)),
            want_lid,
            1e-6,
            &format!("lid, step {step}"),
        );
    }
}

#[test]
fn a_countersunk_seat_is_an_exact_cone() {
    let (mut s, _, sk) = setup();
    let lid = plate(&mut s, offset_xy(14.0), 60.0, 40.0, 3.0);
    let mate = SketchPlane::Face(start_cap(lid));
    ok(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk)), ("mate", face_in(mate))],
            &[
                ("height", 8.0),
                ("draft_deg", 0.0),
                ("gap", 0.5),
                ("cbore_diameter", 6.0),
            ],
            &[("seat", "countersink")],
        ),
    );
    // 90° cone from Ø6 at the lid's top down to the Ø3.4 hole: 1.3 deep.
    let (big, r, hh) = (3.0f64, 1.7f64, 1.3f64);
    let cone = PI * hh / 3.0 * (big * big + big * r + r * r) - PI * r * r * hh;
    let lid_v = 60.0 * 40.0 * 3.0 + 2.0 * (PI * 9.0 * 1.5 - PI * r * r * 4.5) - 2.0 * cone;
    assert_close(vol(&s, BodyId(2)), lid_v, 1e-6, "countersunk lid");
    // Cone radius 3 - d at d below the top: r 2.5 is air at d 0.1, material at d 1.
    let p = |rad: f64, z: f64| [15.0 + rad, 0.0, z];
    let pr = probe(&s, BodyId(2), &[p(2.5, 16.9), p(2.5, 16.0)]);
    assert_eq!(pr, [Some(false), Some(true)]);
}

#[test]
fn a_root_fillet_ring_follows_pappus() {
    let (mut s, _, sk) = setup();
    ok(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[("height", 8.0), ("draft_deg", 0.0), ("root_radius", 1.0)],
            &[],
        ),
    );
    // Spandrel f²(1 − π/4) at its centroid r = R + f(10 − 3π)/(12 − 3π).
    let f = 1.0f64;
    let area = f * f * (1.0 - PI / 4.0);
    let rc = 3.0 + f * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let ring = 2.0 * PI * rc * area;
    let want = PLATE + 2.0 * (PI * 9.0 * 8.0 - PI * 1.25f64.powi(2) * 6.0 + ring);
    assert_close(vol(&s, BodyId(1)), want, 1e-6, "volume");
    // Material in the fillet corner, air beyond the arc.
    let pr = probe(
        &s,
        BodyId(1),
        &[[-15.0 + 3.1, 0.0, 4.1], [-15.0 + 3.9, 0.0, 4.9]],
    );
    assert_eq!(pr, [Some(true), Some(false)]);
}

#[test]
fn ribs_on_a_drafted_shank_are_refused_by_name() {
    let (mut s, _, sk) = setup();
    let m = err(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[("draft_deg", 1.0), ("rib_count", 4.0)],
            &[],
        ),
    );
    assert!(m.contains("drafted shank"), "{m}");
}

#[test]
fn a_root_fillet_on_a_drafted_shank_meets_the_cone() {
    let (mut s, _, sk) = setup();
    ok(
        &mut s,
        feature(
            "boss",
            &[("sketch", sketch_in(sk))],
            &[("height", 8.0), ("draft_deg", 2.0), ("root_radius", 1.0)],
            &[],
        ),
    );
    // Spandrel between the base, the cone generator (leaning 2° in) and the
    // tangent arc, revolved: Pappus with the region integrated numerically
    // in (radius, height) from the closed-form boundary (independent of the
    // kernel).
    let a = 2f64.to_radians();
    let f = 1.0f64;
    let (sa, ca) = a.sin_cos();
    let cx = 3.0 + f * (1.0 - sa) / ca;
    let h_t = f * (1.0 - sa);
    // At height y in [0, h_t]: from the wall r - y tan a to the arc
    // cx - sqrt(f² - (f - y)²); integrate (2π ρ dρ) over that interval.
    let n = 20000;
    let mut v = 0.0;
    for i in 0..n {
        let y = (i as f64 + 0.5) * h_t / n as f64;
        let wall = 3.0 - y * a.tan();
        let arc = cx - (f * f - (f - y).powi(2)).sqrt();
        v += PI * (arc * arc - wall * wall) * h_t / n as f64;
    }
    let shank = frustum(3.0, 3.0 - 8.0 * a.tan(), 8.0);
    let want = PLATE + 2.0 * (shank - PI * 1.25f64.powi(2) * 6.0 + v);
    assert_close(vol(&s, BodyId(1)), want, 1e-6, "volume");
}
