# Agent Development Guide

Read [AGENTS.md](../AGENTS.md) before changing the repository. Then read [README.md](../README.md)
for user visible behavior, this document for the current architecture and workflow,
[workflows.md](workflows.md) for detailed user interactions, [roadmap.md](roadmap.md) for planned
work, and [implementation.md](implementation.md) for dependency choices, design rationale, and
performance characteristics. Treat roadmap items as proposals rather than implemented behavior.

No document records verification results as current state. Every required gate is rerun after
each code change, and git history is the only record of what changed when.

## Repository map

The native entry point is src/main.rs. The application lives in the src/app/ module directory:
mod.rs owns CropDeckApp, its construction, and the eframe App implementation; sources.rs opens
images and folders, installs queues, and drives the loader; crop_commands.rs maps ratio, move,
resize, and snap commands onto crop geometry and resolves catalog sizes; capture.rs submits
exports and polls their results; shortcuts.rs routes keyboard input; panels.rs draws the toolbar,
File menu, recent entries, and status bar; format_bar.rs draws the docked ratio chips, catalog
picker, size rail, and custom fields; dialogs.rs draws the settings and about modals; workspace.rs owns zoom and workspace state, the central panel, and
overlay painting; interaction.rs handles pointer drag, panning, wheel resize, and magnetic
snapping.

Source space crop geometry and snap ladders are in src/crop.rs. Catalog parsing, validation,
orientation grouping, and tier derivation are in src/presets.rs. Viewport transforms, visible-tile
selection, display-level selection, and lazy GPU texture residency are in src/viewport.rs.
Configuration persistence is in src/config.rs. Source discovery, natural sorting, decoding, and
the half-resolution display copy are in src/image_io.rs. Background decode scheduling,
stale-request handling, neighbor prefetch, and decoded-source caching are in src/loader.rs.
Filename templates and collision resolution are in src/naming.rs. Background encoding and export
validation are in src/export.rs. aspect_ratio_catalog.json is compiled into the binary and is the
authoritative source for nominal catalog ratios and tier dimensions.

## Runtime flow

An image or folder selection creates an ImageQueue and submits its current path to ImageLoader.
Two decode workers discard stale requests before decoding and request an egui repaint when a
result is ready. Successful sources enter a byte-budgeted least-recently-used cache. The app
installs only the awaited path while retaining other results for navigation, then prefetches the
next and previous queue neighbors. The displayed source remains interactive during loading.

Every decoded SourceImage also carries a half-resolution copy produced by a 2 x 2 box filter on
the worker thread, and the cache budget counts both copies. TiledTexture keeps one lazy tile level
per copy and records bounded-height source ranges without uploading during installation. Painting
at a display scale of 0.5 or below uses the half level and releases the full level; larger scales
do the reverse, so only one level is resident. Each paint maps the viewport clip back to source
rows, uploads at most two missing visible or neighboring tiles, requests another frame when more
are needed, and releases textures beyond a two-tile margin. Tile conversion reads directly from
shared native RGB or RGBA storage without an intermediate full-image RGBA copy. WorkspaceState
owns the current source space CropRect, display zoom, drag state, resize wheel hysteresis, and
history.

The selected nominal AspectRatio is persisted in AppConfig. A snapped PresetDimensions value may
have a slightly different effective ratio because dimensions are rounded to multiples of eight.
WorkspaceState retains that effective ratio so continued resizing does not jump back to the
nominal shape. Export requests clone the shared source reference and enter a bounded single worker
queue.

## Required invariants

Crop coordinates and dimensions remain in source pixels. Viewport zoom and window resizing must
not alter the exported region. Images must never be enlarged merely to fill the viewport.

Nominal ratio identity and effective preset dimensions must remain distinct. A nominal 2:3 XS crop
is exactly 344 by 512 pixels and must not be recomputed as a mathematical 2:3 rectangle after it
is selected.

Normal wheel input scrolls the document. Shift wheel resizes the crop, Ctrl wheel zooms the
viewport, brackets resize, Shift brackets step through presets, and Ctrl brackets provide one
pixel control. Popup or text field focus suppresses workspace shortcuts.

Crop placement commands may change only crop coordinates. They preserve crop dimensions, nominal
and effective ratios, selected size preference, snapping state, history, zoom, and export state,
and a command that moves the crop must request that the resulting rectangle be brought into view.

Image decoding and exports must remain off the UI thread. The export queue must remain bounded,
and output creation must refuse silent overwrite. Configuration replacement must remain atomic
within its destination directory.

GPU residency must stay bounded by the visible region rather than source height, and exported
pixels must never depend on the display copy.

The application remains offline. New network services, telemetry, external processes, or large
runtime dependencies require an explicit product decision.

## Change boundaries

Geometry changes belong in crop.rs and require focused unit tests. Catalog schema or tier changes
belong in presets.rs and aspect_ratio_catalog.json and require validation tests for every entry.
Viewport math belongs in viewport.rs and must be tested independently from egui rendering. File
discovery, decoding, and the display copy belong in image_io.rs, load scheduling and caching
belong in loader.rs, naming rules belong in naming.rs, and codec or worker changes belong in
export.rs.

Keep the src/app/ modules focused on orchestration and interaction, each on one concern and under
900 lines. Extract reusable domain behavior before duplicating it in widgets or event handlers.
Preserve user changes in a dirty worktree and give parallel agents exclusive file ownership or a
read only review task.

Crop placement geometry belongs on CropRect in crop.rs. src/app/shortcuts.rs and
src/app/interaction.rs map keyboard and pointer input to that geometry and update only interaction
state such as viewport visibility.

## Shortcut allocation

The fixed shortcuts currently occupy arrows, W/A/S/D, Q/E, Space, Enter, F, R, number keys 1
through 9, brackets, Home, End, Plus, and Minus, together with Shift and Ctrl variants documented
in the controls table of workflows.md. Before assigning a key, inspect both that table and
handle_shortcuts in src/app/shortcuts.rs. Avoid modified C combinations while clipboard work
remains on the roadmap.

## Development workflow

Inspect the affected module, its tests, AGENTS.md, and the relevant product or roadmap section
before editing. State the behavior and invariants that will change. Add or update tests with the
domain change, update user documentation when controls change, and update implementation.md when a
dependency choice, design decision, or performance characteristic changes.

Run cargo fmt after Rust edits. Before handoff or commit, run cargo fmt --check, cargo build,
cargo test, cargo clippy --all-targets --all-features -- -D warnings, and cargo build --release. A
UI or load-path change also requires a native smoke test that exercises the changed behavior
instead of only opening the executable. Linux specific claims require the corresponding CI or
device result rather than inference from a Windows build.

Keyboard interaction changes require focused command or event routing tests where practical.
Load-path changes require timings on the same class of source before and after, compared against
the performance characteristics in implementation.md.

### Native smoke procedure

Drive the release binary through the accessibility tree rather than synthetic input. On Windows,
UI Automation InvokePattern reaches every egui button by its label, including the File menu,
Open recent, the ratio chips, the size rail, Previous, Next, Fit width, and the zoom buttons, and works without giving the window
keyboard focus. Never use SendKeys, AppActivate, or other focus-stealing input during a smoke run,
because the developer may be using the machine. Capture the window with the user32 PrintWindow
call, which renders it even while it is occluded, and read GPU residency from the per-process GPU
memory performance counter.

Isolate settings by backing up the real configuration file, writing the smoke configuration in its
place, and restoring the backup when the run ends, even on failure. The `directories` crate
resolves the configuration folder through the platform API, so environment variable overrides do
not redirect it.

## Documentation maintenance

README.md is the concise public project entry point. product.md defines stable product intent and
boundaries. workflows.md owns detailed use cases, operating instructions, controls, and examples.
development.md describes the current architecture and agent workflow. roadmap.md contains only
work that is not yet implemented. implementation.md records dependency choices, design rationale,
and performance characteristics that must be preserved.

When behavior changes, update all affected sources of truth in the same change. Remove completed
roadmap entries or move their durable decisions into product.md, development.md, or
implementation.md. Do not record dated verification results, progress logs, or completed plans in
the documentation; git history holds them. Do not leave alternate layouts, obsolete shortcut
tables, or speculative architecture presented as current behavior.
