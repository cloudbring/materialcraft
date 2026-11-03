# MaterialCraft roadmap

> **Last reviewed:** 2026-11-02 · **Last updated:** 2026-11-02 · **Change:** minor (≈ 67%: portable materials for TextureCraft) · **Target:** the professional node-based material workflow

**Stage:** alpha (in active development). MaterialCraft aims for full parity with the professional
node-based procedural material workflow, with better performance, every feature drivable by agents
(MCP, CLI, control channel), and builds for Windows, macOS, Linux, FreeBSD and the web.

## Headline numbers

- **Parity: ≈ 67%** of a material artist's daily workflow works end to end
  ([docs/target-app-parity.md](docs/target-app-parity.md)).
- **Remaining: ≈ 83 Opus 5.5 agent hours** ([docs/roadmap.md](docs/roadmap.md)).
- **Nodes:** 227 (all tileable), cooked on every CPU core through a content-addressed cache; the
  brick wall sample cooks in about 40 ms at preview size.

## By area

| Area | Weight | Done |
|---|---:|---:|
| Graph editing | 15 | 75% |
| Atomic nodes | 12 | 95% |
| Node library | 20 | 45% |
| Function graphs | 10 | 70% |
| Graph parameters and instances | 8 | 70% |
| 2D view | 5 | 80% |
| 3D view | 10 | 55% |
| Files | 8 | 70% |
| UI and UX | 8 | 70% |
| Platforms | 4 | 50% |
| **Total** | **100** | **≈ 67%** |

## What's next

1. The rest of the node library: scan processing, more grunges, spline tools, SDF shapes.
2. A GPU 3D view with tessellation and lights.
3. Mesh-based generators and bakers.
4. Function loops and reusable functions.
5. The first release on every platform.

The ranked work list is [docs/gaps.md](docs/gaps.md); file formats are in
[docs/file-format-parity.md](docs/file-format-parity.md) and languages in
[docs/localization-parity.md](docs/localization-parity.md).

## Progress log

| Date | What landed |
|---|---|
| 2026-11-03 | `xtask` delegates `version`, `ico`, `icns` and `layers` checking to shared `craft-xtask`; `wasm`, `ci` and the `layers` table report stay local with golden regression coverage |
| 2026-11-02 | Cross-app materials: `.mcmat` portable materials, Send to TextureCraft, input-node preview images (mesh maps from TextureCraft), live re-cooks from TextureCraft through the CLI |
| 2026-11-02 | Visual function editor, dynamic parameters, presets and groups, 105 more nodes (flood fill, FX-Map, SVG, Text, 3D noises, weathering, material blending), mesh import, browser file I/O, 2D histogram, CLI `--set`, better samples |
| 2026-11-02 | First day: engine, ~120 nodes, cooking cache, packages, export, the full editor UI, 3D preview, CLI, control channel with synthetic input, MCP server, wasm builds |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-02 | minor | ≈ 66% → ≈ 67% (portable materials) |
| 2026-11-02 | major | ≈ 51% → ≈ 66% |
| 2026-11-02 | major | First version |
