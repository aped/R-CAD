//! `rcad-cli fusion-script SCRIPT.py [--out DOC.rcad.json] [--commands OUT.json]`
//! (docs/FUSION_SCRIPTS.md).
//!
//! Runs a Python script written for a subset of the Fusion add-in API surface under
//! `python3 -I` with R-CAD's `adsk` package (compat/fusion, embedded in this binary and
//! written to a private temporary directory) and a bootstrap that imports the script and
//! calls its `run(context)`. The package records every call as R-CAD commands; they are
//! then executed in a [`Session`] like `rcad-cli run`, printing one JSON line per step.
//!
//! Boss heights are resolved here, against the model at that point of the history: the
//! API places a boss by its top (its position points plus an offset), R-CAD's plastics boss
//! by its height above the face it stands on, so the face under each point is found
//! along the sketch normal and checked by point membership.
//!
//! Exit status: 0 success; 1 the script failed (Python error, unsupported member) or a
//! step was refused; 2 the script or an output file could not be read or written, or
//! Python is missing; 64 usage.
use crate::out::{esay, say};
use rcad_commands::{Command, Session};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The `adsk` package and the bootstrap, as (path in the temporary directory, text).
const PACKAGE: &[(&str, &str)] = &[
    (
        "rcad_fusion_bootstrap.py",
        include_str!("../../../compat/fusion/rcad_fusion_bootstrap.py"),
    ),
    (
        "adsk/__init__.py",
        include_str!("../../../compat/fusion/adsk/__init__.py"),
    ),
    (
        "adsk/_recorder.py",
        include_str!("../../../compat/fusion/adsk/_recorder.py"),
    ),
    (
        "adsk/core.py",
        include_str!("../../../compat/fusion/adsk/core.py"),
    ),
    (
        "adsk/fusion.py",
        include_str!("../../../compat/fusion/adsk/fusion.py"),
    ),
    (
        "adsk/cam.py",
        include_str!("../../../compat/fusion/adsk/cam.py"),
    ),
];

/// One recorded step: the command, the feature and body ids the recorder predicts it
/// creates, and values to resolve against the model before it runs.
#[derive(Deserialize)]
struct Step {
    command: Value,
    #[serde(default)]
    feature: Option<u32>,
    #[serde(default)]
    body: Option<u32>,
    #[serde(default)]
    resolve: Option<Resolve>,
}

#[derive(Deserialize)]
struct Resolve {
    #[serde(default)]
    boss: Option<BossResolve>,
}

/// A boss placed by its top: `offset` mm above its position points (sketch `sketch`,
/// local mm) on body `body`.
#[derive(Deserialize)]
struct BossResolve {
    sketch: u32,
    body: u32,
    points: Vec<[f64; 2]>,
    offset: f64,
    /// The pilot hole runs the full height of the boss.
    #[serde(default)]
    hole_full: bool,
    /// Ribs end this far below the boss top.
    #[serde(default)]
    rib_offset: Option<f64>,
}

#[derive(Deserialize)]
struct Recorded {
    #[serde(default)]
    steps: Vec<Step>,
    #[serde(default)]
    error: Option<ScriptError>,
}

#[derive(Deserialize)]
struct ScriptError {
    kind: String,
    message: String,
    #[serde(default)]
    traceback: String,
}

fn usage() -> ExitCode {
    esay!("usage: rcad-cli fusion-script SCRIPT.py [--out DOC.rcad.json] [--commands OUT.json]");
    ExitCode::from(64)
}

/// A private temporary directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<TempDir> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("rcad-fusion-{}-{nanos}", std::process::id()));
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        b.create(&dir)?;
        Ok(TempDir(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `fusion-script` with its arguments (after the word `fusion-script`).
pub fn run(args: &[String]) -> ExitCode {
    let mut script = None;
    let (mut out, mut commands) = (None, None);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" | "--commands" => {
                let Some(v) = args.get(i + 1) else {
                    return usage();
                };
                let slot = if args[i] == "--out" {
                    &mut out
                } else {
                    &mut commands
                };
                *slot = Some(PathBuf::from(v));
                i += 2;
            }
            a if a.starts_with('-') => return usage(),
            a => {
                if script.replace(PathBuf::from(a)).is_some() {
                    return usage();
                }
                i += 1;
            }
        }
    }
    let Some(script) = script else {
        return usage();
    };
    if !script.is_file() {
        esay!("cannot read {}: not a file", script.display());
        return ExitCode::from(2);
    }
    let steps = match record(&script) {
        Ok(s) => s,
        Err(code) => return code,
    };
    crate::load_plugins();
    execute(steps, out.as_deref(), commands.as_deref())
}

/// Run the script under Python and return the recorded steps.
fn record(script: &Path) -> Result<Vec<Step>, ExitCode> {
    let tmp = TempDir::new().map_err(|e| {
        esay!("cannot create a temporary directory: {e}");
        ExitCode::from(2)
    })?;
    for (rel, text) in PACKAGE {
        let p = tmp.0.join(rel);
        let written = p
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&p, text));
        if let Err(e) = written {
            esay!("cannot write {}: {e}", p.display());
            return Err(ExitCode::from(2));
        }
    }
    let result_path = tmp.0.join("recorded.json");
    let python = std::env::var_os("RCAD_PYTHON").unwrap_or_else(|| "python3".into());
    let output = std::process::Command::new(&python)
        .arg("-I")
        .arg(tmp.0.join("rcad_fusion_bootstrap.py"))
        .arg(script)
        .arg(&result_path)
        .stdin(std::process::Stdio::null())
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) => {
            esay!(
                "cannot run {}: {e} (fusion-script needs Python 3; set RCAD_PYTHON to choose \
                 the interpreter)",
                python.to_string_lossy()
            );
            return Err(ExitCode::from(2));
        }
    };
    // The script's own output (message boxes, prints) goes to standard error, so standard
    // output stays JSON lines.
    for text in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(text);
        for line in text.lines() {
            esay!("{line}");
        }
    }
    let recorded: Recorded = match std::fs::read_to_string(&result_path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(r) => r,
        Err(e) => {
            esay!(
                "fusion-script: Python exited with {} without a result ({e})",
                output.status
            );
            return Err(ExitCode::FAILURE);
        }
    };
    if let Some(err) = recorded.error {
        report_error(&err, script);
        return Err(ExitCode::FAILURE);
    }
    if !output.status.success() {
        esay!("fusion-script: Python exited with {}", output.status);
        return Err(ExitCode::FAILURE);
    }
    Ok(recorded.steps)
}

fn report_error(err: &ScriptError, script: &Path) {
    match err.kind.as_str() {
        "not_supported" => esay!("fusion-script: not supported: {}", err.message),
        "script" => esay!("fusion-script: {}", err.message),
        _ => esay!("fusion-script: Python error: {}", err.message),
    }
    if err.traceback.is_empty() {
        return;
    }
    if err.kind == "python" {
        esay!("{}", err.traceback.trim_end());
        return;
    }
    // For an unsupported member, the script's own lines are what matters.
    let name = script.file_name().map(|n| n.to_string_lossy().into_owned());
    let lines: Vec<&str> = err.traceback.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        if name.as_deref().is_some_and(|n| l.contains(n)) && l.trim_start().starts_with("File") {
            esay!("{l}");
            if let Some(next) = lines.get(i + 1) {
                esay!("{next}");
            }
        }
    }
}

fn execute(steps: Vec<Step>, out: Option<&Path>, commands: Option<&Path>) -> ExitCode {
    let mut session = Session::new();
    let mut resolved: Vec<Value> = Vec::with_capacity(steps.len());
    let mut failed = false;
    for (i, step) in steps.into_iter().enumerate() {
        if failed {
            resolved.push(step.command);
            continue;
        }
        let mut value = step.command;
        if let Some(b) = step.resolve.as_ref().and_then(|r| r.boss.as_ref())
            && let Err(e) = resolve_boss(&session, b, &mut value)
        {
            esay!("fusion-script: step {i}: {e}");
            failed = true;
            resolved.push(value);
            continue;
        }
        let cmd: Command = match serde_json::from_value(value.clone()) {
            Ok(c) => c,
            Err(e) => {
                esay!("fusion-script: step {i}: the recorder produced an invalid command: {e}");
                failed = true;
                resolved.push(value);
                continue;
            }
        };
        resolved.push(value);
        let t0 = std::time::Instant::now();
        let mut r = session.execute(&cmd);
        r.elapsed_ms = Some(t0.elapsed().as_secs_f64() * 1e3);
        r.summary = None;
        let mut problem = None;
        if !r.ok {
            problem = Some(match &r.error {
                Some(e) => format!("{} was refused: {e}", cmd.name()),
                None => format!("{} was refused", cmd.name()),
            });
        } else if let Some(f) = step.feature
            && r.feature.map(|x| x.0) != Some(f)
        {
            problem = Some(format!(
                "{} created feature {:?}, the recorder expected {f}",
                cmd.name(),
                r.feature.map(|x| x.0)
            ));
        } else if let Some(b) = step.body {
            let created = r.feature.and_then(|f| {
                session
                    .summary()
                    .features
                    .into_iter()
                    .find(|s| s.id == f)
                    .map(|s| s.creates)
            });
            if !created.is_some_and(|c| c.iter().any(|x| x.0 == b)) {
                problem = Some(format!(
                    "{} did not create body {b} as the recorder expected",
                    cmd.name()
                ));
            }
        }
        let ok = problem.is_none();
        say!(
            "{}",
            serde_json::json!({ "step": i, "as_expected": ok, "response": r })
        );
        if let Some(p) = problem {
            esay!("fusion-script: step {i}: {p}");
            failed = true;
        }
    }
    if let Some(path) = commands {
        match serde_json::to_vec_pretty(&resolved) {
            Ok(j) => {
                if let Err(e) = std::fs::write(path, j) {
                    esay!("cannot write {}: {e}", path.display());
                    return ExitCode::from(2);
                }
            }
            Err(e) => {
                esay!("cannot serialize the commands: {e}");
                return ExitCode::from(2);
            }
        }
    }
    if failed {
        return ExitCode::FAILURE;
    }
    if let Some(path) = out {
        let cmd = Command::Save {
            path: path.to_path_buf(),
        };
        let r = session.execute(&cmd);
        let ok = r.ok;
        say!(
            "{}",
            serde_json::json!({ "step": resolved.len(), "as_expected": ok, "response": r })
        );
        if !ok {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Probe distance (mm) on either side of a candidate base face.
const PROBE: f64 = 1e-3;
/// Parallel-plane and same-height tolerances.
const PARALLEL: f64 = 1e-9;
const SAME: f64 = 1e-6;

/// Set the boss's `height` (and a full-depth hole, a rib offset) from the model: for
/// each position point, the nearest face of the body at or below it, facing along the
/// sketch normal, with material just below it and none just above, under the point.
fn resolve_boss(session: &Session, b: &BossResolve, cmd: &mut Value) -> Result<(), String> {
    let summary = session.summary();
    let frame = summary
        .features
        .iter()
        .find(|f| f.id.0 == b.sketch)
        .and_then(|f| f.frame)
        .ok_or_else(|| format!("boss: sketch {} has no plane", b.sketch))?;
    let body = summary
        .bodies
        .iter()
        .find(|x| x.id.0 == b.body)
        .ok_or_else(|| format!("boss: body {} does not exist", b.body))?;
    let brep = session
        .model()
        .and_then(|m| m.evaluation().body_by_id(body.id))
        .and_then(|e| e.geometry.brep())
        .ok_or_else(|| format!("boss: body {} is not a solid", b.body))?;
    let n = frame.z;
    let mut height: Option<f64> = None;
    for p in &b.points {
        let world = [0, 1, 2].map(|k| frame.origin[k] + p[0] * frame.x[k] + p[1] * frame.y[k]);
        let mut cands: Vec<f64> = body
            .planes
            .iter()
            .filter(|pl| dot(pl.normal, n) > 1.0 - PARALLEL)
            .map(|pl| dot([0, 1, 2].map(|k| world[k] - pl.point[k]), n))
            .filter(|s| *s >= -SAME)
            .collect();
        cands.sort_by(f64::total_cmp);
        let mut base = None;
        for s in cands {
            let below = [0, 1, 2].map(|k| world[k] - n[k] * (s + PROBE));
            let above = [0, 1, 2].map(|k| world[k] - n[k] * (s - PROBE));
            let m = rcad_kernel::point_membership(brep, &[below, above])
                .map_err(|e| format!("boss: cannot probe the body: {e}"))?;
            if m == [Some(true), Some(false)] {
                base = Some(s.max(0.0));
                break;
            }
        }
        let s = base.ok_or_else(|| {
            format!(
                "boss: no face of body {} under the position point ({}, {}) facing the sketch \
                 normal",
                b.body, p[0], p[1]
            )
        })?;
        let h = s + b.offset;
        match height {
            Some(h0) if (h0 - h).abs() > SAME => {
                return Err(format!(
                    "boss: the position points stand {h0} and {h} mm above their faces; R-CAD \
                     bosses in one feature share one height"
                ));
            }
            _ => height = Some(h),
        }
    }
    let h = height.ok_or("boss: no position points")?;
    if h <= SAME {
        return Err(format!(
            "boss: its top would be {h} mm above the face it stands on; set \
             BossFeatureInput.offset to the boss height"
        ));
    }
    let params = cmd
        .get_mut("params")
        .and_then(Value::as_object_mut)
        .ok_or("boss: the recorded command has no params")?;
    params.insert("height".into(), h.into());
    if b.hole_full {
        params.insert("hole_depth".into(), h.into());
    }
    if let Some(off) = b.rib_offset {
        params.insert("rib_height".into(), (h - off).into());
    }
    Ok(())
}
