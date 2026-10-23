//! Out-of-process plugins: the JSON protocol, the example plugin, and how
//! misbehaving programs (errors, crashes, hangs, floods, garbage) are
//! contained. Programs are small shell or Python scripts written into
//! temporary directories.
#![cfg(unix)]

use rcad_plugin::process::{MAX_ANSWER_BYTES, load_installed, search_dirs};
use rcad_plugin::{
    BuildRequest, FaceGeom, Frame, Mode, Op, Plugin, PluginError, ProcessPlugin, Profile, Resolved,
    Segment,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn python3() -> bool {
    let ok = std::process::Command::new("python3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("note: python3 not found; skipping the Python part of this test");
    }
    ok
}

fn manifest(id: &str, command: Value, timeout: Option<u64>) -> Value {
    let mut m = json!({
        "id": id,
        "name": "Test plugin",
        "version": "0.1.0",
        "api_version": 1,
        "command": command,
        "features": [{
            "id": "f",
            "name": "F",
            "inputs": [{"name": "face", "label": "Face", "kind": "face"}],
            "params": [{"name": "h", "label": "H", "kind": "length", "default": 1.0}]
        }]
    });
    if let Some(t) = timeout {
        m["timeout_secs"] = json!(t);
    }
    m
}

/// A plugin directory `root/<id>` with `plugin.json` and the shell script
/// `run.sh` (run as `sh run.sh`).
fn shell_plugin(root: &Path, id: &str, script: &str, timeout: Option<u64>) -> PathBuf {
    let d = root.join(id);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("run.sh"), script).unwrap();
    let m = manifest(id, json!(["sh", "run.sh"]), timeout);
    std::fs::write(d.join("plugin.json"), m.to_string()).unwrap();
    d
}

fn load_shell(root: &Path, id: &str, script: &str, timeout: Option<u64>) -> ProcessPlugin {
    ProcessPlugin::load(&shell_plugin(root, id, script, timeout)).unwrap()
}

fn face_frame() -> Frame {
    // A tilted face so no coordinate is trivially zero.
    let n = [0.0, -0.6, 0.8];
    Frame::from_normal([10.0, 20.0, 5.0], n, [1.0, 0.0, 0.0]).unwrap()
}

fn request(feature: &str, params: BTreeMap<String, f64>, data: Value) -> BuildRequest {
    BuildRequest {
        feature: feature.into(),
        params,
        choices: BTreeMap::new(),
        inputs: [(
            "face".to_string(),
            Resolved::Face(FaceGeom {
                frame: face_frame(),
                body: 1,
                loops: vec![],
                body_span: [-8.0, 0.0],
                parallel: vec![],
            }),
        )]
        .into(),
        data,
    }
}

fn failed(r: Result<impl std::fmt::Debug, PluginError>) -> String {
    match r {
        Err(PluginError::Failed(m)) => m,
        other => panic!("expected Failed, got {other:?}"),
    }
}

fn running(pid: u32) -> bool {
    // Gone, or a zombie (killed, waiting for init to reap it), is not running.
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => false,
        Ok(s) => {
            let state = s
                .rsplit(')')
                .next()
                .and_then(|r| r.split_whitespace().next());
            !matches!(state, Some("Z" | "X"))
        }
    }
}

fn read_pid(p: &Path) -> u32 {
    std::fs::read_to_string(p).unwrap().trim().parse().unwrap()
}

#[test]
fn build_answers_are_parsed_into_the_documented_shapes() {
    let tmp = tempfile::tempdir().unwrap();
    // Written by hand (not by this crate's serializer), so the test pins the
    // wire format of operations, profiles, segments and modes.
    let answer = r#"{"ok": [
      {"op": "prism", "id": 7, "body": "face",
       "frame": {"origin": [0,0,0], "x": [1,0,0], "y": [0,1,0], "z": [0,0,1]},
       "profile": {"shape": "loop", "segments": [
          {"seg": "line", "start": [-4, -1]},
          {"seg": "arc", "start": [4, -1], "center": [4, 0], "ccw": true},
          {"seg": "line", "start": [4, 1]},
          {"seg": "arc", "start": [-4, 1], "center": [-4, 0], "ccw": true}]},
       "holes": [{"shape": "circle", "center": [0, 0], "radius": 0.5}],
       "from": -2.5, "to": 1, "taper_deg": 2, "mode": "cut", "label": "slot"},
      {"op": "prism", "id": 8, "body": "face",
       "frame": {"origin": [0,0,0], "x": [1,0,0], "y": [0,1,0], "z": [0,0,1]},
       "profile": {"shape": "polygon", "points": [[0,0],[3,0],[0,3]]},
       "from": 0, "to": 2, "mode": "join"}
    ]}"#;
    std::fs::create_dir_all(tmp.path().join("t-roundtrip")).unwrap();
    std::fs::write(tmp.path().join("t-roundtrip/answer.json"), answer).unwrap();
    let p = load_shell(
        tmp.path(),
        "t-roundtrip",
        "cat > request.json\ncat answer.json\n",
        None,
    );
    let req = request("f", [("h".to_string(), 1.0)].into(), json!({"k": [1, 2]}));
    let ops = p.build(&req).unwrap();
    let frame = Frame {
        origin: [0.0; 3],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        z: [0.0, 0.0, 1.0],
    };
    let expected = vec![
        Op::Prism {
            id: 7,
            body: "face".into(),
            frame,
            profile: Profile::Loop {
                segments: vec![
                    Segment::Line {
                        start: [-4.0, -1.0],
                    },
                    Segment::Arc {
                        start: [4.0, -1.0],
                        center: [4.0, 0.0],
                        ccw: true,
                    },
                    Segment::Line { start: [4.0, 1.0] },
                    Segment::Arc {
                        start: [-4.0, 1.0],
                        center: [-4.0, 0.0],
                        ccw: true,
                    },
                ],
            },
            holes: vec![Profile::Circle {
                center: [0.0, 0.0],
                radius: 0.5,
            }],
            from: -2.5,
            to: 1.0,
            taper_deg: 2.0,
            mode: Mode::Cut,
            label: "slot".into(),
        },
        Op::Prism {
            id: 8,
            body: "face".into(),
            frame,
            profile: Profile::Polygon {
                points: vec![[0.0, 0.0], [3.0, 0.0], [0.0, 3.0]],
            },
            holes: vec![],
            from: 0.0,
            to: 2.0,
            taper_deg: 0.0,
            mode: Mode::Join,
            label: String::new(),
        },
    ];
    assert_eq!(ops, expected);
    // The request on the wire: one JSON line naming the method, the request
    // with its tagged inputs.
    let sent = std::fs::read_to_string(tmp.path().join("t-roundtrip/request.json")).unwrap();
    assert!(sent.ends_with('\n') && sent.trim_end().lines().count() == 1);
    let sent: Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(sent["method"], "build");
    assert_eq!(sent["request"]["feature"], "f");
    assert_eq!(sent["request"]["params"]["h"], 1.0);
    assert_eq!(sent["request"]["data"], json!({"k": [1, 2]}));
    assert_eq!(sent["request"]["inputs"]["face"]["kind"], "face");
    assert_eq!(
        sent["request"]["inputs"]["face"]["body_span"],
        json!([-8.0, 0.0])
    );
    let back: BuildRequest = serde_json::from_value(sent["request"].clone()).unwrap();
    assert_eq!(back, req);
}

#[test]
fn other_methods_use_their_documented_requests_and_answers() {
    let tmp = tempfile::tempdir().unwrap();
    // Echo the request back as the answer of each method.
    let p = load_shell(
        tmp.path(),
        "t-methods",
        "printf '{\"ok\": '; cat; printf '}'\n",
        None,
    );
    let data = json!({"rule": "abs"});
    assert!(p.validate_data(&data).is_ok());
    // An echoed request is not DataEffects / a symbol map: those calls fall
    // back to their empty defaults rather than failing.
    assert_eq!(p.data_effects(&data), Default::default());
    assert!(p.rule_symbols(&data, Some(3)).is_empty());
    let p = load_shell(
        tmp.path(),
        "t-methods2",
        "cat > req.json\n\
         echo '{\"ok\": {\"parameters\": [{\"name\": \"ABS_T\", \"expression\": \"2 mm\", \"kind\": \"length\"}], \"materials\": [[1, \"abs\"]]}}'\n",
        None,
    );
    let e = p.data_effects(&data);
    assert_eq!(e.parameters.len(), 1);
    assert_eq!(e.parameters[0].name, "ABS_T");
    assert_eq!(e.materials, vec![(1, "abs".to_string())]);
    let sent: Value =
        serde_json::from_str(&std::fs::read_to_string(p.dir().join("req.json")).unwrap()).unwrap();
    assert_eq!(sent, json!({"method": "data_effects", "data": data}));
    let p = load_shell(
        tmp.path(),
        "t-methods3",
        "cat > req.json\necho '{\"ok\": {\"T\": \"ABS_T\"}}'\n",
        None,
    );
    assert_eq!(
        p.rule_symbols(&data, Some(3)),
        [("T".to_string(), "ABS_T".to_string())].into()
    );
    let sent: Value =
        serde_json::from_str(&std::fs::read_to_string(p.dir().join("req.json")).unwrap()).unwrap();
    assert_eq!(
        sent,
        json!({"method": "rule_symbols", "data": data, "body": 3})
    );
}

fn example() -> ProcessPlugin {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/vent-slots");
    ProcessPlugin::load(&dir).unwrap()
}

fn example_request(p: &ProcessPlugin, given: &[(&str, f64)]) -> BuildRequest {
    let f = p.manifest().feature("vent_slots").unwrap();
    let given: BTreeMap<String, f64> = given.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let (params, _) =
        rcad_plugin::schema::complete("vent_slots", &f.params, &[], &given, &BTreeMap::new())
            .unwrap();
    request("vent_slots", params, Value::Null)
}

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

#[test]
fn example_vent_slots_plugin_builds_the_expected_slots() {
    if !python3() {
        return;
    }
    let p = example();
    assert_eq!(p.manifest().id, "vent-slots");
    let (n, w, l, pitch, rot) = (3.0, 2.0, 20.0, 4.0, 30.0_f64);
    let req = example_request(
        &p,
        &[
            ("count", n),
            ("slot_width", w),
            ("slot_length", l),
            ("pitch", pitch),
            ("rotation_deg", rot),
        ],
    );
    // `build` already ran check_ops on the answer; check it again explicitly.
    let ops = p.build(&req).unwrap();
    rcad_plugin::check_ops(&req, &ops).unwrap();
    assert_eq!(ops.len(), 3);
    let (s, c) = rot.to_radians().sin_cos();
    let rotp = |u: f64, v: f64| [u * c - v * s, u * s + v * c];
    let (r, a) = (w / 2.0, (l - w) / 2.0);
    for (i, op) in ops.iter().enumerate() {
        let Op::Prism {
            id,
            body,
            frame,
            profile,
            holes,
            from,
            to,
            taper_deg,
            mode,
            ..
        } = op
        else {
            panic!("slot {i} is not a prism: {op:?}");
        };
        assert_eq!(*id as usize, i);
        assert_eq!(body, "face");
        assert_eq!(*frame, face_frame());
        assert!(holes.is_empty());
        // Through cut: from 1 mm below the body (span -8) to 1 mm above.
        assert_eq!((*from, *to, *taper_deg, *mode), (-9.0, 1.0, 0.0, Mode::Cut));
        // Slot i is centred (i - (n-1)/2) * pitch along the rotated y axis.
        let off = (i as f64 - (n - 1.0) / 2.0) * pitch;
        let Profile::Loop { segments } = profile else {
            panic!("slot {i} is not a loop: {profile:?}")
        };
        assert_eq!(segments.len(), 4);
        let expect = [
            (rotp(-a, off - r), None),
            (rotp(a, off - r), Some(rotp(a, off))),
            (rotp(a, off + r), None),
            (rotp(-a, off + r), Some(rotp(-a, off))),
        ];
        for (seg, (start, center)) in segments.iter().zip(expect) {
            match (seg, center) {
                (Segment::Line { start: s0 }, None) => assert!(close(*s0, start)),
                (
                    Segment::Arc {
                        start: s0,
                        center: c0,
                        ccw: true,
                    },
                    Some(ce),
                ) => assert!(close(*s0, start) && close(*c0, ce)),
                other => panic!("slot {i}: unexpected segment {other:?}"),
            }
        }
        // Area of a stadium: rectangle plus one full circle.
        let area = profile.area().unwrap();
        let expected = (l - w) * w + std::f64::consts::PI * r * r;
        assert!((area - expected).abs() < 1e-9, "{area} vs {expected}");
    }
}

#[test]
fn example_vent_slots_plugin_handles_depth_single_slots_and_refusals() {
    if !python3() {
        return;
    }
    let p = example();
    // Blind cut, one slot centred on the origin, as long as it is wide (a
    // circle).
    let req = example_request(
        &p,
        &[
            ("count", 1.0),
            ("depth", 3.0),
            ("slot_width", 5.0),
            ("slot_length", 5.0),
        ],
    );
    let ops = p.build(&req).unwrap();
    let [
        Op::Prism {
            profile, from, to, ..
        },
    ] = ops.as_slice()
    else {
        panic!("{ops:?}")
    };
    assert_eq!((*from, *to), (-3.0, 1.0));
    assert_eq!(
        *profile,
        Profile::Circle {
            center: [0.0, 0.0],
            radius: 2.5
        }
    );
    // Defaults: 5 slots, ids 0..5 (stable: the same ids for the same slots
    // when only sizes change).
    let ids = |r: &BuildRequest| -> Vec<u32> {
        p.build(r)
            .unwrap()
            .iter()
            .map(|op| match op {
                Op::Prism { id, .. }
                | Op::EdgeLoop { id, .. }
                | Op::ReplaceFace { id, .. }
                | Op::Countersink { id, .. } => *id,
            })
            .collect()
    };
    assert_eq!(ids(&example_request(&p, &[])), vec![0, 1, 2, 3, 4]);
    assert_eq!(
        ids(&example_request(&p, &[("slot_length", 12.0)])),
        vec![0, 1, 2, 3, 4]
    );
    // Stated refusals.
    for given in [
        &[("slot_length", 1.0)][..],
        &[("pitch", 2.0), ("slot_width", 2.0)][..],
    ] {
        match p.build(&example_request(&p, given)) {
            Err(PluginError::Unsupported(m)) => assert!(m.starts_with("vent-slots: "), "{m}"),
            other => panic!("expected a refusal for {given:?}, got {other:?}"),
        }
    }
    // The data methods have trivial answers.
    assert!(p.validate_data(&json!({"x": 1})).is_ok());
    assert_eq!(p.data_effects(&json!(null)), Default::default());
    assert!(p.rule_symbols(&json!(null), Some(1)).is_empty());
}

#[test]
fn an_error_answer_is_a_stated_refusal() {
    let tmp = tempfile::tempdir().unwrap();
    let p = load_shell(
        tmp.path(),
        "t-refuse",
        "cat >/dev/null\necho '{\"error\": \"cannot do\\u0007 that\"}'\n",
        None,
    );
    let r = p.validate_data(&Value::Null);
    // Control characters in plugin text are replaced.
    assert_eq!(
        r,
        Err(PluginError::Unsupported(
            "t-refuse: cannot do\u{FFFD} that".into()
        ))
    );
}

#[test]
fn a_crash_is_a_failure_quoting_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    let p = load_shell(
        tmp.path(),
        "t-crash",
        "cat >/dev/null\necho 'boom: no such frobnicator' >&2\nexit 3\n",
        None,
    );
    let m = failed(p.validate_data(&Value::Null));
    assert!(m.contains("boom: no such frobnicator"), "{m}");
    assert!(m.contains('3'), "{m}");
    // Killed by a signal.
    let p = load_shell(tmp.path(), "t-signal", "kill -TERM $$\n", None);
    let m = failed(p.validate_data(&Value::Null));
    assert!(m.contains("signal"), "{m}");
}

#[test]
fn a_program_that_hangs_is_killed_with_its_helpers_at_the_time_limit() {
    let tmp = tempfile::tempdir().unwrap();
    // The program records its own pid and that of a helper it starts, then
    // never answers.
    let p = load_shell(
        tmp.path(),
        "t-hang",
        "echo $$ > pid\nsleep 30 &\necho $! > helper\nsleep 5\n",
        Some(1),
    );
    let t = Instant::now();
    let m = failed(p.validate_data(&Value::Null));
    let took = t.elapsed();
    assert!(m.contains("no answer within 1 s"), "{m}");
    assert!(took < Duration::from_millis(2500), "took {took:?}");
    // The program was reaped by the call (no zombie left behind) ...
    let pid = read_pid(&p.dir().join("pid"));
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "program {pid} still exists"
    );
    // ... and its helper (same process group) was killed too.
    let helper = read_pid(&p.dir().join("helper"));
    let t = Instant::now();
    while running(helper) && t.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!running(helper), "helper {helper} still running");
}

#[test]
fn a_helper_holding_the_output_open_does_not_hang_the_call() {
    let tmp = tempfile::tempdir().unwrap();
    // The program answers and exits, but leaves a helper with its stdout.
    let p = load_shell(
        tmp.path(),
        "t-helper",
        "cat >/dev/null\necho '{\"ok\": null}'\nsleep 30 &\necho $! > helper\n",
        Some(1),
    );
    let t = Instant::now();
    let m = failed(p.validate_data(&Value::Null));
    assert!(m.contains("output stayed open"), "{m}");
    assert!(t.elapsed() < Duration::from_millis(2500));
    let helper = read_pid(&p.dir().join("helper"));
    let t = Instant::now();
    while running(helper) && t.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!running(helper), "helper {helper} still running");
}

#[test]
fn an_endless_answer_is_cut_off_and_the_program_killed() {
    let tmp = tempfile::tempdir().unwrap();
    let p = load_shell(
        tmp.path(),
        "t-flood",
        "echo $$ > pid\ncat >/dev/null\nwhile :; do echo '{\"ok\": [1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9,0,1,2,3,4,5,6,7,8,9]}'; done\n",
        Some(60),
    );
    // A faster flood than the shell loop when `yes` exists.
    let yes = std::process::Command::new("yes")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if yes {
        std::fs::write(
            p.dir().join("run.sh"),
            "echo $$ > pid\ncat >/dev/null\nyes '{\"ok\": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}'\n",
        )
        .unwrap();
    }
    let t = Instant::now();
    let m = failed(p.validate_data(&Value::Null));
    assert!(
        m.contains(&format!("larger than {MAX_ANSWER_BYTES} bytes")),
        "{m}"
    );
    assert!(
        t.elapsed() < Duration::from_secs(30),
        "took {:?}",
        t.elapsed()
    );
    let pid = read_pid(&p.dir().join("pid"));
    assert!(!Path::new(&format!("/proc/{pid}")).exists());
}

#[test]
fn malformed_answers_are_failures() {
    let tmp = tempfile::tempdir().unwrap();
    let cases: &[(&str, &str, &str)] = &[
        ("t-garbage", "echo 'this is not json'", "not valid"),
        ("t-empty", "true", "empty answer"),
        ("t-blank", "printf '\\n  \\n'", "empty answer"),
        ("t-binary", "printf '\\377\\376{'", "not valid"),
        ("t-trailing", "echo '{\"ok\": null} extra'", "not valid"),
        ("t-neither", "echo '{}'", "neither"),
        ("t-errtype", "echo '{\"error\": 5}'", "not valid"),
        ("t-array", "echo '[1, 2]'", "not valid"),
    ];
    for (id, body, want) in cases {
        let p = load_shell(tmp.path(), id, &format!("cat >/dev/null\n{body}\n"), None);
        let m = failed(p.validate_data(&Value::Null));
        assert!(m.contains(want), "{id}: {m}");
    }
    // Well-formed JSON with the wrong shape for a build.
    let req = request("f", [("h".to_string(), 1.0)].into(), Value::Null);
    for (id, body, want) in [
        ("t-okint", "echo '{\"ok\": 5}'", "operations"),
        (
            "t-badop",
            "echo '{\"ok\": [{\"op\": \"sphere\", \"id\": 0}]}'",
            "operations",
        ),
        ("t-okmissing", "echo '{\"ok\": null}'", "operations"),
    ] {
        let p = load_shell(tmp.path(), id, &format!("cat >/dev/null\n{body}\n"), None);
        let m = failed(p.build(&req));
        assert!(m.contains(want), "{id}: {m}");
    }
    // Parsed operations that fail the host's checks.
    let p = load_shell(
        tmp.path(),
        "t-unchecked",
        "cat >/dev/null\necho '{\"ok\": [{\"op\": \"prism\", \"id\": 0, \"body\": \"nope\", \"frame\": {\"origin\": [0,0,0], \"x\": [1,0,0], \"y\": [0,1,0], \"z\": [0,0,1]}, \"profile\": {\"shape\": \"circle\", \"center\": [0,0], \"radius\": 1}, \"from\": 0, \"to\": 1, \"mode\": \"cut\"}]}'\n",
        None,
    );
    let m = failed(p.build(&req));
    assert!(m.contains("not a face or body input"), "{m}");
    // `{"ok": null}` is a complete answer where nothing is returned.
    let p = load_shell(
        tmp.path(),
        "t-oknull",
        "cat >/dev/null\necho '{\"ok\": null}'\n",
        None,
    );
    assert_eq!(p.validate_data(&Value::Null), Ok(()));
}

#[test]
fn programs_that_ignore_their_input_neither_hang_nor_break_the_call() {
    let tmp = tempfile::tempdir().unwrap();
    // A request far larger than a pipe buffer.
    let big = json!({"blob": "x".repeat(4 * 1024 * 1024)});
    // Answers without reading, after writing more than a pipe buffer of
    // whitespace.
    let p = load_shell(
        tmp.path(),
        "t-noread",
        "head -c 300000 /dev/zero | tr '\\0' ' '\necho '{\"ok\": null}'\n",
        Some(10),
    );
    assert_eq!(p.validate_data(&big), Ok(()));
    // Exits at once without reading or answering: a broken pipe on the
    // request, then an empty answer.
    let p = load_shell(tmp.path(), "t-exit", "exit 0\n", Some(10));
    let m = failed(p.validate_data(&big));
    assert!(m.contains("empty answer"), "{m}");
    // Floods stderr (more than the kept part) without reading: stderr is
    // drained, so the program finishes; the kept text is capped.
    let p = load_shell(
        tmp.path(),
        "t-stderr",
        "head -c 1000000 /dev/zero | tr '\\0' 'e' >&2\nexit 1\n",
        Some(10),
    );
    let m = failed(p.validate_data(&big));
    assert!(m.contains("eeee"));
    assert!(m.len() < 20 * 1024, "{} bytes", m.len());
}

#[test]
fn programs_run_with_a_clean_environment() {
    let tmp = tempfile::tempdir().unwrap();
    let p = load_shell(
        tmp.path(),
        "t-env",
        "cat >/dev/null\nprintf '{\"ok\": {\"api\": \"%s\", \"dir\": \"%s\", \"pwd\": \"%s\", \"cargo\": \"%s\", \"path\": \"%s\"}}' \
         \"$RCAD_PLUGIN_API\" \"$RCAD_PLUGIN_DIR\" \"$(pwd -P)\" \"${CARGO_MANIFEST_DIR-unset}\" \"${PATH:+set}\"\n",
        None,
    );
    let req = rcad_plugin::AnalysisRequest {
        analysis: "a".into(),
        params: BTreeMap::new(),
        choices: BTreeMap::new(),
        inputs: BTreeMap::new(),
        data: Value::Null,
        bodies: vec![],
    };
    let v = p.analyze(&req).unwrap();
    let dir = p.dir().to_str().unwrap();
    assert_eq!(v["api"], rcad_plugin::API_VERSION.to_string());
    assert_eq!(v["dir"], dir);
    assert_eq!(v["pwd"], dir);
    // Variables of the host (cargo sets this one for tests) are not passed.
    assert_eq!(v["cargo"], "unset");
    if std::env::var_os("PATH").is_some() {
        assert_eq!(v["path"], "set");
    }
}

#[test]
fn relative_programs_resolve_in_the_plugin_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path().join("t-relprog");
    std::fs::create_dir_all(d.join("bin")).unwrap();
    let prog = d.join("bin/answer");
    std::fs::write(&prog, "#!/bin/sh\ncat >/dev/null\necho '{\"ok\": null}'\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
    let m = manifest("t-relprog", json!(["bin/answer"]), None);
    std::fs::write(d.join("plugin.json"), m.to_string()).unwrap();
    // Loaded through a relative-looking path with a `.` component, from
    // another working directory: the program still resolves.
    let p = ProcessPlugin::load(&tmp.path().join(".").join("t-relprog")).unwrap();
    assert_eq!(p.dir(), std::fs::canonicalize(&d).unwrap());
    assert_eq!(p.validate_data(&Value::Null), Ok(()));
}

fn manifest_error(root: &Path, name: &str, m: Value) -> String {
    let d = root.join(name);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("plugin.json"), m.to_string()).unwrap();
    match ProcessPlugin::load(&d) {
        Err(PluginError::Manifest(m)) => m,
        other => panic!("{name}: expected a manifest error, got {other:?}"),
    }
}

#[test]
fn invalid_manifests_are_refused_before_anything_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    let sh = json!(["sh", "run.sh"]);
    let m = manifest_error(r, "a", manifest("Bad ID", sh.clone(), None));
    assert!(m.contains("plugin id"), "{m}");
    let mut v = manifest("t-api", sh.clone(), None);
    v["api_version"] = json!(2);
    let m = manifest_error(r, "b", v);
    assert!(m.contains("interface version 2"), "{m}");
    let m = manifest_error(r, "c", manifest("t-empty", json!([]), None));
    assert!(m.contains("command is empty"), "{m}");
    let m = manifest_error(r, "d", manifest("t-noprog", json!([""]), None));
    assert!(m.contains("no program"), "{m}");
    for (n, c) in [
        ("e", json!(["../escape.sh"])),
        ("f", json!(["./sub/../../escape.sh"])),
        ("g", json!(["sh", "../outside/run.sh"])),
    ] {
        let m = manifest_error(r, n, manifest("t-dotdot", c, None));
        assert!(m.contains("'..'"), "{n}: {m}");
    }
    let m = manifest_error(r, "h", manifest("t-missing", json!(["./nothing.sh"]), None));
    assert!(m.contains("does not exist"), "{m}");
    let m = manifest_error(r, "i", manifest("t-timeout", sh.clone(), Some(0)));
    assert!(m.contains("timeout_secs 0"), "{m}");
    let m = manifest_error(r, "j", manifest("t-timeout", sh.clone(), Some(601)));
    assert!(m.contains("timeout_secs 601"), "{m}");
    let m = manifest_error(
        r,
        "k",
        json!({"id": "t-nocmd", "name": "x", "version": "1", "api_version": 1}),
    );
    assert!(m.contains("command"), "{m}");
    let mut v = manifest("t-nul", sh, None);
    v["command"] = json!(["sh", "a\u{0}b"]);
    let m = manifest_error(r, "l", v);
    assert!(m.contains("NUL"), "{m}");
    // Not a directory / no plugin.json / oversized plugin.json.
    assert!(matches!(
        ProcessPlugin::load(&r.join("does-not-exist")),
        Err(PluginError::Manifest(_))
    ));
    std::fs::create_dir_all(r.join("m")).unwrap();
    assert!(matches!(
        ProcessPlugin::load(&r.join("m")),
        Err(PluginError::Manifest(_))
    ));
    let d = r.join("n");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("plugin.json"), " ".repeat(1024 * 1024 + 1)).unwrap();
    match ProcessPlugin::load(&d) {
        Err(PluginError::Manifest(m)) => assert!(m.contains("larger than"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn load_installed_registers_plugins_and_reports_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("first");
    let b = tmp.path().join("second");
    let ok_script = "cat >/dev/null\necho '{\"ok\": null}'\n";
    shell_plugin(&a, "t-inst-ok", ok_script, None);
    shell_plugin(&a, "t-inst-builtin", ok_script, None);
    // A directory without plugin.json is not a plugin and not an error.
    std::fs::create_dir_all(a.join("not-a-plugin")).unwrap();
    // A broken manifest.
    std::fs::create_dir_all(a.join("broken")).unwrap();
    std::fs::write(a.join("broken/plugin.json"), "{").unwrap();
    // The same id again in a later directory.
    shell_plugin(&b, "t-inst-ok", ok_script, None);
    let missing = tmp.path().join("missing");
    let (ok, errs) = load_installed(&[missing, a.clone(), b.clone()], &["t-inst-builtin"]);
    assert_eq!(ok, vec!["t-inst-ok".to_string()]);
    let mut paths: Vec<PathBuf> = errs.iter().map(|(p, _)| p.clone()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![
            a.join("broken"),
            a.join("t-inst-builtin"),
            b.join("t-inst-ok")
        ]
    );
    for (p, e) in &errs {
        let m = e.to_string();
        if p.ends_with("t-inst-builtin") {
            assert!(m.contains("built-in"), "{m}");
        } else if p.starts_with(&b) {
            assert!(m.contains("earlier directory"), "{m}");
        }
    }
    let found = rcad_plugin::find("t-inst-ok").expect("registered");
    assert_eq!(found.validate_data(&Value::Null), Ok(()));
    assert!(rcad_plugin::find("t-inst-builtin").is_none());
    assert!(rcad_plugin::unregister("t-inst-ok"));
}

/// Run in a child process by `search_dirs_honours_rcad_plugin_path` (the
/// environment of this process is not changed by the tests): compares
/// `search_dirs()` with the `|`-separated `RCAD_SEARCH_DIRS_EXPECT`.
#[test]
#[ignore = "run by search_dirs_honours_rcad_plugin_path with its environment"]
fn search_dirs_probe() {
    let Some(expect) = std::env::var_os("RCAD_SEARCH_DIRS_EXPECT") else {
        return;
    };
    let expect: Vec<PathBuf> = expect
        .to_str()
        .unwrap()
        .split('|')
        .map(PathBuf::from)
        .collect();
    assert_eq!(search_dirs(), expect);
}

fn probe(env: &[(&str, &str)], expect: &[&str]) {
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["search_dirs_probe", "--exact", "--ignored", "--nocapture"])
        .env_remove("RCAD_PLUGIN_PATH")
        .env_remove("XDG_DATA_HOME")
        .env("RCAD_SEARCH_DIRS_EXPECT", expect.join("|"));
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && text.contains("1 passed"),
        "{env:?}: {text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn search_dirs_honours_rcad_plugin_path() {
    // Absolute entries first, in order; relative and empty entries ignored;
    // then the data directory.
    probe(
        &[
            (
                "RCAD_PLUGIN_PATH",
                "/opt/rcad/plugins:relative/dir::/srv/more",
            ),
            ("XDG_DATA_HOME", "/data/xdg"),
        ],
        &["/opt/rcad/plugins", "/srv/more", "/data/xdg/r-cad/plugins"],
    );
    // A relative XDG_DATA_HOME falls back to HOME.
    probe(
        &[("XDG_DATA_HOME", "rel"), ("HOME", "/data/home")],
        &["/data/home/.local/share/r-cad/plugins"],
    );
}
