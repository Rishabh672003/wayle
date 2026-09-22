# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Wayle is a Wayland desktop shell (bar, notifications, OSD, wallpaper, device controls) in Rust with GTK4 + Relm4. **This repository is a fork of [wayle-rs/wayle](https://github.com/wayle-rs/wayle)**; the default branch is `master` (there is no `main`). Fork-added features are listed in `README.md`.

Crucially, **the shell/CLI in this repo is a thin layer over external published crates that provide the actual services**: `wayle-core` (the reactive `Property<T>` primitive and `ConfigPaths`), `wayle-notification`, `wayle-systray`, `wayle-hyprland`, `wayle-audio`, `wayle-network`, `wayle-power-profiles`, `wayle-bluetooth`, `wayle-niri`, `wayle-mango`. These are **not in the workspace** — they live in `~/.local/share/cargo/registry/src/`. Service state, D-Bus event parsing, and the `Property`/service APIs are defined there. To change service or event-parsing behavior you must patch the external crate (or work around it here), not edit this repo.

## Commands

```bash
cargo run -- shell                     # run the desktop shell (GUI; manages its own tokio runtime)
cargo run -- <subcommand>              # run a CLI subcommand (notify, config, audio, media, systray, ...)
cargo +nightly fmt --all               # format (rustfmt is nightly-only here; CI checks with --check)
cargo clippy --workspace --all-targets -- -D warnings   # lint (must be clean; CI denies warnings)
cargo test --workspace --no-fail-fast  # all tests
cargo test -p wayle-shell <name>       # single test / crate subset
./scripts/ci/check-icons.sh            # verify every icon referenced in config/defaults is installed
./packaging/arch/build.sh              # build the local Arch .pkg.tar.zst (release build, slow)
```

Release builds use `lto = true` + `codegen-units = 1` — expect them to be slow; use `cargo check` while iterating.

## Lint policy (affects how you write code)

Workspace lints in the root `Cargo.toml` are strict and CI-enforced (`-D warnings`):

- `unwrap_used` and `expect_used` are **deny** — never use `.unwrap()`/`.expect()`. Use `let ... else`, `match`, `?`, or `if let`.
- `unsafe_code` is **deny**.
- `panic` is warn; `missing_docs`, `missing_errors_doc`, `missing_panics_doc` are warn — public items need doc comments, and fallible/panicking public fns need `# Errors` / `# Panics` sections.
- `too_many_lines`, `cognitive_complexity` are warn — keep functions small.

## Architecture

### Binary vs. shell (`wayle/`)
`wayle/src/main.rs` is the single entry point. `wayle shell` launches the GUI directly; every other subcommand (`notify`, `config`, `audio`, `media`, `systray`, `power`, `wallpaper`, `idle`, `panel`, `icons`) runs as a short-lived CLI on a shared tokio runtime under `wayle/src/cli/`.

### Reactive + Relm4 model
State is `Property<T>` (from external `wayle-core`); consumers call `.watch()` to get a change stream. UI is Relm4 `Component`/`FactoryComponent` with the `view!` macro and message enums. Watcher tasks that bridge services → UI run either on `tokio::spawn` (async / D-Bus work) or `relm4::spawn_local` (must touch GTK on the main thread). This split matters: D-Bus calls belong on `tokio::spawn`, widget mutation on `spawn_local`.

Almost everything external is over **D-Bus** via `zbus` (tokio feature): SNI tray, NetworkManager, UPower, MPRIS, etc.

### Startup (`crates/wayle-shell/src/bootstrap/mod.rs`)
All services are `tokio::spawn`ed and awaited together with `tokio::join!`, each timed. They load in parallel, not sequentially — reported per-module startup times overlap.

### CLI ↔ shell IPC (`crates/wayle-ipc/`)
`wayle-ipc` declares zbus proxy traits (e.g. `ShellIpcProxy`); the shell implements them in `crates/wayle-shell/src/services/shell_ipc/`. CLI subcommands call the proxy to drive the running shell. (Exception: `wayle notify history` reads a state file directly rather than going through D-Bus.)

### Config (`crates/wayle-config/`)
`config.toml` (user) plus `runtime.toml` (GUI-written) are TOML, hot-reloaded by a file watcher. `ConfigService` exposes typed `Property` fields; schemas generate a JSON schema for editor validation. All paths (config/data/state dirs, XDG-based) go through `ConfigPaths` (`wayle_core::paths`, re-exported via `wayle_config::infrastructure::paths`) — use `ConfigPaths::state_dir()` etc., don't hand-roll `$HOME` paths. Persistent runtime state lives in `~/.local/state/wayle/` (logs, `notification-history.jsonl`).

### Styling (`crates/wayle-styling/`)
SCSS under `scss/` is compiled and **embedded into the binary at build time** (`build.rs`), so any style change requires a rebuild. Structure: `tokens/`, `primitives/`, `components/`, `modules/`. Theme colors are CSS custom properties (`--bg-*`, `--fg-*`, `--space-*`, `--rounding-*`, `--text-*`) — use tokens, not hard-coded values. Global GTK widgets (e.g. `tooltip`) are styled as top-level nodes in `scss/primitives/`.

### i18n
Fluent (FTL) files embedded via `rust-embed`. There are **two independent loaders**: `crates/wayle-shell/src/i18n.rs` (shell — `t!` / `td!` macros, files under `crates/wayle-shell/locales/`) and `crates/wayle-i18n/` (settings GUI — `t()` / `t_attr()`). Both parse the locale env themselves in `requested_languages()` to skip POSIX locales (`C`/`C.UTF-8`) that would otherwise error. New user-facing strings must be added to both `en-US` and `fr` FTL files; the `fl!` macro validates keys at compile time against the fallback language.

### Icons (`crates/wayle-icons/`)
Icons are symbolic, referenced by name (e.g. `ld-clock-symbolic`, `ld-bell-symbolic`). `wayle icons list` shows installed icons; config may reference only installed names, enforced by `scripts/ci/check-icons.sh`. `ld-*` names not in the installed set render as a broken/blank glyph.

### Other crates
`wayle-settings` (GTK4 settings GUI), `wayle-widgets` (shared Relm4 templates: `GhostIconButton`, `Switch`, `EmptyState`, `DropdownContent`), `wayle-derive` (proc macros), `wayle-idle-inhibit`.
