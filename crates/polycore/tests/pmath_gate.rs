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
//! kernel's `clippy.toml` applies. The workspace resolves offline from a copy of the repository's
//! `Cargo.lock`, so it lints the locked versions: a member's own manifest would bring its
//! dev-dependencies and every optional dependency into the lockfile, which the kernel's lockfile
//! does not hold (ruststep behind truck-stepio's `in`, vtkio, proptest; 85 packages that would
//! need the network), so each copy gets a manifest with exactly the dependencies the kernel
//! resolves for it (its entry in `Cargo.lock`) and the features the kernel enables for it
//! (`cargo tree -e normal,dev`, so the test hooks are linted too): the lint covers exactly the
//! code the kernel compiles, and the test checks that nothing outside the repository's lockfile
//! was resolved. The crates' own `deny(clippy::all)` would fail on lints a newer Clippy adds, so
//! every lint is capped to a warning and the `disallowed_methods` diagnostics are read from the
//! output: any one of them fails this test.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use serde_json::Value;

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

const PROBE: &str = "pmath-gate-probe";

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

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// `cargo <args>` in `dir`, offline; its stdout.
fn cargo_output(dir: &Path, args: &[&str]) -> String {
    let mut c = cargo();
    c.current_dir(dir).args(args).arg("--offline");
    eprintln!("$ {c:?}");
    let out = c.output().unwrap();
    assert!(out.status.success(), "cargo {} failed: {}\n{}", args.join(" "), out.status, String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// A lockfile: every package as (name, version, source) and the names of its dependencies.
struct Lock {
    packages: BTreeSet<(String, String, String)>,
    dependencies: HashMap<String, Vec<String>>,
}

impl Lock {
    fn parse(text: &str) -> Lock {
        let mut lock = Lock { packages: BTreeSet::new(), dependencies: HashMap::new() };
        let unquote = |s: &str| s.trim().trim_matches('"').to_string();
        let (mut name, mut version, mut source) = (String::new(), String::new(), String::new());
        let mut deps: Vec<String> = Vec::new();
        let mut in_list = false;
        let mut flush = |name: &mut String, version: &mut String, source: &mut String, deps: &mut Vec<String>| {
            if !name.is_empty() {
                lock.packages.insert((name.clone(), version.clone(), source.clone()));
                lock.dependencies.insert(std::mem::take(name), std::mem::take(deps));
                version.clear();
                source.clear();
            }
        };
        for line in text.lines() {
            if in_list {
                if line.trim_start().starts_with(']') {
                    in_list = false;
                    continue;
                }
                // ` "name",` or ` "name 1.2.3",` or ` "name 1.2.3 (source)",`
                let entry = unquote(line.trim_end_matches(','));
                if !entry.is_empty() {
                    deps.push(entry.split(' ').next().unwrap_or("").to_string());
                }
            } else if line.starts_with("[[package]]") {
                flush(&mut name, &mut version, &mut source, &mut deps);
            } else if let Some(v) = line.strip_prefix("name = ") {
                name = unquote(v);
            } else if let Some(v) = line.strip_prefix("version = ") {
                version = unquote(v);
            } else if let Some(v) = line.strip_prefix("source = ") {
                source = unquote(v);
            } else if line.starts_with("dependencies = [") {
                in_list = true;
            }
        }
        flush(&mut name, &mut version, &mut source, &mut deps);
        lock
    }
}

/// The repository's lockfile.
fn repo_lock() -> Lock {
    Lock::parse(&std::fs::read_to_string(root().join("Cargo.lock")).unwrap())
}

/// The features the kernel enables on each package of its dependency graph (dev-dependencies
/// included, so the vendored test hooks are linted too), by package name.
fn kernel_features() -> HashMap<String, Vec<String>> {
    let text = cargo_output(&root(), &["tree", "-p", "polycore", "-e", "normal,dev", "--prefix", "none", "-f", "{p}|{f}", "--locked"]);
    let mut out = HashMap::new();
    for line in text.lines() {
        let Some((package, features)) = line.split_once('|') else { continue };
        let name = package.split(' ').next().unwrap_or("").to_string();
        let features: Vec<String> = features.trim_end_matches(" (*)").split(',').filter(|f| !f.is_empty()).map(str::to_string).collect();
        out.entry(name).or_insert(features);
    }
    out
}

/// The member manifest for the copy of `pkg` (its `cargo metadata --no-deps` entry): the
/// dependencies the kernel resolves for it (`resolved`, package names from the lockfile) and the
/// features the kernel enables on it (`enabled`), no dev-dependencies and no other optional
/// dependency, so the repository's lockfile covers it and the lint sees what the kernel compiles.
fn member_manifest(pkg: &Value, resolved: &[String], enabled: &[String]) -> String {
    let text = |v: &Value| v.as_str().unwrap_or("").to_string();
    let dir = Path::new(pkg["manifest_path"].as_str().unwrap()).parent().unwrap();
    let is_lib = |t: &&Value| t["kind"].as_array().is_some_and(|k| k.iter().any(|k| k == "lib" || k == "proc-macro"));
    let lib = pkg["targets"].as_array().unwrap().iter().find(is_lib).unwrap();
    let src: Vec<String> = Path::new(lib["src_path"].as_str().unwrap())
        .strip_prefix(dir)
        .unwrap()
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let mut out = format!(
        "[package]\nname = {:?}\nversion = {:?}\nedition = {:?}\nautobins = false\nautoexamples = false\nautotests = false\nautobenches = false\n\n[lib]\nname = {:?}\npath = {:?}\n",
        text(&pkg["name"]),
        text(&pkg["version"]),
        text(&pkg["edition"]),
        text(&lib["name"]),
        src.join("/")
    );
    if lib["kind"].as_array().unwrap().iter().any(|k| k == "proc-macro") {
        out += "proc-macro = true\n";
    }
    let mut sections: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut kept: BTreeSet<String> = BTreeSet::new();
    for d in pkg["dependencies"].as_array().unwrap() {
        let kind = d["kind"].as_str();
        if kind == Some("dev") {
            continue;
        }
        assert!(kind.is_none(), "{}: a {kind:?} dependency ({}) is not expected of a vendored crate", text(&pkg["name"]), text(&d["name"]));
        let name = text(&d["name"]);
        let optional = d["optional"].as_bool() == Some(true);
        if optional && !resolved.contains(&name) {
            continue;
        }
        let key = d["rename"].as_str().map_or_else(|| name.clone(), str::to_string);
        kept.insert(key.clone());
        let mut spec = vec![format!("version = {:?}", text(&d["req"]))];
        if let Some(path) = d["path"].as_str() {
            // A vendored dependency: its copy, beside this one.
            spec.push(format!("path = \"../{}\"", Path::new(path).file_name().unwrap().to_string_lossy()));
        }
        if key != name {
            spec.push(format!("package = {name:?}"));
        }
        if d["uses_default_features"].as_bool() == Some(false) {
            spec.push("default-features = false".to_string());
        }
        let features: Vec<String> = d["features"].as_array().map(|a| a.iter().map(|f| format!("{:?}", text(f))).collect()).unwrap_or_default();
        if !features.is_empty() {
            spec.push(format!("features = [{}]", features.join(", ")));
        }
        if optional {
            spec.push("optional = true".to_string());
        }
        let section = d["target"].as_str().map_or_else(|| "dependencies".to_string(), |t| format!("target.'{t}'.dependencies"));
        sections.entry(section).or_default().push(format!("{key} = {{ {} }}", spec.join(", ")));
    }
    for (section, lines) in &sections {
        out += &format!("\n[{section}]\n{}\n", lines.join("\n"));
    }
    // Only the enabled features, each keeping the entries that still refer to something.
    let enabled: BTreeSet<&str> = enabled.iter().map(String::as_str).collect();
    let refers = |item: &str| {
        if let Some(dep) = item.strip_prefix("dep:") {
            kept.contains(dep)
        } else if let Some((dep, _)) = item.split_once('/') {
            kept.contains(dep.trim_end_matches('?'))
        } else {
            enabled.contains(item) || kept.contains(item)
        }
    };
    let default: Vec<String> = enabled.iter().filter(|f| **f != "default").map(|f| format!("{f:?}")).collect();
    out += &format!("\n[features]\ndefault = [{}]\n", default.join(", "));
    for (feature, items) in pkg["features"].as_object().unwrap() {
        if feature == "default" || !enabled.contains(feature.as_str()) {
            continue;
        }
        let items: Vec<String> = items.as_array().unwrap().iter().map(&text).filter(|i| refers(i)).map(|i| format!("{i:?}")).collect();
        out += &format!("{feature:?} = [{}]\n", items.join(", "));
    }
    out
}

/// The temporary workspace: copies of the vendored crates and the probe as members, with the
/// repository's lockfile, made once.
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
        &format!("[package]\nname = {PROBE:?}\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\npath = \"lib.rs\"\n"),
    );
    let planted: Vec<&str> = PLANTED.iter().map(|(line, _)| *line).collect();
    write(&ws.join("probe").join("lib.rs"), &format!("{}\n", planted.join("\n")));
    // The copies' manifests, cut to what the kernel resolves and enables.
    let lock = repo_lock();
    let features = kernel_features();
    let metadata: Value = serde_json::from_str(&cargo_output(&ws, &["metadata", "--no-deps", "--format-version", "1"])).unwrap();
    for pkg in metadata["packages"].as_array().unwrap() {
        let name = pkg["name"].as_str().unwrap();
        if name == PROBE {
            continue;
        }
        let resolved = lock.dependencies.get(name).unwrap_or_else(|| panic!("{name} is not in the repository's Cargo.lock"));
        let enabled = features.get(name).unwrap_or_else(|| panic!("{name} is not in the kernel's dependency graph"));
        write(Path::new(pkg["manifest_path"].as_str().unwrap()), &member_manifest(pkg, resolved, enabled));
    }
    std::fs::copy(root.join("Cargo.lock"), ws.join("Cargo.lock")).unwrap();
    ws
});

fn clippy(args: &[&str]) -> (bool, String) {
    let ws = &*WORKSPACE;
    let mut c = cargo();
    c.current_dir(ws)
        .env("CLIPPY_CONF_DIR", root())
        .env("CARGO_TARGET_DIR", ws.join("target"))
        .env_remove("RUSTFLAGS")
        .args(["clippy", "--offline", "--manifest-path"])
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

/// The vendored crates and `pmath` itself call no platform transcendental, linted offline at the
/// versions the repository's lockfile pins.
#[test]
fn vendored_crates_call_no_platform_transcendental() {
    let (ok, text) = clippy(&["--workspace", "--exclude", PROBE]);
    let found = flagged(&text);
    assert!(ok && found.is_empty(), "clippy on the vendored crates: ok = {ok}, flagged {found:?}\n{text}");
    assert!(text.contains("Finished"), "{text}");
    for c in VENDOR {
        // A proc-macro crate is compiled, the others checked.
        let linted = text.contains(&format!("Checking polycore-{c} ")) || text.contains(&format!("Compiling polycore-{c} "));
        assert!(linted, "{c} was not linted:\n{text}");
    }
    // Everything the workspace resolved is in the repository's lockfile, at the same version.
    let repo = repo_lock();
    let ws = Lock::parse(&std::fs::read_to_string(WORKSPACE.join("Cargo.lock")).unwrap());
    let outside: Vec<_> = ws.packages.iter().filter(|p| p.0 != PROBE && !repo.packages.contains(p)).collect();
    assert!(outside.is_empty(), "the gate resolved packages outside the repository's Cargo.lock: {outside:?}");
}

/// A new platform call is caught: each planted call is named with its line in the probe.
#[test]
fn a_new_platform_call_fails_the_lint() {
    let (ok, text) = clippy(&["-p", PROBE]);
    assert!(ok, "the probe did not compile:\n{text}");
    let found = flagged(&text);
    for (i, (_, method)) in PLANTED.iter().enumerate() {
        let hit = found.iter().find(|(m, at)| m == method && at.contains(&format!("lib.rs:{}:", i + 1)));
        assert!(hit.is_some(), "{method} on probe line {} was not flagged; flagged {found:?}\n{text}", i + 1);
    }
    assert!(found.iter().all(|(_, at)| at.contains("probe")), "flagged outside the probe: {found:?}");
}
