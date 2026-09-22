# CropDeck Implementation Decisions

This document records the durable engineering decisions behind the current code: dependency
choices, design rationale that is not obvious from the source, and measured performance
characteristics that later work must preserve. The current architecture and agent workflow are in
[development.md](development.md), stable product intent is in [product.md](product.md), and
unimplemented work is in [roadmap.md](roadmap.md). Change history lives in git, not here.

## Dependency decisions

Exact versions live in `Cargo.toml` and the resolved graph in `Cargo.lock`. Add dependencies with
`cargo add` so the constraint is recorded there rather than in prose.

Development builds optimize dependency crates at level 3 while leaving CropDeck itself
unoptimized. Decoder and egui performance therefore remains representative during normal
development without slowing incremental compilation of application code.

| Crate | Purpose and decision |
| --- | --- |
| `eframe` | Native egui shell; use `glow`, X11, and Wayland instead of `wgpu` |
| `image` | JPEG and PNG decoding and encoding only; its WebP decoder is disabled and `image-webp` must stay absent from the resolved graph |
| `webp` | libwebp-based WebP decode and lossy encode; optional `image` integration stays disabled |
| `rfd` | Native dialogs; use the XDG portal backend on Linux. It resolves with `pollster` and no DBus client of its own, so its synchronous API is a blocking wrapper over the async one and an executor would buy nothing; dialogs run on detached threads instead |
| `serde`, `serde_json` | Typed JSON settings |
| `thiserror`, `anyhow` | Library and application error context |
| `directories` | Platform correct settings location; it resolves the roaming folder through the platform API, so an `APPDATA` environment override does not redirect it |
| `natord` | Natural file ordering with a small focused implementation |
| `arboard` | System clipboard images on Windows and Linux, with the X11 and Wayland data control backends instead of hand written selection handling |
| `percent-encoding` | File URI escaping for the Linux reveal call; a Unix only dependency and far smaller than a URL parser |
| `crossbeam-channel` | Background decode, scan, probe, and export workers |
| `chrono` | Local date token with minimal clock/std features |
| `tempfile` | Filesystem tests; development dependency only |

## Design decisions

- Sources decode fully into CPU memory, but the GPU only ever holds tiles no taller than 2,048
  rows near the viewport. This avoids maximum texture height failures for very tall sources and
  keeps GPU memory independent of source height.
- Export jobs own an immutable image reference and run on a single unbounded background worker so
  encoding cannot stall input handling and a capture can never be rejected. A bounded queue was
  tried first and discarded the crop, its history entry, and the advance together when it filled.
  It also bounded the wrong quantity: every job from one page clones the same reference counted
  source, so job count and pinned bytes are unrelated.
- Existing files are never silently overwritten. Name resolution increments the requested index and
  must stay on the UI thread, because the capture needs that index in the same frame for the
  history label and the status message. The atomic create-new open is the actual overwrite guard,
  covering the window between resolution and writing that the probe cannot see.

### Clipboard and reveal

Copying reuses the export crop_pixels function, so clipboard and file output cannot drift apart,
and it runs on its own worker because resizing a crop to output dimensions is Lanczos work that
does not belong in a frame. A failed copy touches no export state.

Copy is routed from egui's Copy event rather than from a Ctrl+C key press, because egui-winit
translates the platform copy combination into that event and never delivers the key itself. That
also keeps the binding correct on any platform whose copy chord differs.

The worker holds one long lived clipboard handle. On X11 the last dropped `arboard` handle tears
down the selection owner and hands the data to a clipboard manager if one exists, so a handle
created per copy loses the image as soon as the copy returns. A write failure drops the handle so
the next copy reconnects.

Revealing is a list of candidate commands rather than one command. Linux tries the freedesktop
FileManager1 ShowItems method, which selects the file in the desktop's own file manager, then
falls back to opening the containing folder with xdg-open; Windows uses one Explorer selection.
The commands are built as program and argument values by a pure function, so both platforms are
unit testable on either host and a path containing spaces or quotes never reaches a shell. Only
ShowItems is waited on, because Explorer reports a nonzero exit code even when it succeeds.

### Filesystem access

No filesystem call may block a frame. Scanning, probing, and native dialogs therefore live behind
a service with two persistent workers and a detached thread per dialog.

Scans and probes get separate threads rather than one worker behind a job enum, because their
latencies are unrelated and unbounded in both directions: a recursive walk of a large tree would
delay an existence probe, and a probe against an unreachable network mount would delay a scan. A
shared pool has the same defect, and a fairness scheduler is more code than a second thread parked
on a channel. They are kept out of the decode pool for the same reason, since a worker walking a
directory is a worker not decoding.

Scanning streams rather than completing before the first image appears. The walk is breadth first
and sorts each directory before emitting it, which the previous stack-based walk did not need
because the whole list was sorted once at the end. Streaming makes emission order visible, and
depth-first order with unspecified directory listing would surface pages out of order.

Batches merge into the queue in linear time rather than re-sorting it, and the merge shifts the
current index so the image on screen never moves while the queue grows underneath the user. Only
the reported position changes. A queue is never constructed empty, because the current path is
indexed directly.

Cancellation is a generation counter. A new selection advances it, the walk observes the change at
its next batch and abandons, and any event already queued is discarded by the same check on the UI
side. Both halves are needed; neither alone is sufficient.

A probe reports existence and directory-ness from one metadata call, because the path fields need
both and probing twice would be wasteful. Any error other than a missing entry reports the path as
present, since a permission failure on a parent is not evidence that the entry is gone and
offering to forget a temporarily unreachable source is destructive.

Path input is validated on the probe worker after a debounce, and a result is applied only while it
still matches the draft, which makes a stale probe from an earlier keystroke harmless. A
destination that does not yet exist is accepted rather than rejected, because it is created on
first capture; only an existing file at that path, a missing parent, or a relative path is refused.

The capture path caches its prepared destination and parsed filename template behind a
configuration revision counter, so directory creation and template parsing happen once per settings
and source pair instead of on every keypress. `AppConfig` compares persisted fields only, because
the counter is in-memory bookkeeping and would otherwise make identical settings compare unequal.

### Image load path

Decoded sources retain RGB8 or RGBA8 pixels according to their native alpha requirements. JPEG
and PNG use the `image` crate, while WebP uses the already-linked libwebp implementation. Export
converts only the selected crop to RGBA, and viewport tiles convert directly from their source
layout, so no full-image RGBA promotion happens anywhere.

Image loading uses two crossbeam-backed workers. Current requests advance a generation counter,
workers discard stale queued work before decoding, and the UI installs only the path currently
awaited. Successful results also enter a least-recently-used native-pixel cache, including results
that arrived after navigation moved elsewhere. The cache defaults to 1 GiB, is configurable from
128 MiB to 16 GiB, and always retains its newest source even when that source alone exceeds the
budget. The next queue item is prefetched before the previous item.

GPU textures are resident only around the visible source rows. Source installation creates tile
metadata without uploading pixels. Painting selects intersecting tiles with logarithmic boundary
search, prioritizes visible ranges before their immediate neighbors, and uploads no more than two
tiles per frame. Textures more than two tile positions beyond the visible range are dropped.

Each decoded source also carries a half-resolution copy built by a 2 x 2 box filter on native RGB
or RGBA bytes inside the loader worker, and the source cache counts both copies. The tiled texture
keeps one lazy tile level per copy. At display scales of 0.5 or below it paints the half level and
releases the full level; above 0.5 it does the reverse, so only one level is ever resident. The
half level's last tile is stretched to the full source height so odd source heights leave no gap.
Alpha is averaged unweighted, which is acceptable for a display-only copy.

### Performance characteristics

Steady-state release timings on an Intel i9-12900K running Windows 11. They describe the current
cost model that load-path changes must not regress; rerun them on the same class of source before
and after any change to decoding, tiling, or caching.

Worker-side decoding, including the half copy, on a 38 megapixel source takes roughly 120 ms for
JPEG, 240 to 290 ms for PNG depending on compression, and 170 ms for a 26 megapixel WebP. The
half copy adds 10 to 25 percent to that time and 25 percent to resident CPU bytes per source.

UI-thread tile conversion and first-paint upload on real sources, assuming a 1900 x 1000 display
viewport:

| Source | Fit-width scale | Full tile conversion | Full upload | Half copy build | Half tile conversion | Half upload |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 9000 x 6363 JPEG | 0.21 | 13.5 ms per tile | 221 MB | 24 ms | 6.6 ms per tile | 74 MB |
| 8192 x 4096 PNG with alpha | 0.23 | 18 ms per tile | 134 MB | 13 ms | 9 ms per tile | 34 MB |
| 3840 x 3048 JPEG | 0.495 | 6 ms per tile | 32 MB | 5 ms | 2.6 ms per tile | 12 MB |
| 3508 x 4961 JPEG | 0.54 | 5.6 ms per tile | 29 MB | 8 ms | not used at fit width | |
| 2500 x 3539 JPEG | 0.76 | 4 ms per tile | 21 MB | 4 ms | not used at fit width | |
| 720 x 12753 JPEG | 1.0 | 1.2 ms per tile | 6 MB | 4 ms | not used at fit width | |

The `image` crate's Triangle resize was rejected for the half copy because it took 150 to 780 ms
on the same sources, against 4 to 24 ms for the box filter. Sources between 2,500 and 3,500 pixels
wide stay above display scale 0.5 at fit width, so only the full level serves them, and their
per-tile cost is already small.

### Crop size snapping

Crop sizing uses the built-in catalog's ratio specific XS through XXL ladder together with
proportional free resizing. The tiers use longest sides of 512, 768, 1024, 1536, 2048, and 2560
pixels; the derived side is aligned to eight pixels. The largest nominal ratio crop that fits the
source is appended to every ladder. Custom ratios derive sizes with the same tier rule, keeping the
picker, keyboard stepping, and wheel snapping on one source of truth.

Normal scroll always navigates the document. Shift+scroll anywhere over the image canvas resizes
the crop by roughly 6.25 percent per deliberate wheel action. When the next step is within one
step of a standard size, the crop moves directly onto that size. A snapped size acts as a magnetic
detent and requires three times the normal wheel travel to release. This provides fast continuous
movement without making important sizes difficult to select. Ctrl+scroll zooms the viewport,
brackets provide 16 source pixel adjustments, Ctrl+brackets provide one pixel adjustments, and
Shift+brackets jump directly between presets. Raw line/page wheel events are normalized while
precision touchpad deltas accumulate, preventing device specific scrolling scales from causing
large jumps.

The defaults prioritize open source workflows: Stability AI documents SDXL's native 1024 square
resolution and its multi aspect buckets; Black Forest Labs documents common FLUX square, portrait,
landscape, and ultrawide resolutions; HunyuanVideo documents 540p and recommended 720p profiles;
and Wan 2.1 documents 480p and 720p model families.

### Format bar

Aspect ratio and crop size are docked in the toolbar rather than in a popup, because both are
tuned repeatedly against the canvas and a popup closes on every pick and silences the keyboard
while open. Nine chips show the ratios bound to the 1 through 9 keys, each with its number, so the
bar doubles as the shortcut legend; a ratio outside the nine is shown as an extra highlighted chip.
All ratios is the only popup in the bar and is a pure picker: catalog entries in portrait,
landscape, and square seven column groups, closing on the pick. Each tile communicates shape
visually instead of relying only on text.

Resolution is a segmented XS through XXL rail with a Max action. Tiers larger than the source are
disabled with a reason. Automatic size preference highlights only a tier whose exact pixels match
the crop. The bar shows the current crop dimensions and megapixels, which also gives visible
feedback for wheel and keyboard resizing. Custom ratio fields are inline and commit on Enter, on
leaving the field, or at the end of a drag; Esc reverts an edit. As the window narrows the bar
drops the megapixel figure, moves the custom fields into the catalog picker, and finally collapses
the chips into a single picker button showing the current ratio. The rail and Capture never
collapse.

The first toolbar item is a File menu with Open image, Open folder, an Open recent submenu, Settings,
and About. Recent entries carry the file name, parent folder, and image count for folders, and
missing entries are marked rather than silently dropped. Capture is the single primary action and
shows its key. With no image open the same open actions and recent list are drawn in the empty
workspace.

Settings and About use egui's modal container so they dim the canvas, block workspace input, and
close on Esc, the backdrop, or Close. Filename template errors are shown inline in the dialog.

The footer is a status bar: queue navigation, file name, crop position, and capture count on the
left; zoom controls and the latest message on the right. Informational messages expire after four
seconds, errors persist until replaced, and the message truncates instead of wrapping so the canvas
height never changes.

### Keyboard ownership

Input focus is resolved once per frame into canvas, text field, menu, or modal. Text fields and
modals suppress workspace shortcuts; a modal also suppresses wheel zoom. An open menu keeps only
arrows, Enter, and Esc; any other workspace key closes the menu and runs, so a capture is never
lost to an open menu. Esc during a pointer drag restores the crop to where the drag began.

The catalog deliberately separates presentation from crop geometry:

- A nominal ratio is the familiar label chosen by the user, such as 2:3.
- A preset's effective ratio comes from its exact pixel dimensions. Encoder friendly rounding
  means the XS 2:3 preset is 344 x 512, or exactly 43:64.
- Selecting a preset must use and export its exact dimensions while the picker continues to show
  its nominal catalog label.
- Maximum that fits source is computed from the nominal ratio and remains separate from the six
  catalog tiers.

XS through XXL are ordered size tiers. Selecting one records that preference. A ratio change
reuses the preferred tier when its exact dimensions fit the source. Otherwise, it applies the
largest fitting catalog tier but retains the preference so a later ratio or source can restore it.
When the current crop is not a catalog size, the closest fitting crop by pixel area is selected,
with a stable smaller first tie break. Oversized tiers remain visible but disabled so users can
understand the catalog and why a choice is unavailable.

Keyboard access mirrors the picker without changing shortcuts based on which section is visible.
R cycles forward through six quick ratios and Shift+R cycles backward. Entering the cycle from a
custom or other catalog ratio selects 1:1 forward or 16:9 backward. Number keys 1 through 9 retain
their stable direct mappings to 1:1, 2:3, 3:2, 4:3, 3:4, 4:5, 16:9, 9:16, and 7:3; matching badges
appear on those tiles. Crop shortcuts are suspended while a text or numeric field has keyboard
focus.

Catalog and interaction tests cover schema validation, all 29 nominal ratios and six tiers,
orientation grouping, stable cycle and number key order, exact source bound filtering, disabled
oversized choices, maximum fit availability, preferred tier fallback and restoration, area nearest
selection, and the nominal 2:3 versus effective 344 x 512 distinction. Viewport and interaction
tests cover normal scrolling, modifier based zoom and resizing, magnetic attraction, snap release,
visible tile selection, per-frame upload limits, tile release, and display level switching.
