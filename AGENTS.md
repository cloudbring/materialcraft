# AGENTS.md: guide for AI agents and contributors

MaterialCraft is an open-source, native **node-based procedural material designer**: a clean-room
implementation of the workflow professional material artists know from industry-standard node
graph tools, written in **Rust only** (no JavaScript or TypeScript). **No Tauri, Electron or
webview shells:** the desktop app is native egui/eframe on wgpu, and the web build is the same Rust
compiled to WebAssembly (trunk + wasm-bindgen). Never add Tauri (or any webview/JS UI framework) as
a dependency, build step or packaging target.

The product name is always written **MaterialCraft** (`{Function}Craft` in PascalCase, like its
siblings PhotoCraft, VectorCraft, FilmCraft, LightCraft, PdfCraft, EffectCraft, DesignCraft,
TextureCraft) in user-facing text: UI, window titles, About, installers, release names, docs prose.
Machine names stay lowercase: crates (`materialcraft-*`), binaries, file names, ids
(`ai.storyteller.materialcraft`). The main project format is SBS (`.sbs`); the older native
package extension `.mcraft` is still read and written.

Standards and learnings shared across the crafting apps live in `../craftrules` (checked out next
to this repo, or `storytold/craftrules`). Read its `AGENTS.md` at the start of a session, follow its
standards, and contribute reusable learnings back there. Shared code goes only in craft-libs
(`standards/shared-libraries.md`); otherwise copy, don't depend. TextureCraft is the sibling this
repository's infrastructure (xtask, automation, packaging, docking, theme) was copied from.

The goal is full parity with the professional node-based material workflow (the same panels,
menus, node library, parameters, shortcuts and file conventions) with better performance, every
feature drivable by agents, and builds for Windows, macOS, Linux, FreeBSD and the web. Read this
file first, then `docs/`.

## 1. Orientation (5 minutes)

| Read | Why |
|---|---|
| `docs/architecture.md` | Crate map, layers, the CPU cook engine and cache, data flow |
| `docs/control-protocol.md` | The JSON control channel, synthetic input and MCP: how agents drive the app |
| `ROADMAP.md` | Honest parity estimate, milestones, **current focus** |
| `docs/target-app-parity.md` | The parity assessment by area |
| `docs/gaps.md` | Every known shortfall, ranked: the work list |
| `ATTRIBUTION.md` | Every bundled asset with author, source, license |

## 2. Workspace map

```text
crates/
  core image geom            L0 document model (package, graphs, nodes, values, function graphs); float images; meshes, BVH, camera
  nodes                      L1 the node library: definitions + CPU cooks (atomic nodes, noises, patterns, filters, PBR utilities, function evaluator)
  eval formats               L2 graph cooking (topological order, sizes, content-addressed cache, sub-graphs); .sbs and .mcraft packages, bitmaps, texture export
  engine render              L3 Session + command registry (every action is a command), inspect, samples; the 3D preview renderer (CPU PBR + IBL)
  automation                 L4 MCP server + headless JSON-lines RPC + bridge to the running app
  ui-egui                    L5 egui shell (thin: every edit goes through engine commands)
apps/
  materialcraft              desktop app (eframe/wgpu), `--control <port>` loopback control server
  materialcraft-cli          headless CLI (info, run, batch, new, cook, export, render, nodes, commands, mcp, serve)
  materialcraft-web          the same app in the browser (trunk + wasm-bindgen)
xtask/                       cargo xtask layers | wasm | ci | version | ico | icns
```

**Layering is enforced** by `cargo xtask layers`: a crate may depend only on lower layers. Nothing
below `ui-egui` may use egui, eframe, winit or rfd; only `render`, `ui-egui` and the apps may use
wgpu. A new crate must be registered in `xtask/src/layers.rs`.

**The UI crate is swappable.** `materialcraft-ui-egui` holds no document logic: it calls engine
commands (`materialcraft_engine::execute`). Another front end could replace it without touching
the engine.

## 3. Golden rules

### Never name the reference product (absolutely essential)

- The repository never names the commercial product MaterialCraft is modeled on, nor its vendor,
  in code, comments, docs, commit messages, test names, file names or UI text. Say "the reference
  app", "the professional node-based material workflow" or describe the feature itself.
- Notes that name it may live only in the gitignored `plan/` folder.

### Assets, icons and images (absolutely essential)

- **Never use any icon, image, logo, texture, material, sample graph, preset, environment, font or
  other asset from Adobe, Avid, Figma, Autodesk or any other commercial product**, and never copy
  files from an installed commercial application (its package files, compiled material archives,
  presets, sample graphs, HDR environments, UI icons). Not as reference files in the repo either.
- Every image or icon must be **open source, public domain, Creative Commons (CC0/CC-BY), or
  original** (made by a contributor who has the right to license it). Prefer generating content in
  code: MaterialCraft's noises, patterns, sample materials, environments, library previews and
  template thumbnails are all procedural.
- **Every bundled asset gets a row in [`ATTRIBUTION.md`](ATTRIBUTION.md)** (path, title, author,
  source, license) **in the same change that adds it**, plus its license file next to it when the
  license requires one. Assets compiled in through dependencies (icon fonts, default fonts) get a
  row too. No row, no merge.
- Demo images and README screenshots show MaterialCraft rendering original procedural content
  only. Never personal photos.
- The ArtCraft logos in `docs/brand/` are not open source (`docs/brand/LICENSE-brand.txt`).
- Fonts live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts); never commit font
  files here.

### Clean room

MaterialCraft is an independent, clean-room implementation of a node-based procedural material
workflow. Work from public documentation, general domain knowledge and public algorithms (Perlin
and simplex gradient noise, Worley/cellular noise, fractal sums, Gaussian and box blurs,
Felzenszwalb–Huttenlocher distance transforms, Sobel/central-difference normals, reoriented normal
mapping, HSL, levels, monotone cubic curves, W3C blend modes, GGX/Smith/Schlick PBR, split-sum IBL,
ACES fitted tone mapping). Never copy proprietary code, shaders, node implementations or presets.
SBS packages are the main project format (issue #49): `crates/formats/src/sbs.rs` reads and writes
them with MaterialCraft's own code, written from the format's public documentation and sample
files, never from another implementation. Always describe MaterialCraft as a clean-room
implementation, in code, docs and commits.

### Never crash (outranks feature work)

People trust MaterialCraft with hours of work. A malformed package, a hostile command or MCP
parameter, an odd keystroke or a full disk must produce an error the user or agent can act on,
never a panic. The shared standard is `../craftrules/standards/never-crash.md`.

- Non-test code never panics: no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or
  `unimplemented!`. Every production crate root carries
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
- No `unsafe` (`unsafe_code = "forbid"` for the workspace).
- Input-derived numbers are hostile: `get()` instead of `[i]`, checked/saturating arithmetic, caps
  on allocations (image sizes are clamped to 8192, zip entries capped), NaN guards.
- Every crash fix comes with a regression test. The engine's `panic_hunt` test runs every command
  with hostile params, and the node library's `every_node_cooks` test cooks every node with no
  inputs and with odd-sized inputs; keep both green.

### Architecture rules

1. **Everything is a command.** New user-visible behaviour = a command in the engine
   (`crates/engine/src/*_cmds.rs`, registered in `commands.rs`) with id, label, menu path, shortcut,
   params doc, `enabled` and `run`, plus tests. The menus, shortcuts, CLI, control channel and MCP
   all dispatch commands by id. Menu items come from the catalog in `crates/ui-egui/src/menus.rs`.
2. **Every node is a catalog entry.** A new node is a `NodeDef` (ports, parameters with defaults and
   ranges, library category, description) and a pure cook function in `crates/nodes`. It tiles
   (whole-number lattice scales), works at any size, scales pixel-based parameters by `w / 256`,
   and needs no new UI code: the library, the properties panel and the search pick it up.
3. **Programmatic calls never open dialogs.** A command run through the control channel or MCP runs
   with defaults; only `ui.menu.invoke` behaves like a menu click.
4. **Inspect shows the agent's work.** `session.inspect` must reveal every node, parameter, wire,
   cook error, size and mean value an agent changed.
5. **CPU oracle first.** Nodes cook on the CPU (rayon) through a content-addressed cache, so every
   node is testable and identical in the app, the CLI, MCP and the browser. Any GPU acceleration
   must match the CPU cook in parity tests.
6. **Undo is snapshots.** `Session::edit` clones the package and restores it on error, so every
   command is atomic; `edit_merged` folds a slider drag into one step.
7. **Never break wasm.** L0–L5 must `cargo check --target wasm32-unknown-unknown`
   (`cargo xtask wasm`). File-system code is `cfg(not(target_arch = "wasm32"))` with byte-based
   alternatives.
8. **Performance is a feature.** Edits re-cook only what lies downstream (the cache). Cooking runs
   on a worker thread; the UI never blocks on it. Benchmark heavy nodes in release.
9. **Cross-app formats are shared conventions.** `.mcmat` portable materials are read by
   TextureCraft with its own code: keep `docs/control-protocol.md` § Portable materials, the usage
   ids and the mesh-map input identifiers in step with TextureCraft, bump the manifest `version`
   for incompatible changes, and never make one app depend on the other's crates.
10. **Verify UI changes visually.** `MC_SNAPSHOT_DIR=<dir> cargo test -p materialcraft-ui-egui
   --release --test snapshot` renders the real UI offscreen; look at the PNGs. The same test
   compares each scene with `crates/ui-egui/tests/goldens/` and fails on a difference: after an
   intended UI change re-bless with `MC_BLESS=1` and review the PNG diff (see the README there).

## 4. Picking work

1. `ROADMAP.md` → **Current focus** and the parity table.
2. `docs/gaps.md` → the ranked list of shortfalls.
3. Render snapshots of MaterialCraft and fix the biggest visible gaps in the workflow.
4. `log/devlog.md` (local) → the "Still open" bullets of recent entries.

## 5. Before you finish a task

```sh
cargo fmt --all
cargo test -p <crates you touched> --release
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask layers
cargo xtask wasm            # if you touched L0–L5
cargo test -p materialcraft-engine --release --test session panic_hunt   # if you added/changed commands
```

Then append a terse entry to `log/devlog.md` (what landed, numbers, what's still open). Commit
after every arc of work that builds and passes.

## 6. Parallel agents

- Use your own target dir (`CARGO_TARGET_DIR=target/agent-<name>`) and edit only the files you own.
  Shared files (`engine/src/commands.rs`, `nodes/src/catalog.rs`, `ui-egui/src/menus.rs`,
  `ui-egui/src/lib.rs`, the root `Cargo.toml`) get small, surgical edits; re-read before editing.
- Put new commands in a **new module** with its own `specs()` registered by one line, and new nodes
  in the module of their library category.
- Delete your `target/agent-*` dir when done.

## 7. Where things are tracked

- `ROADMAP.md`: milestones, parity estimate and remaining effort.
- `docs/target-app-parity.md` and `docs/gaps.md`: the assessment and the work list.
- `docs/releasing.md` and `../craftrules/release/playbook.md`: release builds and signing.
- `plan/` (local, gitignored): execution plan and notes.
- `log/` (local, gitignored): the dev log.
