//! Workspace tooling: `cargo xtask <command>`.
//!
//! Shared tasks delegate to `craft-xtask`; MaterialCraft-specific commands (`wasm`, `ci`) and
//! the `layers` report formatting stay local. `cargo` is invoked through `std::process::Command`.

mod task_policy;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  layers          enforce the crate dependency layering (xtask/src/layers.rs)
  wasm            cargo check --target wasm32-unknown-unknown for the wasm crates and the web app
  ci              fmt --check, clippy -D warnings, test, layers, wasm (stops at the first failure)
  version [set X.Y.Z[-pre]]
                  print the workspace version, or set it (Cargo.toml + Cargo.lock)
  ico <out.ico> <in.png>...
                  pack square PNGs (<= 256 px) into a Windows .ico (see packaging/icons.sh)
  icns <out.icns> <in.png>...
                  pack square PNGs (16-1024 px) into a macOS .icns (see packaging/icons.sh)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let shared_args: Vec<String> = args.iter().skip(1).cloned().collect();
    let configs = task_policy::configs(root());
    let result = match args.first().map(String::as_str) {
        Some("layers") => cmd_layers(&configs.layers, &shared_args),
        Some("wasm") => cmd_wasm(),
        Some("ci") => cmd_ci(),
        Some("version") => craft_xtask::version::run(&configs.version, &shared_args),
        Some("ico") => craft_xtask::ico::run(&configs.ico, &shared_args),
        Some("icns") => craft_xtask::ico::run_icns(&configs.ico, &shared_args),
        Some("-h" | "--help" | "help") | None => {
            print!("{}", usage());
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{}", usage())),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> String {
    format!("{USAGE}\nShared task syntax:\n  {}\n  {}\n  {}\n", craft_xtask::version::cli_help(), craft_xtask::ico::cli_help(), craft_xtask::layers::cli_help(),)
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    let xtask = Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask.parent().unwrap_or(xtask).to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

pub fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

/// `cargo metadata --no-deps`: the workspace members and their declared dependencies.
fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn cmd_layers(config: &craft_xtask::layers::Config, args: &[String]) -> Result<(), String> {
    let report = craft_xtask::layers::compute(config, &craft_xtask::layers::Args::parse(args)?)?;
    println!("Dependency layering (xtask/src/layers.rs)\n");
    for (dir, _) in task_policy::TABLE {
        let present = report.crates.iter().any(|c| c.name == task_policy::package(dir));
        if !present {
            if root().join("crates").join(dir).join("Cargo.toml").is_file() {
                println!("note: crates/{dir} has a Cargo.toml but is not a workspace member; not checked");
            } else {
                println!("skip: crates/{dir} (no Cargo.toml yet)");
            }
        }
    }
    println!("\n{:<28} {:<8} workspace deps", "crate", "layer");
    for c in &report.crates {
        let ws: Vec<&str> = c.deps.iter().filter(|d| d.workspace).map(|d| d.name.as_str()).collect();
        println!("{:<28} {:<8} {}", c.name, task_policy::describe(config, &c.name), ws.join(", "));
    }
    println!();
    if report.ok {
        println!("OK: {} crates, no layering violations.", report.crates.len());
        Ok(())
    } else {
        println!("{} violation(s):", report.violations.len());
        for v in &report.violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", report.violations.len()))
    }
}

/// Packages that must build for wasm32-unknown-unknown, in dependency order.
const WASM_PACKAGES: &[&str] = &[
    "materialcraft-core",
    "materialcraft-image",
    "materialcraft-geom",
    "materialcraft-nodes",
    "materialcraft-eval",
    "materialcraft-formats",
    "materialcraft-engine",
    "materialcraft-render",
    "materialcraft-ui-egui",
    "materialcraft-web",
];

fn cmd_wasm() -> Result<(), String> {
    let members = craft_xtask::layers::from_metadata(&metadata()?)?;
    let mut results = Vec::new();
    for pkg in WASM_PACKAGES {
        if !members.iter().any(|c| c.name == *pkg) {
            println!("skip: {pkg} (not a workspace member yet)");
            continue;
        }
        // One package per invocation, so no crate passes only thanks to features another enables.
        let mut c = cargo();
        c.args(["check", "--target", "wasm32-unknown-unknown", "-p", pkg]);
        let ok = run(c, &format!("cargo check --target wasm32-unknown-unknown -p {pkg}")).is_ok();
        results.push((*pkg, ok));
    }
    println!("\nwasm32-unknown-unknown check:");
    for (p, ok) in &results {
        println!("  {:<6} {p}", if *ok { "ok" } else { "FAIL" });
    }
    let failed = results.iter().filter(|r| !r.1).count();
    if failed == 0 { Ok(()) } else { Err(format!("{failed} package(s) failed the wasm check")) }
}

fn cmd_ci() -> Result<(), String> {
    let cargo_step = |args: &'static [&'static str]| {
        move || {
            let mut c = cargo();
            c.args(args);
            run(c, &format!("cargo {}", args.join(" ")))
        }
    };
    type Step = (&'static str, Box<dyn Fn() -> Result<(), String>>);
    let steps: Vec<Step> = vec![
        ("fmt", Box::new(cargo_step(&["fmt", "--all", "--", "--check"]))),
        ("clippy", Box::new(cargo_step(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]))),
        ("test", Box::new(cargo_step(&["test", "--workspace"]))),
        ("layers", Box::new(|| cmd_layers(&task_policy::configs(root()).layers, &[]))),
        ("wasm", Box::new(cmd_wasm)),
    ];
    for (i, (name, step)) in steps.iter().enumerate() {
        eprintln!("\n=== ci: {name} ===");
        if let Err(e) = step() {
            println!("\nCI summary:");
            for (done, _) in &steps[..i] {
                println!("  ok    {done}");
            }
            println!("  FAIL  {name}: {e}");
            for (skipped, _) in steps.iter().skip(i + 1) {
                println!("  skip  {skipped}");
            }
            return Err(format!("ci failed at `{name}`"));
        }
    }
    println!("\nCI summary: all {} steps passed", steps.len());
    Ok(())
}
