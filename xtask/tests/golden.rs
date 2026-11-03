#[path = "../src/task_policy.rs"]
mod task_policy;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn task(args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask")).current_dir(root()).args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    output
}

fn stdout(args: &[&str]) -> String {
    String::from_utf8(task(args).stdout).unwrap()
}

fn temporary(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("materialcraft-xtask-70-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.trim().as_bytes().as_chunks::<2>().0.iter().map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()).collect()
}

fn fake_png(size: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend_from_slice(&size.to_be_bytes());
    v.extend_from_slice(&size.to_be_bytes());
    v.extend_from_slice(&[8, 6, 0, 0, 0, 1, 2, 3, 4]);
    v
}

#[test]
fn version_matches_original_and_rewrites_temp_copy() {
    assert_eq!(stdout(&["version"]), include_str!("golden/version.stdout"));
    let temp = temporary("version");
    let manifest = temp.join("Cargo.toml");
    std::fs::write(&manifest, include_str!("golden/version.input.toml")).unwrap();
    let mut config = task_policy::configs(temp.clone()).version;
    config.refresh_lockfile = false;
    config.manifest_paths = vec!["Cargo.toml".into()];
    assert_eq!(craft_xtask::version::read(&std::fs::read_to_string(&manifest).unwrap()).unwrap(), "0.1.0");
    let plan = craft_xtask::version::compute(&config, &craft_xtask::version::Args::Set("0.2.0-rc.1".into())).unwrap();
    assert_eq!(craft_xtask::version::render(&plan), include_str!("golden/version-set.stdout"));
    assert_eq!(std::fs::read_to_string(&manifest).unwrap(), include_str!("golden/version.input.toml"));
    assert_eq!(plan.edits[0].2, include_str!("golden/version.updated.toml"));
    assert_eq!(
        craft_xtask::version::rewrite(&config, include_str!("golden/version.input.toml"), "0.2.0-rc.1").unwrap(),
        include_str!("golden/version.updated.toml")
    );
    craft_xtask::version::apply(&plan).unwrap();
    assert_eq!(std::fs::read_to_string(&manifest).unwrap(), include_str!("golden/version.updated.toml"));
    assert!(!temp.join("target").exists());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn version_preserves_crlf_and_validation_rules() {
    let crlf = include_str!("golden/version.input.toml").replace('\n', "\r\n");
    assert_eq!(craft_xtask::version::replace(&crlf, "2.0.0").unwrap(), crlf.replace("\"0.1.0\"", "\"2.0.0\""));
    assert!(craft_xtask::version::read("[package]\nversion = \"1.0.0\"\n").is_err());
    for ok in ["0.1.0", "1.20.300", "1.0.0-rc.1", "1.0.0-alpha", "1.0.0-x-y.2"] {
        assert!(craft_xtask::version::validate(ok).is_ok(), "{ok}");
    }
    for bad in ["1.0", "1.0.0.0", "v1.0.0", "01.0.0", "1.0.0-", "1.0.0-a..b", "1.0.0+meta", "1.a.0", ""] {
        assert!(craft_xtask::version::validate(bad).is_err(), "{bad}");
    }
    let text = std::fs::read_to_string(root().join("Cargo.toml")).unwrap();
    craft_xtask::version::validate(&craft_xtask::version::read(&text).unwrap()).unwrap();
}

#[test]
fn layers_matches_original_report_and_policy() {
    assert_eq!(stdout(&["layers"]), include_str!("golden/layers.stdout"));
    let config = task_policy::configs(root()).layers;
    assert_eq!(task_policy::package("geom"), "materialcraft-geom");
    assert_eq!(task_policy::describe(&config, "materialcraft-geom"), "L0");
    assert_eq!(task_policy::describe(&config, "materialcraft"), "exempt");
    assert_eq!(task_policy::describe(&config, "materialcraft-mystery"), "UNKNOWN");
    use craft_xtask::layers::{Crate, Dep, DepKind, DepKind::*};
    let c = |name: &str, deps: &[(&str, DepKind, bool)]| Crate {
        name: name.into(),
        deps: deps.iter().map(|(n, k, w)| Dep { name: (*n).into(), kind: *k, workspace: *w }).collect(),
    };
    let g = [
        c("materialcraft-ui-egui", &[("materialcraft-engine", Normal, true), ("egui", Normal, false), ("wgpu", Normal, false)]),
        c("materialcraft-engine", &[("materialcraft-nodes", Normal, true), ("materialcraft-eval", Dev, true)]),
        c("materialcraft-nodes", &[("materialcraft-core", Normal, true), ("materialcraft-image", Normal, true)]),
        c("materialcraft-core", &[]),
    ];
    assert!(craft_xtask::layers::check(&config, &g).unwrap().is_empty());
    assert_eq!(craft_xtask::layers::check(&config, &[c("materialcraft-core", &[("materialcraft-geom", Normal, true)])]).unwrap().len(), 1);
    assert_eq!(craft_xtask::layers::check(&config, &[c("materialcraft-nodes", &[("materialcraft-eval", Dev, true)])]).unwrap().len(), 1);
    assert_eq!(craft_xtask::layers::check(&config, &[c("materialcraft-eval", &[("materialcraft-formats", Build, true)])]).unwrap().len(), 1);
    assert_eq!(craft_xtask::layers::check(&config, &[c("materialcraft-engine", &[("materialcraft", Normal, true)])]).unwrap().len(), 1);
    for dep in ["egui", "egui_extras", "egui-wgpu", "eframe", "winit", "rfd"] {
        assert_eq!(craft_xtask::layers::check(&config, &[c("materialcraft-automation", &[(dep, Normal, false)])]).unwrap().len(), 1, "{dep}");
    }
    assert!(craft_xtask::layers::check(&config, &[c("materialcraft-automation", &[("eguilike", Normal, false)])]).unwrap().is_empty());
    assert_eq!(craft_xtask::layers::check(&config, &[c("materialcraft-engine", &[("wgpu", Normal, false)])]).unwrap().len(), 1);
    assert_eq!(craft_xtask::layers::check(&config, &[c("materialcraft-nodes", &[("wgpu-types", Normal, false)])]).unwrap().len(), 1);
    assert!(craft_xtask::layers::check(&config, &[c("materialcraft-render", &[("wgpu", Normal, false)])]).unwrap().is_empty());
    assert_eq!(craft_xtask::layers::check(&config, &[c("materialcraft-mystery", &[])]).unwrap().len(), 1);
    assert!(craft_xtask::layers::check(&config, &[c("xtask", &[("materialcraft-ui-egui", Normal, true)])]).unwrap().is_empty());
    assert!(craft_xtask::layers::check(&config, &[c("materialcraft-core", &[("materialcraft-core", Dev, true)])]).unwrap().is_empty());
}

#[test]
fn ico_matches_original_bytes() {
    let temp = temporary("ico");
    let out = temp.join("test.ico");
    let pngs = ["assets/app-icon/hicolor/16x16/apps/ai.storyteller.materialcraft.png", "assets/app-icon/hicolor/32x32/apps/ai.storyteller.materialcraft.png"];
    let output = stdout(&["ico", out.to_str().unwrap(), pngs[0], pngs[1]]);
    assert_eq!(output.replace(out.to_str().unwrap(), "/tmp/materialcraft70.ico"), include_str!("golden/ico.stdout"));
    let expected = hex_bytes(include_str!("golden/ico.hex"));
    assert_eq!(std::fs::read(&out).unwrap(), expected);
    let report = craft_xtask::ico::compute(
        &task_policy::configs(root()).ico,
        &craft_xtask::ico::Args { output: Some("/tmp/materialcraft70.ico".into()), inputs: pngs.into_iter().map(PathBuf::from).collect() },
    )
    .unwrap();
    assert_eq!(craft_xtask::ico::render(&report), include_str!("golden/ico.stdout"));
    assert_eq!(report.bytes, expected);
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn icns_matches_original_bytes_and_slots() {
    let temp = temporary("icns");
    let out = temp.join("test.icns");
    let pngs = ["assets/app-icon/hicolor/16x16/apps/ai.storyteller.materialcraft.png", "assets/app-icon/hicolor/32x32/apps/ai.storyteller.materialcraft.png"];
    let output = stdout(&["icns", out.to_str().unwrap(), pngs[0], pngs[1]]);
    assert_eq!(output.replace(out.to_str().unwrap(), "/tmp/materialcraft70.icns"), include_str!("golden/icns.stdout"));
    let expected = hex_bytes(include_str!("golden/icns.hex"));
    assert_eq!(std::fs::read(&out).unwrap(), expected);
    let (a, b) = (fake_png(16), fake_png(32));
    let icns = craft_xtask::ico::pack_icns(&[a.clone(), b.clone()]).unwrap();
    assert_eq!(&icns[..4], b"icns");
    let types: Vec<&[u8]> = vec![&icns[8..12], &icns[8 + 8 + a.len()..12 + 8 + a.len()], &icns[8 + 16 + a.len() + b.len()..12 + 16 + a.len() + b.len()]];
    assert_eq!(types, [b"icp4" as &[u8], b"icp5", b"ic11"]);
    assert!(craft_xtask::ico::pack_ico(&[]).is_err());
    assert!(craft_xtask::ico::pack_ico(&[fake_png(512)]).is_err());
    assert!(craft_xtask::ico::pack_icns(&[fake_png(48)]).is_err());
    let dir = root().join("assets/app-icon");
    let ico = std::fs::read(dir.join("materialcraft.ico")).unwrap();
    assert_eq!(&ico[..4], &[0, 0, 1, 0]);
    let icns = std::fs::read(dir.join("materialcraft.icns")).unwrap();
    assert_eq!(&icns[..4], b"icns");
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn help_includes_shared_task_syntax() {
    let text = stdout(&["--help"]);
    assert!(text.starts_with(include_str!("golden/help.stdout")));
    for expected in
        ["layers", "wasm", "ci", "version", "ico", "icns", craft_xtask::version::cli_help(), craft_xtask::ico::cli_help(), craft_xtask::layers::cli_help()]
    {
        assert!(text.contains(expected), "missing {expected}");
    }
}
