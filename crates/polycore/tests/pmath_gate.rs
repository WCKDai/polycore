//! No platform transcendental is called in the kernel, the geometry crate or the vendored
//! crates: every sine, exponential and logarithm goes through `pmath` (vendor/pmath), so that
//! every platform builds the same bits (PC-12). `clippy.toml`'s `disallowed-methods` enforces it:
//! `cargo xtask clippy` lints the workspace members with it, and this test lints the vendored
//! crates (excluded from the workspace) the same way, then plants a call of each kind in a probe
//! crate to show the lint catches it.
//!
//! The vendored crates are copied under `target/pmath-gate` and linted there as members of a
//! temporary workspace (Cargo wants members below the workspace root, and their own directories
//! stay untouched), lib targets only, with `CLIPPY_CONF_DIR` at the repository root so the
//! kernel's `clippy.toml` applies. The crates' own `deny(clippy::all)` would fail on lints a
//! newer Clippy adds, so every lint is capped to a warning and the `disallowed_methods`
//! diagnostics are read from the output: any one of them fails this test.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

const VENDOR: [&str; 12] = [
    "pmath",
    "matext4cgmath",
    "truck-base",
    "truck-derivers",
    "truck-geotrait",
    "truck-geometry",
    "truck-topology",
    "truck-polymesh",
    "truck-meshalgo",
    "truck-modeling",
    "truck-shapeops",
    "truck-stepio",
];

/// The planted calls, each on its own line of the probe crate, and the method clippy must name.
const PLANTED: [(&str, &str); 6] = [
    ("pub fn a(x: f64) -> f64 { x.sin() }", "f64::sin"),
    ("pub fn b(x: f64) -> f64 { x.powf(1.5) }", "f64::powf"),
    ("pub fn c(x: f64) -> f64 { f64::atan2(x, 1.0) }", "f64::atan2"),
    ("pub fn d(x: f32) -> f32 { x.cos() }", "f32::cos"),
    ("pub fn e(x: f64) -> f64 { x.mul_add(2.0, 1.0) }", "f64::mul_add"),
    ("pub fn f(x: f64) -> f64 { x.exp2() + x.ln_1p() }", "f64::exp2"),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// `from` copied to `to` (sources and manifests; no `target` directory, no lockfile).
fn copy_crate(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == "target" || name == "Cargo.lock" {
            continue;
        }
        let dest = to.join(&name);
        if entry.file_type().unwrap().is_dir() {
            copy_crate(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), &dest).unwrap();
        }
    }
}

/// The temporary workspace: copies of the vendored crates and the probe as members, made once.
static WORKSPACE: LazyLock<PathBuf> = LazyLock::new(|| {
    let root = root();
    let ws = root.join("target").join("pmath-gate");
    let vendor = ws.join("vendor");
    if vendor.exists() {
        std::fs::remove_dir_all(&vendor).unwrap();
    }
    for c in VENDOR {
        copy_crate(&root.join("vendor").join(c), &vendor.join(c));
    }
    let members: Vec<String> = VENDOR.iter().map(|c| format!("    \"vendor/{c}\",")).collect();
    write(
        &ws.join("Cargo.toml"),
        &format!(
            "[workspace]\nresolver = \"2\"\nmembers = [\n{}\n    \"probe\",\n]\n\n[profile.dev]\nopt-level = 0\ndebug = false\n",
            members.join("\n")
        ),
    );
    write(
        &ws.join("probe").join("Cargo.toml"),
        "[package]\nname = \"pmath-gate-probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\npath = \"lib.rs\"\n",
    );
    let planted: Vec<&str> = PLANTED.iter().map(|(line, _)| *line).collect();
    write(&ws.join("probe").join("lib.rs"), &format!("{}\n", planted.join("\n")));
    ws
});

fn clippy(args: &[&str]) -> (bool, String) {
    let ws = &*WORKSPACE;
    let mut c = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    c.current_dir(ws)
        .env("CLIPPY_CONF_DIR", root())
        .env("CARGO_TARGET_DIR", ws.join("target"))
        .env_remove("RUSTFLAGS")
        .args(["clippy", "--manifest-path"])
        .arg(ws.join("Cargo.toml"))
        .args(args)
        .args(["--lib", "--", "--cap-lints", "warn", "-W", "clippy::disallowed_methods"]);
    eprintln!("$ {c:?}");
    let out = c.output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

/// The methods and places clippy named in `use of a disallowed method` diagnostics.
fn flagged(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.split("use of a disallowed method `").nth(1) {
            let method = rest.split('`').next().unwrap_or("").to_string();
            let at = lines[i + 1..].iter().take(4).find_map(|l| l.trim().strip_prefix("--> ")).unwrap_or("").to_string();
            found.push((method, at));
        }
    }
    found
}

/// The vendored crates and `pmath` itself call no platform transcendental.
#[test]
fn vendored_crates_call_no_platform_transcendental() {
    let (ok, text) = clippy(&["--workspace", "--exclude", "pmath-gate-probe"]);
    let found = flagged(&text);
    assert!(ok && found.is_empty(), "clippy on the vendored crates: ok = {ok}, flagged {found:?}\n{text}");
    assert!(text.contains("Finished"), "{text}");
}

/// A new platform call is caught: each planted call is named with its line in the probe.
#[test]
fn a_new_platform_call_fails_the_lint() {
    let (ok, text) = clippy(&["-p", "pmath-gate-probe"]);
    assert!(ok, "the probe did not compile:\n{text}");
    let found = flagged(&text);
    for (i, (_, method)) in PLANTED.iter().enumerate() {
        let hit = found.iter().find(|(m, at)| m == method && at.contains(&format!("lib.rs:{}:", i + 1)));
        assert!(hit.is_some(), "{method} on probe line {} was not flagged; flagged {found:?}\n{text}", i + 1);
    }
    assert!(found.iter().all(|(_, at)| at.contains("probe")), "flagged outside the probe: {found:?}");
}
