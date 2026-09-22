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
images and folders, installs queues, and drives the loader; file_drop.rs classifies hovered and
dropped paths and paints the drop overlay; crop_commands.rs maps ratio, move,
resize, and snap commands onto crop geometry and resolves catalog sizes; capture.rs submits
exports and polls their results; output_actions.rs copies the crop to the clipboard and reveals
the last export; shortcuts.rs resolves bound commands from keyboard input and runs them;
shortcut_editor.rs draws the shortcut modal; panels.rs draws the toolbar,
File menu, recent entries, and status bar; crop_position.rs draws the footer position
fields and position lock; size_rail.rs draws the footer size rail and crop dimensions; icons.rs declares the Phosphor icon subset and registers
it with egui at startup; format_bar.rs draws the docked ratio chips, catalog
picker, and custom fields; dialogs.rs draws the settings and about modals, including the editable source and destination path
rows; path_field.rs normalizes typed or pasted paths and resolves them against probe results;
workspace.rs owns zoom and workspace state, the central panel, the launcher, and overlay painting;
interaction.rs handles pointer drag, panning, wheel resize, and magnetic snapping.

Source space crop geometry and snap ladders are in src/crop.rs. Catalog parsing, validation,
orientation grouping, and tier derivation are in src/presets.rs. Viewport transforms, visible-tile
selection, display-level selection, and lazy GPU texture residency are in src/viewport.rs.
Configuration persistence is in src/config.rs. The breadth-first streaming directory walk, natural
sorting, the incrementally mergeable ImageQueue, decoding, and the half-resolution display copy are
in src/image_io.rs. Directory scanning, path existence probing, and native file dialogs run on the
worker threads owned by src/filesystem.rs. Background decode scheduling, stale-request handling,
neighbor prefetch, and decoded-source caching are in src/loader.rs.
Filename templates and collision resolution are in src/naming.rs. Background encoding and export
validation are in src/export.rs; its crop_pixels is the single source of exported and copied
pixels. Clipboard image writing runs on the worker in src/clipboard.rs, and the platform reveal
commands are built in src/file_manager.rs. The ratio catalog and tier sizes are constants in src/presets.rs;
every preset dimension is derived from a tier's longest side and the ratio, never stored.

Developer tooling that is not part of the application lives in the xtask crate, a workspace member
that is excluded from the default build. screenshot.rs owns its arguments, capture verification,
and pixel conversion, while x11_capture.rs owns the X11 connection. New developer tasks belong
there as further subcommands rather than as loose scripts.

## Runtime flow

An image or folder selection submits a scan to FilesystemService and returns immediately; the UI
thread never inspects the selection itself. The scan worker distinguishes a file from a folder with
one metadata call and streams naturally sorted batches. The first batch builds an ImageQueue and
submits its current path to ImageLoader, so an image opens while the walk is still running. Later
batches are buffered, sorted, and merged in linear time, and the merge shifts the current index so
the displayed image stays in place while the queue grows around it. Every selection advances a
generation counter, which cancels a walk still in progress and discards events already in flight.

Two decode workers discard stale requests before decoding and request an egui repaint when a
result is ready. Successful sources enter a byte-budgeted least-recently-used cache. The app
installs only the awaited path while retaining other results for navigation, then prefetches the
next and previous queue neighbors, including after a merge makes a neighbor valid. The displayed
source remains interactive during loading and for the remainder of the scan.

A separate probe worker answers path existence questions, so a scan of a large tree cannot delay a
probe and a probe against an unreachable mount cannot delay a scan. Its results feed the recent
source list and the editable path fields, which is why neither the launcher nor the settings modal
performs a filesystem call during a frame. Field input is debounced before a probe is issued, and a
result is applied only while it still matches the current draft. Native dialogs run on detached
threads and report back through the same event channel.

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
nominal shape. Export requests clone the shared source reference and enter an unbounded single
worker queue, so a capture is never rejected.

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

Image decoding, exports, directory scanning, path probing, and native dialogs must remain off the
UI thread. No frame may perform a blocking filesystem call; the one deliberate exception is the
single existence check in name resolution, which must stay synchronous because the capture needs
its index in the same frame for the history label and the status message.

The export queue must never reject a capture. It is deliberately unbounded, because a rejection
previously discarded the crop, its history entry, and the advance together while consuming a job
identifier. Queue memory is bounded in practice because crops from one page share one reference
counted source.

A queue that grows during a scan must keep the displayed image in place; only its reported position
may change. Output creation must refuse silent overwrite, and the atomic create-new open is that
guard, not the name resolution probe that precedes it. Configuration replacement must remain atomic
within its destination directory, and persisted settings equality must ignore in-memory bookkeeping
such as the mutation counter.

GPU residency must stay bounded by the visible region rather than source height, and exported
pixels must never depend on the display copy.

The application remains offline. New network services, telemetry, or large runtime dependencies
require an explicit product decision. The only external processes are the file manager commands in
src/file_manager.rs, which are spawned with separate arguments and never through a shell.

Clipboard and reveal actions are explicit and optional. Neither may change export behavior: a
failed copy or a desktop with no file manager reports a status message and nothing else.

## Change boundaries

Geometry changes belong in crop.rs and require focused unit tests. Catalog ratio or tier changes
belong in presets.rs and require tests for the derived dimensions.
Viewport math belongs in viewport.rs and must be tested independently from egui rendering. The
directory walk, queue merging, decoding, and the display copy belong in image_io.rs; scan, probe,
and dialog threading belongs in filesystem.rs; path normalization and validation rules belong in
src/app/path_field.rs; load scheduling and caching belong in loader.rs; naming rules belong in
naming.rs; and codec or worker changes belong in export.rs.

Keep the src/app/ modules focused on orchestration and interaction, each on one concern and under
900 lines. Extract reusable domain behavior before duplicating it in widgets or event handlers.
Preserve user changes in a dirty worktree and give parallel agents exclusive file ownership or a
read only review task.

Crop placement geometry belongs on CropRect in crop.rs. src/app/shortcuts.rs and
src/app/interaction.rs map keyboard and pointer input to that geometry and update only interaction
state such as viewport visibility.

## Shortcut allocation

Commands and their default chords live in one table, SHORTCUTS in src/shortcuts.rs, which also
supplies the editor labels, grouping, and matching rules. A new command is a new entry in that
table, a new ShortcutAction variant in the same order, and one arm in run_shortcut in
src/app/shortcuts.rs; nothing else enumerates commands. The default map is asserted to be free of
collisions, so a new default must not overlap an existing one. The defaults occupy arrows,
W/A/S/D, Q/E, Space, Enter, F, R, number keys 1 through 9, brackets, Home, End, Plus, Minus, and
the Ctrl combinations listed in the controls table of workflows.md; unmodified C stays free for
the centering command on the roadmap.

A command declares the modifiers its binding ignores. Movement ignores Ctrl and Shift because they
select the step size, crop resizing ignores Ctrl for the same reason, and zooming ignores Shift so
that both Plus and Shift+Equals reach it. Collision detection accounts for those free modifiers, so
Shift+bracket can belong to a different command than bracket. A command that should react to a held
key must set repeats; Ctrl+C never arrives as a key event on winit, so the clipboard event is
translated into the Ctrl+C chord before matching.

## Development workflow

Inspect the affected module, its tests, AGENTS.md, and the relevant product or roadmap section
before editing. State the behavior and invariants that will change. Add or update tests with the
domain change, update user documentation when controls change, and update implementation.md when a
dependency choice, design decision, or performance characteristic changes.

Clipboard round trip tests use the real system clipboard and therefore replace its contents. They
run wherever a display is available and are skipped on a headless Linux session; CI covers them on
the Windows runner, which also builds the Windows reveal command.

CI runs once per change: on pull requests, on pushes to master, and on manual dispatch. Feature
branch pushes are covered by their pull request, and the concurrency group is keyed by pull request
number, so a new push cancels the stale run instead of adding a duplicate. Master runs are never
cancelled, so a release in progress always completes.

Releases follow the version in Cargo.toml and are never tagged by hand. When a push to master
carries a cropdeck version whose v tag does not exist yet, the same run builds every platform, then
creates that tag on the pushed commit and publishes the GitHub Release with the binaries and
checksums. A tag created by the workflow token starts no further run, so a release never builds
twice. To release, bump the version in Cargo.toml in the change that should ship. The version is
read from the cropdeck package by name, because the workspace also contains the unversioned xtask
crate. Tests
that check absolute paths build them from std::env::temp_dir(), because a Unix path such as /tmp is
relative on Windows.

Run cargo fmt --all after Rust edits. Before handoff or commit, run cargo fmt --all --check,
cargo build, cargo test --workspace, cargo clippy --workspace --all-targets --all-features --
-D warnings, and cargo build --release. The workspace flags matter because the application is the
only default member; without them the xtask crate is never checked. A
UI or load-path change also requires a native smoke test that exercises the changed behavior
instead of only opening the executable. Linux specific claims require the corresponding CI or
device result rather than inference from a Windows build.

Keyboard interaction changes require focused command or event routing tests where practical.
Load-path changes require timings on the same class of source before and after, compared against
the performance characteristics in implementation.md.

### Native smoke procedure

The procedure differs by platform, but the rule does not: capture the application window, never
the desktop, and verify the capture before reading it.

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

### Window capture on Linux

Capture one window with the repository's own task runner:

```shell
cargo xtask screenshot --output screenshots/cropdeck.png
```

It reads the window's pixels straight from the X server and writes the PNG itself, so a warm run
costs about 0.1 second and needs no screenshot utility. It verifies the capture before it is read
and fails when no window of that name is mapped, when another window overlaps it, when the capture
covers the whole desktop, and when the window is smaller than --min-side. The overlap check matters
most: X11 hands back whatever is on screen inside the window's geometry, so a capture of a covered
window silently shows the other application. A desktop screenshot of a multi monitor session shows
nothing useful about a window level change either, so a capture that has not passed these checks is
not evidence.

--name selects another window, and --activate raises and focuses the target first, which takes
focus from the user and is therefore opt in. The default output is screenshots/cropdeck.png, which
git ignores. Window capture is implemented for X11; the task reports that plainly elsewhere, and
Windows smoke runs keep using the PrintWindow procedure above, because reaching it would require
unsafe FFI that this repository does not allow.

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
