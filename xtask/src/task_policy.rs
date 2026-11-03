use std::path::PathBuf;

use craft_xtask::{Cargo, Configs, layers};

/// Library crates by directory under `crates/` (package `materialcraft-<dir>`) and their layer.
pub const TABLE: &[(&str, &str)] = &[
    ("core", "L0"),
    ("image", "L0"),
    ("geom", "L0"),
    ("nodes", "L1"),
    ("eval", "L2"),
    ("formats", "L2"),
    ("engine", "L3"),
    ("render", "L3"),
    ("automation", "L4"),
    ("ui-egui", "L5"),
];

pub fn package(dir: &str) -> String {
    format!("materialcraft-{dir}")
}

pub fn describe(config: &layers::Config, name: &str) -> String {
    if config.exempt.iter().any(|item| item == name) {
        return "exempt".into();
    }
    config.layers.iter().find(|(_, names)| names.iter().any(|item| item == name)).map_or_else(|| "UNKNOWN".into(), |(layer, _)| layer.clone())
}

pub fn configs(root: PathBuf) -> Configs {
    let cargo = Cargo { root: root.clone(), ..Cargo::default() };
    let mut configs = Configs::for_root(&root);
    configs.version.cargo = cargo;
    configs.version.temporary_dir = ".".into();
    configs.ico.root = root.clone();
    configs.layers = layers::Config {
        root,
        layers: ["L0", "L1", "L2", "L3", "L4", "L5"]
            .into_iter()
            .map(|level| (level.into(), TABLE.iter().filter(|(_, l)| *l == level).map(|(dir, _)| package(dir)).collect()))
            .collect(),
        exempt: ["materialcraft", "materialcraft-cli", "materialcraft-web", "xtask"].into_iter().map(str::to_owned).collect(),
        external_rules: vec![
            layers::ExternalRule {
                dependencies: ["egui", "egui_*", "egui-*", "eframe", "winit", "rfd"].into_iter().map(str::to_owned).collect(),
                allowed_from: vec!["materialcraft-ui-egui".into()],
                allowed_dev_from: Vec::new(),
            },
            layers::ExternalRule {
                dependencies: ["wgpu", "wgpu-*"].into_iter().map(str::to_owned).collect(),
                allowed_from: ["materialcraft-render", "materialcraft-ui-egui"].into_iter().map(str::to_owned).collect(),
                allowed_dev_from: Vec::new(),
            },
        ],
        ..layers::Config::default()
    };
    configs
}
