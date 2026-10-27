//! Build the tests once, then run their binaries beside one another. Random boolean property
//! tests are built but not run here; `cargo xtask property-test` runs them explicitly.

use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

use crate::{cargo, root};

struct TestBin {
    label: String,
    exe: String,
    dir: String,
}

pub fn run(cargo_args: &[&str], test_args: &[&str], side: Vec<(String, Command)>) -> Result<(), String> {
    let bins = build(cargo_args)?;
    let start = Instant::now();
    let (tx, rx) = mpsc::channel::<(String, Result<String, String>, f64)>();
    let mut failed = Vec::new();
    std::thread::scope(|scope| {
        for b in &bins {
            let tx = tx.clone();
            scope.spawn(move || {
                let t0 = Instant::now();
                let mut c = Command::new(&b.exe);
                c.args(test_args).current_dir(&b.dir).env("CARGO_MANIFEST_DIR", &b.dir);
                let _ = tx.send((b.label.clone(), finish(c), t0.elapsed().as_secs_f64()));
            });
        }
        let tx_side = tx.clone();
        scope.spawn(move || {
            for (label, c) in side {
                let t0 = Instant::now();
                let _ = tx_side.send((label, finish(c), t0.elapsed().as_secs_f64()));
            }
        });
        drop(tx);
        for (label, result, secs) in rx {
            match result {
                Ok(summary) => println!("ok    {label:<44} {secs:6.1}s  {summary}"),
                Err(output) => {
                    println!("FAIL  {label:<44} {secs:6.1}s\n{output}");
                    failed.push(label);
                }
            }
        }
    });
    println!("test: {} binaries in {:.1}s", bins.len(), start.elapsed().as_secs_f64());
    if failed.is_empty() { Ok(()) } else { Err(format!("failed: {}", failed.join(", "))) }
}

fn finish(mut c: Command) -> Result<String, String> {
    let out = c.stdin(Stdio::null()).output().map_err(|e| format!("failed to spawn: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if out.status.success() {
        Ok(stdout.lines().rev().find(|l| l.starts_with("test result:")).unwrap_or("").trim_start_matches("test result: ").to_string())
    } else {
        Err(format!("{stdout}{}\n({})", String::from_utf8_lossy(&out.stderr), out.status))
    }
}

fn build(cargo_args: &[&str]) -> Result<Vec<TestBin>, String> {
    let mut c = cargo();
    c.args(["test", "--workspace", "--profile", "ci", "--no-run", "--message-format=json-render-diagnostics"]).args(cargo_args);
    eprintln!("$ cargo test --workspace --profile ci --no-run");
    let out = c.stderr(Stdio::inherit()).output().map_err(|e| format!("cargo test --no-run: failed to spawn: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo test --no-run: exited with {}", out.status));
    }
    let mut bins: Vec<TestBin> = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(m) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(exe) = m["executable"].as_str().filter(|_| m["reason"] == "compiler-artifact" && m["profile"]["test"] == true) else {
            continue;
        };
        if m["target"]["name"] == "boolean_props" || bins.iter().any(|b| b.exe == exe) {
            continue;
        }
        let manifest = std::path::Path::new(m["manifest_path"].as_str().unwrap_or_default()).to_path_buf();
        let dir = manifest.parent().map(|d| d.to_path_buf()).unwrap_or_else(root);
        let pkg = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let kind = m["target"]["kind"].as_array().and_then(|k| k.first()).and_then(|k| k.as_str()).unwrap_or("target");
        let label = format!("{pkg} {kind} {}", m["target"]["name"].as_str().unwrap_or(""));
        bins.push(TestBin { label, exe: exe.to_string(), dir: dir.to_string_lossy().into_owned() });
    }
    if bins.is_empty() {
        return Err("cargo test --no-run built no test executables".into());
    }
    Ok(bins)
}

pub fn doc_tests(cargo_args: &[&str], test_args: &[&str]) -> (String, Command) {
    let mut c = cargo();
    c.args(["test", "--workspace", "--profile", "ci", "--doc"]).args(cargo_args).arg("--").args(test_args);
    ("doc tests".into(), c)
}
