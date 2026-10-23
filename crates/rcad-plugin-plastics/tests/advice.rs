//! Design Advice on hand-built meshes with analytic answers: boxes built
//! from grid cells (every face a plane), a drafted frustum, a faceted prism
//! labelled as a cylinder, a notched block and a stepped plate.
use rcad_plugin::{
    AnalysisRequest, BodyGeom, BodyMesh, FaceGeom, FaceInfo, Frame, PluginError, Resolved, Surface,
};
use rcad_plugin_plastics::advice::analyze;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

const TOL: f64 = 1e-9;

/// Closed solid made of the `filled` cells of the grid `xs × ys × zs`: one
/// quad (two triangles) per exposed cell side, one plane face per
/// (axis, side, grid plane), vertices welded by grid index.
fn grid_solid(coords: [&[f64]; 3], filled: impl Fn(usize, usize, usize) -> bool) -> BodyMesh {
    let n = [
        coords[0].len() - 1,
        coords[1].len() - 1,
        coords[2].len() - 1,
    ];
    let is = |c: [isize; 3]| {
        (0..3).all(|a| c[a] >= 0 && (c[a] as usize) < n[a])
            && filled(c[0] as usize, c[1] as usize, c[2] as usize)
    };
    let mut vid: HashMap<[usize; 3], u32> = HashMap::new();
    let mut vertices = Vec::new();
    let mut fid: HashMap<(usize, bool, usize), u32> = HashMap::new();
    let mut faces: Vec<FaceInfo> = Vec::new();
    let (mut triangles, mut tri_face) = (Vec::new(), Vec::new());
    for i in 0..n[0] {
        for j in 0..n[1] {
            for k in 0..n[2] {
                if !filled(i, j, k) {
                    continue;
                }
                let cell = [i, j, k];
                for a in 0..3 {
                    for pos in [false, true] {
                        let mut nb = cell.map(|v| v as isize);
                        nb[a] += if pos { 1 } else { -1 };
                        if is(nb) {
                            continue;
                        }
                        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                        let plane = cell[a] + usize::from(pos);
                        let mut corners = [[0usize; 3]; 4];
                        for (q, (db, dc)) in [(0, 0), (1, 0), (1, 1), (0, 1)].iter().enumerate() {
                            corners[q][a] = plane;
                            corners[q][b] = cell[b] + db;
                            corners[q][c] = cell[c] + dc;
                        }
                        if !pos {
                            corners.reverse();
                        }
                        let ids = corners.map(|g| {
                            *vid.entry(g).or_insert_with(|| {
                                vertices.push([coords[0][g[0]], coords[1][g[1]], coords[2][g[2]]]);
                                (vertices.len() - 1) as u32
                            })
                        });
                        let f = *fid.entry((a, pos, plane)).or_insert_with(|| {
                            let mut normal = [0.0; 3];
                            normal[a] = if pos { 1.0 } else { -1.0 };
                            faces.push(FaceInfo {
                                surface: Surface::Plane { normal },
                                area: 0.0,
                            });
                            (faces.len() - 1) as u32
                        });
                        faces[f as usize].area += (coords[b][cell[b] + 1] - coords[b][cell[b]])
                            * (coords[c][cell[c] + 1] - coords[c][cell[c]]);
                        triangles.push([ids[0], ids[1], ids[2]]);
                        triangles.push([ids[0], ids[2], ids[3]]);
                        tri_face.extend([f, f]);
                    }
                }
            }
        }
    }
    BodyMesh {
        body: 7,
        chord: 0.01,
        faces,
        vertices,
        triangles,
        tri_face,
    }
}

fn block(x: f64, y: f64, z: f64) -> BodyMesh {
    grid_solid([&[0.0, x], &[0.0, y], &[0.0, z]], |_, _, _| true)
}

fn request(mesh: BodyMesh, over: &[(&str, f64)], axis: &str) -> AnalysisRequest {
    let mut params: BTreeMap<String, f64> = [
        ("min_thickness", 1.0),
        ("max_thickness", 3.0),
        ("variation", 0.15),
        ("min_draft_deg", 0.5),
        ("knife_edge", 1.0),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), *v))
    .collect();
    for (k, v) in over {
        params.insert(k.to_string(), *v);
    }
    AnalysisRequest {
        analysis: "design_advice".into(),
        params,
        choices: [("pull_axis".to_string(), axis.to_string())].into(),
        inputs: [(
            "body".to_string(),
            Resolved::Body(BodyGeom {
                body: mesh.body,
                bounds: [[0.0; 3], [1.0; 3]],
                faces: vec![],
            }),
        )]
        .into(),
        data: Value::Null,
        bodies: vec![mesh],
    }
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} != {b} (tol {tol})");
}

/// The face index of the plane with this outward normal containing `p`.
fn face_of(m: &BodyMesh, normal: [f64; 3], p: [f64; 3]) -> usize {
    (0..m.faces.len())
        .find(|&fi| {
            m.faces[fi].surface == Surface::Plane { normal }
                && m.tri_face.iter().zip(&m.triangles).any(|(&tf, t)| {
                    tf as usize == fi && {
                        let v = m.vertices[t[0] as usize];
                        (0..3).all(|a| normal[a] == 0.0 || v[a] == p[a])
                    }
                })
        })
        .expect("face")
}

fn alerts<'a>(r: &'a Value, kind: &str) -> Vec<&'a Value> {
    r["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["kind"] == kind)
        .collect()
}

#[test]
fn solid_box_thickness_draft_and_no_undercut() {
    let m = block(40.0, 20.0, 10.0);
    assert_eq!(m.triangles.len(), 12);
    let r = analyze(&request(m.clone(), &[("max_thickness", 30.0)], "+z")).unwrap();
    assert_eq!(r["mesh"]["closed"], true);
    assert_eq!(r["pull"], serde_json::json!([0.0, 0.0, 1.0]));
    assert_eq!(r["pull_source"], "axis");
    // Thickness along each axis is the box size in that direction.
    let t = &r["thickness"];
    close(f(&t["min"]), 10.0, TOL);
    close(f(&t["max"]), 40.0, TOL);
    // Area-weighted mean: (2·800·10 + 2·200·40 + 2·400·20) / 2800 = 120/7.
    close(f(&t["mean"]), 120.0 / 7.0, TOL);
    for fe in t["faces"].as_array().unwrap() {
        let fi = f(&fe["face"]) as usize;
        let Surface::Plane { normal } = m.faces[fi].surface else {
            panic!()
        };
        let want = if normal[0] != 0.0 {
            40.0
        } else if normal[1] != 0.0 {
            20.0
        } else {
            10.0
        };
        close(f(&fe["min"]), want, TOL);
        close(f(&fe["max"]), want, TOL);
    }
    // Too thick (> 30): the two x faces, 2·20·10.
    close(f(&t["too_thick_area"]), 400.0, TOL);
    let thick = alerts(&r, "too_thick");
    assert_eq!(thick.len(), 2);
    for a in thick {
        close(f(&a["value"]), 40.0, TOL);
        assert_eq!(a["unit"], "mm");
    }
    assert!(alerts(&r, "too_thin").is_empty());
    // With the default maximum (3 mm) every wall is too thick.
    let r3 = analyze(&request(m.clone(), &[], "+z")).unwrap();
    close(f(&r3["summary"]["too_thick_area"]), 2800.0, 1e-9);
    // Draft: four walls parallel to the pull at exactly 0°, top/bottom 90°.
    let mut zero = 0;
    for d in r["draft"]["faces"].as_array().unwrap() {
        assert_eq!(d["exact"], true);
        let fi = f(&d["face"]) as usize;
        let Surface::Plane { normal } = m.faces[fi].surface else {
            panic!()
        };
        if normal[2] == 0.0 {
            zero += 1;
            assert_eq!(f(&d["draft_deg"]), 0.0);
            assert_eq!(d["side"], "vertical");
            close(f(&d["area_below"]), m.faces[fi].area, TOL);
        } else {
            close(f(&d["draft_deg"]), 90.0, TOL);
            assert_eq!(d["side"], if normal[2] > 0.0 { "+pull" } else { "-pull" });
            assert_eq!(f(&d["area_below"]), 0.0);
        }
    }
    assert_eq!(zero, 4);
    assert_eq!(alerts(&r, "draft").len(), 4);
    close(f(&r["draft"]["area_below"]), 2.0 * 400.0 + 2.0 * 200.0, TOL);
    // A plain block has no undercut, knife edge or thickness step.
    assert_eq!(f(&r["undercut"]["area"]), 0.0);
    assert_eq!(f(&r["knife_edge"]["area"]), 0.0);
    assert_eq!(r["variation"]["pairs"], 0);
    assert_eq!(r["summary"]["alerts"]["draft"], 4);
    assert_eq!(r["summary"]["alerts"]["too_thick"], 2);
}

#[test]
fn pull_from_a_face_normal() {
    let m = block(40.0, 20.0, 10.0);
    let mut req = request(m, &[], "+z");
    req.inputs.insert(
        "pull".into(),
        Resolved::Face(FaceGeom {
            frame: Frame::from_normal([40.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap(),
            body: 7,
            loops: vec![],
            body_span: [-40.0, 0.0],
            parallel: vec![],
        }),
    );
    let r = analyze(&req).unwrap();
    assert_eq!(r["pull_source"], "face");
    assert_eq!(r["pull"], serde_json::json!([1.0, 0.0, 0.0]));
    // Walls parallel to x: the y and z faces, 2·(40·10) + 2·(40·20).
    assert_eq!(alerts(&r, "draft").len(), 4);
    close(f(&r["draft"]["area_below"]), 800.0 + 1600.0, TOL);
}

#[test]
fn hollow_box_walls_and_enclosed_void() {
    // Outer 40×20×10, closed inner void 36×16×6: 2 mm walls, floor and roof.
    let m = grid_solid(
        [
            &[0.0, 2.0, 38.0, 40.0],
            &[0.0, 2.0, 18.0, 20.0],
            &[0.0, 2.0, 8.0, 10.0],
        ],
        |i, j, k| (i, j, k) != (1, 1, 1),
    );
    assert_eq!(m.faces.len(), 12);
    let r = analyze(&request(m.clone(), &[("min_thickness", 2.5)], "+z")).unwrap();
    assert_eq!(r["mesh"]["closed"], true);
    close(f(&r["thickness"]["min"]), 2.0, TOL);
    // Every face, outer or inner, has 2 mm walls somewhere (over the void).
    let faces = r["thickness"]["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 12);
    for fe in faces {
        close(f(&fe["min"]), 2.0, TOL);
    }
    // Over the rim the inward ray runs through a whole wall: up to 40.
    close(f(&r["thickness"]["max"]), 40.0, TOL);
    // Too thin (< 2.5): the void's surface plus its projection on the
    // outer faces, 2 · 2·(36·16 + 16·6 + 36·6).
    let void_area = 2.0 * (36.0 * 16.0 + 16.0 * 6.0 + 36.0 * 6.0);
    close(f(&r["thickness"]["too_thin_area"]), 2.0 * void_area, 1e-9);
    assert_eq!(alerts(&r, "too_thin").len(), 12);
    // An enclosed void can never release: all of it is undercut, the outside
    // none.
    close(f(&r["undercut"]["area"]), void_area, 1e-9);
    assert_eq!(r["undercut"]["faces"].as_array().unwrap().len(), 6);
    // The outer faces step from 2 mm (over the void) to the rim's full wall.
    assert!(f(&r["variation"]["pairs"]) > 0.0);
}

/// Square frustum: base 2a at z = 0, top 2b at z = h, walls drafted by `deg`.
fn frustum(a: f64, h: f64, deg: f64) -> BodyMesh {
    let th = deg.to_radians();
    let b = a - h * th.tan();
    let vertices = vec![
        [-a, -a, 0.0],
        [a, -a, 0.0],
        [a, a, 0.0],
        [-a, a, 0.0],
        [-b, -b, h],
        [b, -b, h],
        [b, b, h],
        [-b, b, h],
    ];
    let (c, s) = (th.cos(), th.sin());
    let side = (a + b) * h / c;
    let plane = |normal: [f64; 3], area: f64| FaceInfo {
        surface: Surface::Plane { normal },
        area,
    };
    let faces = vec![
        plane([0.0, 0.0, -1.0], 4.0 * a * a),
        plane([0.0, 0.0, 1.0], 4.0 * b * b),
        plane([0.0, -c, s], side),
        plane([c, 0.0, s], side),
        plane([0.0, c, s], side),
        plane([-c, 0.0, s], side),
    ];
    let triangles = vec![
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    BodyMesh {
        body: 7,
        chord: 0.0,
        faces,
        vertices,
        triangles,
        tri_face: vec![0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5],
    }
}

#[test]
fn drafted_frustum_walls_measure_their_draft_exactly() {
    let m = frustum(10.0, 10.0, 2.0);
    let side_area = m.faces[2].area;
    let r = analyze(&request(m.clone(), &[("min_draft_deg", 1.0)], "+z")).unwrap();
    assert_eq!(r["mesh"]["closed"], true);
    let d = r["draft"]["faces"].as_array().unwrap();
    for fe in &d[2..] {
        close(f(&fe["draft_deg"]), 2.0, 1e-9);
        assert_eq!(fe["side"], "+pull");
        assert_eq!(fe["exact"], true);
    }
    assert_eq!(d[0]["side"], "-pull");
    assert!(alerts(&r, "draft").is_empty());
    assert_eq!(f(&r["undercut"]["area"]), 0.0);
    // Below a 3° minimum all four walls are flagged with their whole area.
    let r3 = analyze(&request(m.clone(), &[("min_draft_deg", 3.0)], "+z")).unwrap();
    let a = alerts(&r3, "draft");
    assert_eq!(a.len(), 4);
    for x in a {
        close(f(&x["value"]), 2.0, 1e-9);
        assert_eq!(x["unit"], "deg");
    }
    close(f(&r3["draft"]["area_below"]), 4.0 * side_area, 1e-9);
    // Pulled the other way the walls are on the -pull side and still release.
    let rn = analyze(&request(m, &[("min_draft_deg", 1.0)], "-z")).unwrap();
    assert_eq!(rn["draft"]["faces"][3]["side"], "-pull");
    assert_eq!(f(&rn["undercut"]["area"]), 0.0);
}

/// Octagonal prism (circumradius 10, height 5) whose eight walls form one
/// face labelled as a cylinder about z.
fn octagon_prism() -> BodyMesh {
    let (r, h) = (10.0, 5.0);
    let mut vertices = Vec::new();
    for z in [0.0, h] {
        for k in 0..8 {
            // Corners at 22.5° + k·45°, so the wall normals are at k·45°.
            let a = (22.5 + 45.0 * k as f64).to_radians();
            vertices.push([r * a.cos(), r * a.sin(), z]);
        }
    }
    vertices.push([0.0, 0.0, 0.0]);
    vertices.push([0.0, 0.0, h]);
    let (mut triangles, mut tri_face) = (Vec::new(), Vec::new());
    for k in 0..8u32 {
        let k1 = (k + 1) % 8;
        triangles.push([16, k1, k]);
        tri_face.push(0);
        triangles.push([17, 8 + k, 8 + k1]);
        tri_face.push(1);
        triangles.push([k, k1, 8 + k1]);
        triangles.push([k, 8 + k1, 8 + k]);
        tri_face.extend([2, 2]);
    }
    let side = 2.0 * r * (22.5f64).to_radians().sin();
    BodyMesh {
        body: 7,
        chord: 0.5,
        faces: vec![
            FaceInfo {
                surface: Surface::Plane {
                    normal: [0.0, 0.0, -1.0],
                },
                area: 0.0,
            },
            FaceInfo {
                surface: Surface::Plane {
                    normal: [0.0, 0.0, 1.0],
                },
                area: 0.0,
            },
            FaceInfo {
                surface: Surface::Cylinder {
                    point: [0.0; 3],
                    axis: [0.0, 0.0, 1.0],
                    radius: r,
                    outward: true,
                },
                area: 8.0 * side * h,
            },
        ],
        vertices,
        triangles,
        tri_face,
    }
}

#[test]
fn cylinder_draft_is_exact_along_its_axis_and_meshed_across_it() {
    let m = octagon_prism();
    let wall = m.faces[2].area;
    let r = analyze(&request(m.clone(), &[], "+z")).unwrap();
    let c = &r["draft"]["faces"][2];
    assert_eq!(c["surface"], "cylinder");
    assert_eq!(c["exact"], true);
    assert_eq!(f(&c["draft_deg"]), 0.0);
    assert_eq!(c["side"], "vertical");
    close(f(&c["area_below"]), wall, 1e-12);
    // Pulled along x the walls whose normals are ±y (k = 2, 6) have 0°
    // draft; the others at least 45°.
    let rx = analyze(&request(m, &[], "+x")).unwrap();
    let c = &rx["draft"]["faces"][2];
    assert_eq!(c["exact"], false);
    assert_eq!(c["side"], "mixed");
    close(f(&c["draft_deg"]), 0.0, 1e-9);
    close(f(&c["area_below"]), 2.0 * wall / 8.0, 1e-9);
    assert!(
        rx["approximation"]["note"]
            .as_str()
            .unwrap()
            .contains("0.5 mm")
    );
}

#[test]
fn side_notch_is_an_undercut_for_a_vertical_pull_only() {
    // 40×20×10 block with a slot through y in its +x wall: x 36..40, z 4..6.
    let xs = [0.0, 36.0, 40.0];
    let ys = [0.0, 20.0];
    let zs = [0.0, 4.0, 6.0, 10.0];
    let m = grid_solid([&xs, &ys, &zs], |i, _, k| (i, k) != (1, 1));
    let r = analyze(&request(m.clone(), &[], "+z")).unwrap();
    assert_eq!(r["mesh"]["closed"], true);
    // Slot roof 4·20 + slot floor 4·20 + slot back 2·20.
    close(f(&r["undercut"]["area"]), 200.0, 1e-9);
    let mut by_face: Vec<(usize, f64)> = r["undercut"]["faces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| (f(&u["face"]) as usize, f(&u["area"])))
        .collect();
    by_face.sort_by_key(|u| u.0);
    let mut want = vec![
        (face_of(&m, [0.0, 0.0, -1.0], [0.0, 0.0, 6.0]), 80.0),
        (face_of(&m, [0.0, 0.0, 1.0], [0.0, 0.0, 4.0]), 80.0),
        (face_of(&m, [1.0, 0.0, 0.0], [36.0, 0.0, 0.0]), 40.0),
    ];
    want.sort_by_key(|u| u.0);
    assert_eq!(by_face.len(), 3);
    for (g, w) in by_face.iter().zip(&want) {
        assert_eq!(g.0, w.0);
        close(g.1, w.1, 1e-9);
    }
    let u = alerts(&r, "undercut");
    assert_eq!(u.len(), 3);
    assert_eq!(u[0]["unit"], "mm2");
    // Pulled along +x the slot opens towards the moving half: no undercut.
    let rx = analyze(&request(m, &[], "+x")).unwrap();
    assert_eq!(f(&rx["undercut"]["area"]), 0.0);
}

#[test]
fn stepped_plate_variation_and_knife_edge() {
    // 40×10 plate: x 0..20 is 0.8 thick, x 20..40 is 6 thick.
    let m = grid_solid(
        [&[0.0, 20.0, 40.0], &[0.0, 10.0], &[0.0, 0.8, 6.0]],
        |i, _, k| k == 0 || i == 1,
    );
    let r = analyze(&request(m.clone(), &[], "+z")).unwrap();
    assert_eq!(r["mesh"]["closed"], true);
    close(f(&r["thickness"]["min"]), 0.8, 1e-9);
    // The floor changes from 0.8 to 6 across x = 20: (6 − 0.8)/0.8 = 6.5
    // between two 100 mm² triangles. On the +x end wall the inward ray runs
    // 40 through the thin plate below z = 0.8 but stops at the step (20)
    // above it: (40 − 20)/20 = 1 between triangles of 4 and 26 mm².
    let v = &r["variation"];
    assert_eq!(v["pairs"], 2);
    close(f(&v["max"]), 6.5, 1e-9);
    close(f(&v["area"]), 200.0 + 30.0, 1e-9);
    let loc = &v["locations"][0]["location"];
    close(f(&loc[0]), 20.0, 1e-12);
    close(f(&loc[2]), 0.0, 1e-12);
    close(f(&v["locations"][1]["value"]), 1.0, 1e-9);
    let floor = face_of(&m, [0.0, 0.0, -1.0], [0.0, 0.0, 0.0]);
    let end = face_of(&m, [1.0, 0.0, 0.0], [40.0, 0.0, 0.0]);
    let va = alerts(&r, "variation");
    assert_eq!(va.len(), 2);
    let mut got: Vec<usize> = va.iter().map(|a| f(&a["face"]) as usize).collect();
    got.sort();
    let mut want = vec![floor, end];
    want.sort();
    assert_eq!(got, want);
    // Knife edges (< 1 mm) and too thin (< 1 mm): the thin plate's floor and
    // top, 2·20·10.
    close(f(&r["knife_edge"]["area"]), 400.0, 1e-9);
    close(f(&r["thickness"]["too_thin_area"]), 400.0, 1e-9);
    for k in r["knife_edge"]["locations"].as_array().unwrap() {
        close(f(&k["value"]), 0.8, 1e-9);
        assert!(f(&k["location"][0]) < 20.0);
    }
    assert_eq!(alerts(&r, "knife_edge").len(), 2);
    // A looser variation limit (7 = 700 %) accepts the step.
    let r7 = analyze(&request(m, &[("variation", 7.0)], "+z")).unwrap();
    assert_eq!(r7["variation"]["pairs"], 0);
}

fn input_error(r: Result<Value, PluginError>) -> String {
    match r {
        Err(PluginError::Input(m)) => m,
        other => panic!("expected an input error, got {other:?}"),
    }
}

#[test]
fn invalid_inputs_are_refused() {
    let good = block(4.0, 2.0, 1.0);
    let run = |m: BodyMesh| analyze(&request(m, &[], "+z"));

    let mut m = good.clone();
    m.triangles.clear();
    m.tri_face.clear();
    assert!(input_error(run(m)).contains("empty"));

    let mut m = good.clone();
    m.tri_face.pop();
    assert!(input_error(run(m)).contains("face indices"));

    let mut m = good.clone();
    m.triangles[3][1] = 99;
    assert!(input_error(run(m)).contains("out of range"));

    let mut m = good.clone();
    m.tri_face[0] = 99;
    assert!(input_error(run(m)).contains("missing face"));

    let mut m = good.clone();
    m.vertices[0][2] = f64::NAN;
    assert!(input_error(run(m)).contains("non-finite"));

    let mut m = good.clone();
    for t in &mut m.triangles {
        t.swap(1, 2);
    }
    assert!(input_error(run(m)).contains("outward"));

    // A plane face whose triangles point the other way.
    let mut m = good.clone();
    m.faces[0].surface = Surface::Plane {
        normal: match m.faces[0].surface {
            Surface::Plane { normal } => normal.map(|v| -v),
            _ => unreachable!(),
        },
    };
    assert!(input_error(run(m)).contains("disagrees"));

    let mut req = request(good.clone(), &[], "+z");
    req.inputs.clear();
    assert!(input_error(analyze(&req)).contains("missing"));

    let mut req = request(good.clone(), &[], "+z");
    req.bodies[0].body = 8;
    assert!(input_error(analyze(&req)).contains("no mesh"));

    let req = request(good.clone(), &[], "up");
    assert!(input_error(analyze(&req)).contains("pull axis"));

    let mut req = request(good.clone(), &[], "+z");
    req.params.remove("variation");
    assert!(input_error(analyze(&req)).contains("variation"));

    let req = request(good, &[("min_thickness", f64::INFINITY)], "+z");
    assert!(input_error(analyze(&req)).contains("min_thickness"));
}

/// ~20k triangles: a 40×20×10 block on a 60×40×25 cell grid.
fn fine_block() -> BodyMesh {
    let axis = |len: f64, n: usize| {
        (0..=n)
            .map(|i| len * i as f64 / n as f64)
            .collect::<Vec<_>>()
    };
    let (xs, ys, zs) = (axis(40.0, 60), axis(20.0, 40), axis(10.0, 25));
    grid_solid([&xs, &ys, &zs], |_, _, _| true)
}

#[test]
fn twenty_thousand_triangles_analyse_quickly() {
    let m = fine_block();
    assert_eq!(m.triangles.len(), 2 * 2 * (60 * 40 + 40 * 25 + 60 * 25));
    let req = request(m, &[], "+z");
    let start = std::time::Instant::now();
    let r = analyze(&req).unwrap();
    let took = start.elapsed();
    close(f(&r["thickness"]["min"]), 10.0, 1e-9);
    close(f(&r["thickness"]["max"]), 40.0, 1e-9);
    assert_eq!(r["thickness"]["unresolved"], 0);
    assert_eq!(f(&r["undercut"]["area"]), 0.0);
    // The bound holds with a wide margin even unoptimised (this crate builds
    // at opt-level 0 in the dev profile): well under half a second.
    assert!(took.as_secs_f64() < 2.0, "took {took:?}");
}
