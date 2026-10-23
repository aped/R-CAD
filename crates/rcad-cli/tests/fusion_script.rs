//! `rcad-cli fusion-script` (docs/FUSION_SCRIPTS.md) through the built binary: the
//! examples against hand-derived closed-form volumes, unit conversion, unsupported members,
//! script errors, and the Python unit tests of the recorder.
//!
//! Every test is skipped, with a note on standard error, when `python3` is not installed.
use serde_json::Value;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_rcad-cli");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn have_python() -> bool {
    let ok = Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("python3 is not installed: fusion-script tests skipped");
    }
    ok
}

fn cli(args: &[&Path]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("XDG_DATA_HOME", tmp_data())
        .env_remove("RCAD_PLUGIN_PATH")
        .output()
        .expect("run rcad-cli")
}

/// An empty data directory, so no installed external plugins are loaded.
fn tmp_data() -> PathBuf {
    let d = std::env::temp_dir().join(format!("rcad-fusion-test-data-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// `fusion-script SCRIPT --out DOC --commands CMDS` in `dir`.
fn fusion(script: &Path, dir: &Path) -> (Output, PathBuf, PathBuf) {
    let doc = dir.join("doc.rcad.json");
    let cmds = dir.join("commands.json");
    let o = cli(&[
        Path::new("fusion-script"),
        script,
        Path::new("--out"),
        &doc,
        Path::new("--commands"),
        &cmds,
    ]);
    (o, doc, cmds)
}

/// Volumes of the bodies of a saved document, by opening it with `rcad-cli run`.
fn volumes(doc: &Path, dir: &Path) -> Vec<f64> {
    let script = dir.join("inspect.json");
    let steps = serde_json::json!([{"cmd": "open", "path": doc}, {"cmd": "inspect"}]);
    std::fs::write(&script, steps.to_string()).unwrap();
    let o = cli(&[Path::new("run"), &script]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    let out = text(&o.stdout);
    let last: Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
    last["response"]["summary"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            b["measurements"]["volume"]["Value"]["volume"]
                .as_f64()
                .unwrap_or_else(|| panic!("no volume: {b}"))
        })
        .collect()
}

fn assert_close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() <= 1e-6 * want.abs(),
        "{what}: got {got}, want {want}"
    );
}

/// Run an example, check it succeeded, that its recorded commands replay with `run`,
/// and return the saved body volumes.
fn example(name: &str) -> Vec<f64> {
    let dir = tempfile::tempdir().unwrap();
    let script = repo().join("compat/fusion/examples").join(name);
    let (o, doc, cmds) = fusion(&script, dir.path());
    assert!(
        o.status.success(),
        "{name}: {}\n{}",
        text(&o.stdout),
        text(&o.stderr)
    );
    for line in text(&o.stdout).lines() {
        let v: Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["as_expected"], true, "{line}");
    }
    let replay = cli(&[Path::new("run"), &cmds]);
    assert!(replay.status.success(), "{}", text(&replay.stdout));
    volumes(&doc, dir.path())
}

#[test]
fn plate_with_hole_has_the_closed_form_volume() {
    if !have_python() {
        return;
    }
    // 100 x 60 mm plate, 8 mm (0.8 cm) thick, minus a 7.5 mm radius hole through it.
    let want = 100.0 * 60.0 * 8.0 - PI * 7.5 * 7.5 * 8.0;
    let v = example("plate_with_hole.py");
    assert_eq!(v.len(), 1);
    assert_close(v[0], want, "plate");
}

#[test]
fn plastic_box_has_the_closed_form_volume() {
    if !have_python() {
        return;
    }
    // 80 x 50 x 25 box shelled to 2 mm walls and floor: 80*50*25 - 76*46*23.
    let shell = 80.0 * 50.0 * 25.0 - 76.0 * 46.0 * 23.0;
    // Four bosses on the floor (z = 2), 15 mm high (offset 1.5 cm), 6 mm across, with
    // 2.5 mm pilot holes 6 mm deep.
    let bosses = 4.0 * (PI * 9.0 * 15.0 - PI * 1.25 * 1.25 * 6.0);
    // Web at x = 0 from the rim (z = 25) to the floor (z = 2), 2 mm thick, across the
    // 46 mm cavity.
    let web = 46.0 * 2.0 * 23.0;
    // Rib at y = 0 (XZ plane), 2 mm thick, from z = 15 down to the floor, across the
    // 76 mm cavity, less the 2 x 2 x 13 mm where it crosses the web.
    let rib = 76.0 * 2.0 * 13.0 - 2.0 * 2.0 * 13.0;
    let v = example("plastic_box.py");
    assert_eq!(v.len(), 1);
    assert_close(v[0], shell + bosses + web + rib, "plastic box");
}

fn write_script(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("script.py");
    let header = "import adsk.core, adsk.fusion, adsk.cam, traceback\n\
                  V = adsk.core.ValueInput\nP = adsk.core.Point3D\n";
    std::fs::write(&p, format!("{header}{body}")).unwrap();
    p
}

const BLOCK: &str = "
def block(root, d):
    sk = root.sketches.add(root.xYConstructionPlane)
    sk.sketchCurves.sketchLines.addTwoPointRectangle(P.create(0, 0, 0), P.create(2, 1, 0))
    return root.features.extrudeFeatures.addSimple(
        sk.profiles.item(0), d, adsk.fusion.FeatureOperations.NewBodyFeatureOperation)
";

#[test]
fn reals_are_centimetres_and_strings_carry_units() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = write_script(
        dir.path(),
        &format!(
            "{BLOCK}
def run(context):
    root = adsk.core.Application.get().activeProduct.rootComponent
    block(root, V.createByReal(1.5))
    block(root, V.createByString('0.25 in'))
"
        ),
    );
    let (o, doc, cmds) = fusion(&script, dir.path());
    assert!(o.status.success(), "{}", text(&o.stderr));
    let cmds: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(cmds).unwrap()).unwrap();
    let d: Vec<f64> = cmds
        .iter()
        .filter(|c| c["cmd"] == "extrude")
        .map(|c| c["distance"].as_f64().unwrap())
        .collect();
    assert_eq!(d, [15.0, 6.35]);
    // The sketch is 2 x 1 cm = 20 x 10 mm.
    let v = volumes(&doc, dir.path());
    assert_close(v[0], 20.0 * 10.0 * 15.0, "1.5 cm block");
    assert_close(v[1], 20.0 * 10.0 * 6.35, "0.25 in block");
}

#[test]
fn an_unsupported_member_fails_with_its_name() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = write_script(
        dir.path(),
        "
def run(context):
    root = adsk.core.Application.get().activeProduct.rootComponent
    root.features.loftFeatures.createInput(adsk.fusion.FeatureOperations.NewBodyFeatureOperation)
",
    );
    let (o, doc, _) = fusion(&script, dir.path());
    assert_eq!(o.status.code(), Some(1));
    let err = text(&o.stderr);
    assert!(
        err.contains(
            "not supported: adsk.fusion.Features.loftFeatures is not in R-CAD's Fusion API subset"
        ),
        "{err}"
    );
    assert!(err.contains("script.py"), "the script line is shown: {err}");
    assert!(o.stdout.is_empty(), "nothing is executed");
    assert!(!doc.exists());
}

#[test]
fn an_unsupported_member_caught_by_the_script_still_fails() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = write_script(
        dir.path(),
        &format!(
            "{BLOCK}
def run(context):
    ui = None
    try:
        app = adsk.core.Application.get()
        ui = app.userInterface
        root = app.activeProduct.rootComponent
        block(root, V.createByReal(1))
        root.features.chamferFeatures
    except:
        if ui:
            ui.messageBox('Failed:\\n{{}}'.format(traceback.format_exc()))
"
        ),
    );
    let (o, _, _) = fusion(&script, dir.path());
    assert_eq!(o.status.code(), Some(1));
    let err = text(&o.stderr);
    assert!(
        err.contains("adsk.fusion.Features.chamferFeatures is not in R-CAD's Fusion API subset"),
        "{err}"
    );
    assert!(err.contains("which caught it"), "{err}");
}

#[test]
fn python_errors_and_missing_scripts_are_reported() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = write_script(dir.path(), "def run(context):\n    1 / 0\n");
    let (o, _, _) = fusion(&script, dir.path());
    assert_eq!(o.status.code(), Some(1));
    let err = text(&o.stderr);
    assert!(err.contains("Python error: ZeroDivisionError"), "{err}");
    assert!(err.contains("Traceback"), "{err}");

    let script = write_script(dir.path(), "x = 1\n");
    let (o, _, _) = fusion(&script, dir.path());
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stderr).contains("no run(context) function"));

    let (o, _, _) = fusion(&dir.path().join("missing.py"), dir.path());
    assert_eq!(o.status.code(), Some(2));
    let o = cli(&[Path::new("fusion-script")]);
    assert_eq!(o.status.code(), Some(64));
}

#[test]
fn a_refused_step_stops_the_run() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // A cut pointing away from the top face removes nothing: refused, exit 1.
    let script = write_script(
        dir.path(),
        &format!(
            "{BLOCK}
def run(context):
    root = adsk.core.Application.get().activeProduct.rootComponent
    e = block(root, V.createByReal(1))
    sk = root.sketches.add(e.endFaces.item(0))
    sk.sketchCurves.sketchCircles.addByCenterRadius(P.create(1, 0.5, 0), 0.2)
    root.features.extrudeFeatures.addSimple(
        sk.profiles.item(0), V.createByReal(0.5), adsk.fusion.FeatureOperations.CutFeatureOperation)
"
        ),
    );
    let (o, doc, cmds) = fusion(&script, dir.path());
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o.stderr).contains("extrude was refused"),
        "{}",
        text(&o.stderr)
    );
    assert!(!doc.exists(), "nothing is saved after a refusal");
    assert!(cmds.exists(), "the recorded commands are still written");
}

#[test]
fn recorder_unit_tests_pass() {
    if !have_python() {
        return;
    }
    let o = Command::new("python3")
        .args(["-I", "-m", "unittest", "discover", "-s"])
        .arg(repo().join("compat/fusion/tests"))
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{}\n{}",
        text(&o.stdout),
        text(&o.stderr)
    );
}
