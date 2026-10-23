//! Out-of-process plugins: a directory holding `plugin.json` (a
//! [`Manifest`] plus a `command`) and whatever the command runs. Each call
//! starts the command in the plugin directory, writes one JSON request to its
//! standard input and reads one JSON answer from its standard output:
//!
//! ```text
//! -> {"method": "build",    "request": BuildRequest}
//! <- {"ok": [Op, ...]}            or {"error": "message"}
//! -> {"method": "analyze",  "request": AnalysisRequest}
//! <- {"ok": <report JSON>}        or {"error": "message"}
//! -> {"method": "validate_data", "data": <JSON>}
//! <- {"ok": null}                 or {"error": "message"}
//! -> {"method": "data_effects", "data": <JSON>}
//! <- {"ok": DataEffects}
//! -> {"method": "rule_symbols", "data": <JSON>, "body": 3}
//! <- {"ok": {"T": "ABS_T", ...}}
//! ```
//!
//! The answer is untrusted: it is size-limited, time-limited, parsed into
//! typed values and checked ([`crate::check_ops`]) before the document uses
//! it. Documents never carry plugin code; only installed plugins run.
use crate::{AnalysisRequest, BuildRequest, Manifest, Op, Plugin, PluginError};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Largest accepted answer.
pub const MAX_ANSWER_BYTES: u64 = 64 * 1024 * 1024;
/// Largest accepted `plugin.json`.
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Default time limit of one call.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestFile {
    #[serde(flatten)]
    manifest: Manifest,
    /// Program and arguments, run in the plugin directory. A relative
    /// program path with a `/` is resolved against that directory.
    command: Vec<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// A plugin served by an external program.
#[derive(Debug)]
pub struct ProcessPlugin {
    manifest: Manifest,
    dir: PathBuf,
    command: Vec<String>,
    timeout: Duration,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    ok: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<String>,
}

impl ProcessPlugin {
    /// Load the plugin in directory `dir` (reads and validates `plugin.json`;
    /// runs nothing).
    pub fn load(dir: &Path) -> Result<ProcessPlugin, PluginError> {
        let path = dir.join("plugin.json");
        let bad = |m: String| PluginError::Manifest(format!("{}: {m}", path.display()));
        let meta = std::fs::metadata(&path).map_err(|e| bad(e.to_string()))?;
        if meta.len() > MAX_MANIFEST_BYTES {
            return Err(bad(format!("larger than {MAX_MANIFEST_BYTES} bytes")));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| bad(e.to_string()))?;
        let f: ManifestFile = serde_json::from_str(&text).map_err(|e| bad(e.to_string()))?;
        f.manifest.validate()?;
        if f.command.is_empty() {
            return Err(bad("command is empty".into()));
        }
        let timeout = match f.timeout_secs {
            Some(s) if (1..=600).contains(&s) => Duration::from_secs(s),
            Some(s) => return Err(bad(format!("timeout_secs {s} must be 1..=600"))),
            None => DEFAULT_TIMEOUT,
        };
        Ok(ProcessPlugin {
            manifest: f.manifest,
            dir: dir.to_path_buf(),
            command: f.command,
            timeout,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn call(&self, request: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        let id = &self.manifest.id;
        let fail = |m: String| PluginError::Failed(format!("{id}: {m}"));
        let mut prog = PathBuf::from(&self.command[0]);
        if prog.is_relative() && self.command[0].contains('/') {
            prog = self.dir.join(prog);
        }
        let mut child = Command::new(&prog)
            .args(&self.command[1..])
            .current_dir(&self.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| fail(format!("cannot start {}: {e}", prog.display())))?;
        let body = serde_json::to_vec(request).map_err(|e| fail(e.to_string()))?;
        // Write and read on separate threads so neither pipe can fill up and
        // block the other side.
        let mut stdin = child.stdin.take().ok_or_else(|| fail("no stdin".into()))?;
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&body);
        });
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| fail("no stdout".into()))?;
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let r = stdout.take(MAX_ANSWER_BYTES + 1).read_to_end(&mut buf);
            r.map(|_| buf)
        });
        let stderr = child.stderr.take();
        let err_reader = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(e) = stderr {
                let _ = e.take(16 * 1024).read_to_string(&mut s);
            }
            s
        });
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if start.elapsed() > self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(fail(format!(
                        "no answer within {} s",
                        self.timeout.as_secs()
                    )));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => return Err(fail(e.to_string())),
            }
        };
        let _ = writer.join();
        let out = reader
            .join()
            .map_err(|_| fail("reader panicked".into()))?
            .map_err(|e| fail(e.to_string()))?;
        let err = err_reader.join().unwrap_or_default();
        if out.len() as u64 > MAX_ANSWER_BYTES {
            return Err(fail(format!("answer larger than {MAX_ANSWER_BYTES} bytes")));
        }
        if !status.success() {
            return Err(fail(format!("exited with {status}: {}", err.trim())));
        }
        let a: Answer =
            serde_json::from_slice(&out).map_err(|e| fail(format!("answer is not valid: {e}")))?;
        match (a.ok, a.error) {
            (_, Some(e)) => Err(PluginError::Unsupported(format!("{id}: {e}"))),
            (Some(v), None) => Ok(v),
            (None, None) => Ok(serde_json::Value::Null),
        }
    }
}

impl Plugin for ProcessPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn build(&self, req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
        let v = self.call(&serde_json::json!({"method": "build", "request": req}))?;
        let ops: Vec<Op> = serde_json::from_value(v)
            .map_err(|e| PluginError::Failed(format!("{}: operations: {e}", self.manifest.id)))?;
        crate::check_ops(req, &ops)?;
        Ok(ops)
    }

    fn analyze(&self, req: &AnalysisRequest) -> Result<serde_json::Value, PluginError> {
        self.call(&serde_json::json!({"method": "analyze", "request": req}))
    }

    fn validate_data(&self, data: &serde_json::Value) -> Result<(), PluginError> {
        self.call(&serde_json::json!({"method": "validate_data", "data": data}))
            .map(|_| ())
    }

    fn data_effects(&self, data: &serde_json::Value) -> crate::DataEffects {
        self.call(&serde_json::json!({"method": "data_effects", "data": data}))
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    fn rule_symbols(
        &self,
        data: &serde_json::Value,
        body: Option<u32>,
    ) -> std::collections::BTreeMap<String, String> {
        self.call(&serde_json::json!({"method": "rule_symbols", "data": data, "body": body}))
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }
}

/// Directories searched for installed plugins: `RCAD_PLUGIN_PATH`
/// (`:`-separated, `;` on Windows) first, then `<data dir>/r-cad/plugins`
/// (`$XDG_DATA_HOME`, else `~/.local/share`; `%APPDATA%` on Windows).
pub fn search_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("RCAD_PLUGIN_PATH") {
        v.extend(std::env::split_paths(&p).filter(|p| p.is_absolute()));
    }
    let data = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    if let Some(d) = data {
        v.push(d.join("r-cad").join("plugins"));
    }
    v
}

/// Load and register every plugin under `dirs` (each subdirectory with a
/// `plugin.json`). A built-in plugin's id is never replaced by an external
/// one. Returns the registered ids and the per-directory errors.
pub fn load_installed(
    dirs: &[PathBuf],
    builtin: &[&str],
) -> (Vec<String>, Vec<(PathBuf, PluginError)>) {
    let mut ok = Vec::new();
    let mut errs = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else {
            continue;
        };
        let mut subs: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.join("plugin.json").is_file())
            .collect();
        subs.sort();
        for s in subs {
            match ProcessPlugin::load(&s) {
                Ok(p) if builtin.contains(&p.manifest.id.as_str()) => errs.push((
                    s,
                    PluginError::Manifest(format!(
                        "'{}' is a built-in plugin id; not replaced",
                        p.manifest.id
                    )),
                )),
                Ok(p) => {
                    let id = p.manifest.id.clone();
                    match crate::register(Arc::new(p)) {
                        Ok(()) => ok.push(id),
                        Err(e) => errs.push((s, e)),
                    }
                }
                Err(e) => errs.push((s, e)),
            }
        }
    }
    (ok, errs)
}
