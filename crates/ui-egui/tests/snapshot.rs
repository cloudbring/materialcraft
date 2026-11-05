//! Offscreen renders of the real UI (no window). Each scene is compared with a committed golden in
//! `tests/goldens/` (see the README there); a difference beyond `Tolerance::RENDER` fails the test
//! and writes the render and a x8 diff PNG to `target/golden-failures/`. Bless new or intended
//! changes with:
//!
//! ```sh
//! MC_BLESS=1 cargo test -p materialcraft-ui-egui --test snapshot
//! ```
//!
//! To also write the full-size 1600x960 pictures (to look at, or for the README screenshots):
//!
//! ```sh
//! MC_SNAPSHOT_DIR=<dir> cargo test -p materialcraft-ui-egui --release --test snapshot
//! ```
//!
//! The tests need a GPU adapter and skip (with a message) without one, as CI has none.

#[path = "common/golden.rs"]
mod golden;

use std::time::Duration;

use egui::vec2;
use egui_kittest::{Harness, kittest::Queryable};
use materialcraft_ui_egui::{MaterialCraftApp, UiState};
use serde_json::json;

fn harness(app: MaterialCraftApp, w: f32, h: f32) -> Harness<'static, MaterialCraftApp> {
    Harness::builder().with_size(vec2(w, h)).with_pixels_per_point(1.0).wgpu().build_ui_state(
        |ui, app: &mut MaterialCraftApp| {
            app.step(&ui.ctx().clone());
            app.draw(ui);
        },
        app,
    )
}

/// Run frames until background work (cooks, previews) settles.
fn settle(h: &mut Harness<'static, MaterialCraftApp>, frames: usize) {
    for _ in 0..frames {
        h.step();
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Whether a GPU (or software) adapter exists; the snapshot tests skip without one.
fn gpu_available() -> bool {
    let instance = wgpu::Instance::default();
    let found = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).is_ok();
    if !found {
        eprintln!("skipping: no GPU adapter");
    }
    found
}

/// Where to also write the full-size pictures, if `MC_SNAPSHOT_DIR` is set.
fn snapshot_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("MC_SNAPSHOT_DIR").map(std::path::PathBuf::from)?;
    std::fs::create_dir_all(&dir).unwrap();
    Some(dir)
}

/// The picture with the bottom 30 points (the 22-point status bar and its frame) blanked: it shows wall-clock UI frame time and
/// cook progress, which differ run to run, so goldens and the settling check leave it out. The
/// full-size pictures written to `MC_SNAPSHOT_DIR` keep it.
fn without_status_bar(h: &Harness<'static, MaterialCraftApp>, img: &image::RgbaImage) -> image::RgbaImage {
    let strip = (30.0 * h.ctx.pixels_per_point()).ceil() as u32;
    let mut out = img.clone();
    for y in img.height().saturating_sub(strip)..img.height() {
        for x in 0..img.width() {
            out.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
        }
    }
    out
}

/// Step with pauses until the picture is unchanged for three frames (cooks, previews, GPU uploads
/// and read-backs land over several frames), then compare it with the golden `<name>.png`.
fn save(h: &mut Harness<'static, MaterialCraftApp>, dir: Option<&std::path::Path>, name: &str) {
    let (mut img, mut masked, mut same) = (None, None::<image::RgbaImage>, 0);
    for _ in 0..400 {
        // The 3D view prints its last render time ("Render time: 20 ms"): zero it before each frame
        // so a settled picture shows a fixed value (a frame that arrives mid-step resets the count).
        h.state_mut().view3d.render_ms = 0.0;
        h.step();
        std::thread::sleep(Duration::from_millis(15));
        let now = h.render().expect("render");
        let now_masked = without_status_bar(h, &now);
        same = if masked.as_ref() == Some(&now_masked) { same + 1 } else { 0 };
        (img, masked) = (Some(now), Some(now_masked));
        if same >= 3 {
            break;
        }
    }
    let (img, masked) = (img.unwrap(), masked.unwrap());
    assert!(same >= 3, "{name}: the picture never settled");
    if let Some(dir) = dir {
        img.save(dir.join(format!("{name}.png"))).expect("save");
        println!("wrote {}", dir.join(format!("{name}.png")).display());
    }
    // GPU output, so Tolerance::RENDER (the pictures are also box-downscaled to fit the size cap).
    golden::assert_golden(
        env!("CARGO_MANIFEST_DIR"),
        name,
        img.width(),
        img.height(),
        masked.as_raw(),
        golden::Tolerance::RENDER,
        &format!("MaterialCraft snapshot: {name}"),
    );
}

/// The startup graph stays empty through real UI frames; templates are still available on demand.
#[test]
fn empty_startup_can_create_a_template() {
    if !gpu_available() {
        return;
    }
    let dir = snapshot_dir();
    let app = MaterialCraftApp::headless(materialcraft_engine::new_session(), UiState::default());
    let mut h = harness(app, 1600.0, 960.0);
    settle(&mut h, 40);
    assert_eq!(h.state().session.pkg.graphs.len(), 1);
    assert!(h.state().session.graph().unwrap().nodes.is_empty());
    assert!(h.state().graph.rects.is_empty());
    save(&mut h, dir.as_deref(), "empty-startup");
    h.get_by_label("New graph...").click();
    settle(&mut h, 4);
    h.get_by_label("Brick wall").click();
    settle(&mut h, 4);
    h.get_by_label("Create").click();
    settle(&mut h, 40);
    assert_eq!(h.state().session.pkg.graphs.len(), 2);
    assert!(!h.state().session.graph().unwrap().nodes.is_empty());
    assert!(!h.state().graph.rects.is_empty());
    save(&mut h, dir.as_deref(), "chosen-template");
}

#[test]
fn snapshots() {
    if !gpu_available() {
        return;
    }
    let dir = snapshot_dir();
    let app = MaterialCraftApp::headless(materialcraft_engine::sample_session(), UiState::default());
    let mut h = harness(app, 1600.0, 960.0);
    settle(&mut h, 40);
    // Select a node so the properties panel shows parameters.
    let id = h.state().session.graph().unwrap().nodes[0].id;
    h.state_mut().run("graph.select", json!({"ids": [id.0]}));
    h.state_mut().run("view.2d", json!({"node": 16, "histogram": true}));
    settle(&mut h, 30);
    save(&mut h, dir.as_deref(), "workspace");
    for (tpl, name) in
        [("wood", "wood"), ("rusty_metal", "rusty-metal"), ("stone_tiles", "stone-tiles"), ("fabric", "fabric"), ("ground", "ground"), ("lava", "lava")]
    {
        h.state_mut().run("file.new", json!({"template": tpl, "size": 1024}));
        h.state_mut().ui.frame_all = true;
        settle(&mut h, 40);
        save(&mut h, dir.as_deref(), name);
    }
    h.state_mut().ui.windows.new_graph = true;
    settle(&mut h, 40);
    save(&mut h, dir.as_deref(), "new-graph");
    h.state_mut().ui.windows.new_graph = false;

    // A cut-out material in the 3D view (an opacity output), a reroute dot pulled out of a wire
    // and another wire selected; then the alpha channel view.
    h.state_mut().run("file.new", json!({"template": "wood", "size": 512}));
    let holes = h.state_mut().run("graph.node.add", json!({"def": "grid_lines", "at": [0, -260]})).unwrap()["id"].as_u64().unwrap();
    let out = h.state_mut().run("graph.node.add", json!({"def": "output", "at": [420, -260], "connectFrom": {"node": holes}, "params": {"usage": 7}})).unwrap()
        ["id"]
        .as_u64()
        .unwrap();
    h.state_mut().run("graph.wire.insertDot", json!({"to": out, "toPort": "input", "at": [210, -330]}));
    let wire = h.state().session.graph().unwrap().connections.iter().find(|c| c.to.0 != out).map(|c| (c.to, c.to_port.clone()));
    h.state_mut().graph.selected_wires = wire.into_iter().collect();
    h.state_mut().run("view.3d", json!({"scene": "sphere"}));
    h.state_mut().ui.frame_all = true;
    settle(&mut h, 60);
    save(&mut h, dir.as_deref(), "alpha-transparency");
    h.state_mut().run("view.3d", json!({"alphaView": true}));
    settle(&mut h, 40);
    save(&mut h, dir.as_deref(), "alpha-channel");
    h.state_mut().run("view.3d", json!({"alphaView": false, "scene": "rounded_cube"}));

    // A pixel processor drawing rings, with its function graph open.
    h.state_mut().run("file.new", json!({"template": "empty", "name": "Rings"}));
    let f = json!({"t": "function", "v": {"nodes": [
        {"id": 1, "op": "get_pos", "pos": [0, 0]},
        {"id": 2, "op": "const_float2", "value": {"t": "float2", "v": [0.5, 0.5]}, "pos": [0, 110]},
        {"id": 3, "op": "sub", "inputs": [1, 2], "pos": [170, 40]},
        {"id": 4, "op": "length", "inputs": [3], "pos": [340, 40]},
        {"id": 5, "op": "const_float", "value": {"t": "float", "v": 60.0}, "pos": [340, 130]},
        {"id": 6, "op": "mul", "inputs": [4, 5], "pos": [510, 60]},
        {"id": 7, "op": "sin", "inputs": [6], "pos": [680, 60]}
    ], "output": 7}});
    let id = h.state_mut().run("graph.node.add", json!({"def": "pixel_processor", "at": [0, 0], "params": {"output": 1}})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().run("graph.node.param", json!({"id": id, "param": "function", "value": f}));
    let out =
        h.state_mut().run("graph.node.add", json!({"def": "output", "connectFrom": {"node": id}, "params": {"usage": 4}})).unwrap()["id"].as_u64().unwrap();
    let _ = out;
    h.state_mut().run("graph.select", json!({"ids": [id]}));
    h.state_mut().run("view.2d", json!({"node": id}));
    h.state_mut().fn_editor = Some(materialcraft_ui_egui::function_editor::FnEditor::new(materialcraft_engine::core::NodeId(id), "function"));
    h.state_mut().ui.frame_all = true;
    settle(&mut h, 40);
    save(&mut h, dir.as_deref(), "function-editor");
    h.state_mut().fn_editor = None;

    // The Levels editor: the input histogram under the transfer curve, with its handles.
    h.state_mut().run("file.new", json!({"template": "stone_tiles", "size": 512}));
    let levels = {
        let g = h.state().session.graph().unwrap();
        g.nodes.iter().find(|n| n.def == "levels" && g.connections.iter().any(|c| c.to == n.id)).map(|n| n.id.0)
    };
    if let Some(id) = levels {
        h.state_mut().run("graph.select", json!({"ids": [id]}));
        h.state_mut().run("graph.node.param", json!({"id": id, "param": "in_low", "value": 0.3}));
        h.state_mut().run("graph.node.param", json!({"id": id, "param": "in_mid", "value": 0.35}));
        h.state_mut().run("graph.node.param", json!({"id": id, "param": "in_high", "value": 0.85}));
        h.state_mut().run("view.2d", json!({"node": id}));
    }
    h.state_mut().ui.frame_all = true;
    settle(&mut h, 40);
    save(&mut h, dir.as_deref(), "levels-editor");

    // An SBS file with nodes MaterialCraft doesn't have: placeholders on the canvas, the warnings
    // in the Console, which comes forward by itself.
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sbs/all_nodes.sbs");
    materialcraft_ui_egui::windows::open_path(h.state_mut(), sample.to_str().unwrap());
    settle(&mut h, 60);
    save(&mut h, dir.as_deref(), "sbs-warnings");
}

/// Steady-state UI build time per frame with each panel closed in turn (release):
/// `cargo test -p materialcraft-ui-egui --release --test snapshot perf -- --ignored --nocapture`.
#[test]
#[ignore]
fn perf() {
    use materialcraft_ui_egui::docking::Panel;
    let app = MaterialCraftApp::headless(materialcraft_engine::sample_session(), UiState::default());
    let mut h = harness(app, 1600.0, 960.0);
    settle(&mut h, 60);
    let measure = |h: &mut Harness<'static, MaterialCraftApp>| {
        let t = std::time::Instant::now();
        for _ in 0..30 {
            h.step();
        }
        t.elapsed().as_secs_f64() * 1000.0 / 30.0
    };
    println!("all panels: {:.2} ms/frame", measure(&mut h));
    for p in [Panel::Library, Panel::View3d, Panel::View2d, Panel::Properties, Panel::Explorer] {
        h.state_mut().ui.dock.close(p);
        settle(&mut h, 3);
        println!("without {p:?}: {:.2} ms/frame", measure(&mut h));
        h.state_mut().ui.dock.focus(p);
        settle(&mut h, 3);
    }
}
