# CropDeck Workflows and Use Cases

## Motivation
Large sequential images are awkward to process in general purpose editors. Repeatedly creating a ratio locked selection, exporting it safely, and moving to the next region adds unnecessary tool switching and interface work. CropDeck concentrates on this extraction loop and measures success by the time and number of interactions needed for each useful crop.

All processing stays on the local machine. CropDeck does not require a cloud account, transmit source images, or embed generation services. Source coordinate crop geometry also means that viewport scaling and window resizing cannot silently change the extracted pixels.

## Suitable workloads

### Sequential artwork

Comics, manga, manhwa, webnovels, and vertically assembled chapter images can contain many useful frames in one tall source. Progressive edge scrolling and capture and advance let the crop move through the document without repeatedly switching tools.

### Storyboards and contact sheets

A fixed ratio can be applied consistently while extracting individual compositions from a board. Crop history marks completed regions, while a naturally sorted folder queue keeps related sources in sequence.

### Image and video generation inputs

The ratio catalog and XS through XXL size tiers provide repeatable frame shapes for downstream image and video generation workflows. Nominal labels remain recognizable while exact dimensions use encoder friendly alignment.

### Dataset and research preparation

Source pixel coordinates, deterministic naming templates, and non overwriting output make repeated manual extraction suitable for preparing reviewed image collections without altering originals.

## Core workflow

1. Open one supported image or a folder of images.
2. Click a quick ratio chip, pick any catalog ratio from All ratios, or type a custom ratio.
3. Choose a size tier on the rail or Max for the largest crop that fits the source.
4. Position the crop with the pointer or keyboard.
5. Press Space or Enter to export and advance by the configured amount.
6. Continue through the source or move to the next image in the queue.

Folder queues use natural filename ordering and can include nested folders when recursive scanning is enabled. Queue navigation appears only when more than one image is loaded. The File menu and the empty workspace both list the ten most recently opened images and folders; entries that no longer exist are marked and can be removed by clicking them.

Image decoding runs in the background. The displayed image stays available for dragging,
scrolling, and capture while another source loads. CropDeck prefetches the next image and then the
previous image, and keeps recently decoded sources in a least-recently-used cache so reversing
direction normally avoids another decode.

Very tall images are uploaded to the GPU on demand. CropDeck prepares visible tiles first, warms
one neighboring tile, limits upload work per frame, and releases tiles after they move more than
two tiles away from the viewport. Zooming out to 50 percent or less switches to a half-resolution
copy of the source that was prepared during loading, which quarters upload work for very wide
sources and smooths minification. CPU cache use follows the configured budget, which counts both
copies, while GPU residency remains bounded by the visible region rather than source height.

## Example workflows

### Extracting portrait frames from a tall chapter image

Open the chapter image, choose 9:16, and select the largest useful size that fits. Drag the crop to the first composition and capture it. CropDeck records the completed region and advances the crop downward according to the configured overlap, ready for the next adjustment.

### Preparing a consistent folder of model inputs

Open the source folder and select a generation oriented tier. Move between naturally sorted images with Q and E, refine each crop in source pixel increments when needed, and capture. The selected tier is retained across ratio and source changes whenever it fits.

### Mixing a familiar ratio with aligned dimensions

A nominal 2:3 XS preset is 344 by 512 pixels rather than recomputing an unaligned mathematical rectangle. CropDeck displays the familiar 2:3 identity while preserving and exporting the exact preset dimensions when output resizing is disabled.

## Crop sizing behavior

The format picker shows every catalog ratio in portrait, landscape, and square groups. Size tiers run from XS through XXL, and Max selects the largest ratio locked crop that fits the current source. Oversized tiers remain visible but disabled.

Changing ratios retains an explicitly selected tier when it fits. If it does not fit, CropDeck uses the largest available tier without forgetting the preference. Magnetic resizing moves continuously, pulls the crop toward nearby catalog sizes, and requires deliberate movement to leave a snapped size.

## Performance settings

The decoded image cache defaults to 1 GiB and can be configured from 128 MiB to 16 GiB in
Settings. The limit applies when CropDeck next starts. If one decoded image exceeds the limit, it
is retained by itself so the active source remains reusable.

## Controls

| Input | Action |
| --- | --- |
| Ctrl + O / Ctrl + Shift + O | Open an image / a folder |
| Ctrl + , | Open Settings |
| Esc | Close a menu, the catalog picker, or a dialog; cancel a crop drag |
| Arrow keys or W/A/S/D | Move the crop |
| Shift + movement | Move farther |
| Ctrl + movement | Move by one source pixel |
| Page Up / Page Down | Move by approximately one crop height |
| Q / E | Previous / next source |
| Space or Enter | Capture and advance |
| F | Fit image width |
| R / Shift + R | Cycle forward / backward through the six quick ratios |
| 1 through 9 | Select the numbered quick ratio chip in the format bar |
| `[` / `]` | Fine ratio locked crop resizing |
| Ctrl + `[` / `]` | One source pixel crop resizing |
| Shift + `[` / `]` | Previous / next generation oriented crop size |
| Scroll | Navigate the image |
| Shift + scroll over the canvas | Resize continuously with magnetic size snapping |
| Ctrl + scroll | Zoom the viewport |
| Home / End | Move to the top / bottom of the image |
| Pointer drag inside crop | Move the crop and auto scroll near viewport edges |
| Space + pointer drag or middle drag | Pan the document |

## Output behavior

Exports can retain the crop's source dimensions or resize to configured output dimensions. Naming templates support source and crop information, and collision resolution creates a new filename instead of replacing an existing export. Completed crops remain visible as indexed history overlays during the current source session.
