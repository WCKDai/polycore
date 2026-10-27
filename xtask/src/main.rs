#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod kernel_cases;
mod kernel_ladder;
mod task;
mod test;

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

const TRUCK_PACKAGES: [&str; 10] = [
    "polycore-truck-base",
    "polycore-truck-derivers",
    "polycore-truck-geometry",
    "polycore-truck-geotrait",
    "polycore-truck-meshalgo",
    "polycore-truck-modeling",
    "polycore-truck-polymesh",
    "polycore-truck-shapeops",
    "polycore-truck-stepio",
    "polycore-truck-topology",
];

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("ci");
    let rest: Vec<&str> = args.get(1..).unwrap_or_default().iter().map(String::as_str).collect();
    match command {
        "ci" => ci(),
        "fmt" => run_cargo(&["fmt", "-p", "solidcraft-geom", "-p", "solidcraft-kernel", "-p", "xtask", "--", "--check"]),
        "check" => run_cargo(&["check", "--workspace", "--all-targets", "--locked"]),
        "build" => run_cargo(&["build", "--workspace", "--profile", "ci", "--locked"]),
        "clippy" => run_cargo(&["clippy", "--workspace", "--all-targets", "--profile", "ci", "--locked", "--", "-D", "warnings"]),
        "test" => test(),
        "property-test" => run_cargo(&["test", "-p", "solidcraft-kernel", "--test", "boolean_props", "--profile", "ci", "--locked"]),
        "wasm" => wasm(),
        "verify-deps" => verify_deps(),
        "task-check" => task::check(rest.first().copied().ok_or("task-check needs a JSON task path")?),
        "kernel-cases" => kernel_cases::run(&rest),
        _ => Err(format!(
            "unknown task {command:?}; expected ci, fmt, check, build, clippy, test, property-test, wasm, verify-deps, task-check or kernel-cases"
        )),
    }
}

fn ci() -> Result<(), String> {
    run_cargo(&["fmt", "-p", "solidcraft-geom", "-p", "solidcraft-kernel", "-p", "xtask", "--", "--check"])?;
    run_cargo(&["clippy", "--workspace", "--all-targets", "--profile", "ci", "--locked", "--", "-D", "warnings"])?;
    test()?;
    wasm()?;
    verify_deps()?;
    task::check("agent/tasks/example.json")
}

fn test() -> Result<(), String> {
    test::run(&["--locked"], &[], vec![test::doc_tests(&["--locked"], &[])])
}

fn wasm() -> Result<(), String> {
    run_cargo(&["check", "--target", "wasm32-unknown-unknown", "--profile", "ci", "--locked", "-p", "solidcraft-geom", "-p", "solidcraft-kernel"])
}

fn verify_deps() -> Result<(), String> {
    let output = cargo().args(["metadata", "--format-version", "1", "--locked"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata: {}\n{}", output.status, String::from_utf8_lossy(&output.stderr)));
    }
    let metadata: Value = serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata JSON: {e}"))?;
    let count = validate_metadata(&metadata)?;
    println!("dependency boundary: all {count} renamed Truck-family packages are local; no registry Truck packages");
    Ok(())
}

fn validate_metadata(metadata: &Value) -> Result<usize, String> {
    let packages = metadata["packages"].as_array().ok_or("cargo metadata: packages is not an array")?;
    let mut local = Vec::new();
    for package in packages {
        let name = package["name"].as_str().ok_or("cargo metadata: a package has no name")?;
        if name.starts_with("truck-") || name == "matext4cgmath" {
            return Err(format!("registry-named package escaped the renamed closure: {name}"));
        }
        if TRUCK_PACKAGES.contains(&name) || name == "polycore-matext4cgmath" {
            if !package["source"].is_null() {
                return Err(format!("renamed Truck-family package is not a path dependency: {name}"));
            }
            local.push(name.to_string());
        }
    }
    local.sort();
    local.dedup();
    let expected = [TRUCK_PACKAGES.as_slice(), &["polycore-matext4cgmath"]].concat();
    let missing: Vec<&str> = expected.into_iter().filter(|name| !local.iter().any(|found| found == name)).collect();
    if !missing.is_empty() {
        return Err(format!("renamed Truck-family packages missing: {}", missing.join(", ")));
    }
    Ok(local.len())
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn cargo() -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(root());
    command
}

fn run_cargo(args: &[&str]) -> Result<(), String> {
    eprintln!("$ cargo {}", args.join(" "));
    let status = cargo().args(args).status().map_err(|e| format!("cargo {}: {e}", args.join(" ")))?;
    if status.success() { Ok(()) } else { Err(format!("cargo {} exited with {status}", args.join(" "))) }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn metadata(names: &[&str]) -> Value {
        json!({"packages": names.iter().map(|name| json!({"name": name, "source": null})).collect::<Vec<_>>()})
    }

    #[test]
    fn dependency_boundary_accepts_the_complete_local_closure() {
        let mut names = TRUCK_PACKAGES.to_vec();
        names.push("polycore-matext4cgmath");
        names.push("serde");
        assert_eq!(validate_metadata(&metadata(&names)), Ok(11));
    }

    #[test]
    fn dependency_boundary_rejects_registry_truck_and_missing_packages() {
        let mut names = TRUCK_PACKAGES.to_vec();
        names.push("polycore-matext4cgmath");
        names.push("truck-base");
        assert!(validate_metadata(&metadata(&names)).is_err());
        assert!(validate_metadata(&metadata(&["serde"])).is_err());
    }

    #[test]
    fn dependency_boundary_rejects_a_renamed_registry_package() {
        let mut names = TRUCK_PACKAGES.to_vec();
        names.push("polycore-matext4cgmath");
        let mut value = metadata(&names);
        value["packages"][0]["source"] = json!("registry+https://github.com/rust-lang/crates.io-index");
        assert!(validate_metadata(&value).is_err());
    }
}
