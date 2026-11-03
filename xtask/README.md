# MaterialCraft xtask

The `xtask` binary dispatches `version`, `ico`, `icns`, and `layers` to
`craft-xtask`. MaterialCraft's layer table, external UI/GPU dependency rules,
and adjacent version-staging policy live in `src/task_policy.rs`. The original
`layers` report formatting stays in `src/main.rs` over `craft_xtask::layers::Report`
because the shared `run(Config, args)` prints a terse summary; its metadata
parsing and rule checking are shared. `wasm` and `ci` remain app-specific.

`tests/golden/` captures the original command output on the pre-migration
checkout (2026-11-03). The `.ico` and `.icns` fixtures store the original packed
binaries as hexadecimal, and the `version` fixtures exercise `version::compute`
(`Plan`) and `version::apply` on a temporary copy without modifying the
workspace.
