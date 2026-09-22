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

Scanning a folder does not block the window. The first image opens as soon as it is found and the
rest of the folder keeps loading behind it, so the queue total climbs while cropping is already
under way. The image on screen never changes position in the process; only the reported count
does. A marker beside the count shows a scan is still running, and opening a different source
abandons it.

One image, several images, or one folder can also be dropped onto the window. While the pointer
carries them, the canvas dims and names what the drop will open, so a mistaken drag can be carried
back out. A single dropped item is scanned exactly as a dialog choice is, and several dropped
images become one naturally sorted queue without scanning their folder. A drop that mixes images
with anything else is refused by name and changes nothing.

A source can be typed or pasted instead of chosen through a dialog, from the field on the empty
workspace or the Source row in Settings. Paths wrapped in quotes or prefixed with ~ are accepted.
A path that does not exist, or a file that is not a supported image, is reported under the field
and nothing is opened.

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

## Export destination

By default each crop is written beside its source image. The Destination row in Settings accepts a
typed or pasted folder path, a folder chosen through the dialog, or Use source folder to return to
the default. A destination that does not exist yet is accepted and created with the first capture.
An existing file at that path, a missing parent folder, or a relative path is refused and reported
under the field, leaving the previous destination in place.

Validation runs in the background, so a path on a slow or unreachable network mount leaves that one
field waiting while the rest of the dialog stays usable.

## Performance settings

The decoded image cache defaults to 1 GiB and can be configured from 128 MiB to 16 GiB in
Settings. The limit applies when CropDeck next starts. If one decoded image exceeds the limit, it
is retained by itself so the active source remains reusable.

## Controls

Every entry below is the shipped default. All of them can be rebound from Keyboard shortcuts in
the File menu, so the table describes a fresh installation rather than a fixed map.

| Input | Action |
| --- | --- |
| Ctrl + O / Ctrl + Shift + O | Open an image / a folder |
| Ctrl + , | Open Settings |
| Ctrl + Shift + K | Open Keyboard shortcuts |
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
| Drop files on the window | Open one image, several images, or one folder |
| Space + pointer drag or middle drag | Pan the document |
| Ctrl + C | Copy the current crop to the system clipboard |
| Ctrl + Shift + R | Reveal the last export in the file manager |

## Keyboard shortcuts

Keyboard shortcuts is a modal in the File menu. It lists every command by group, shows the
shortcuts bound to each one, and records a replacement when a shortcut is clicked: the next key
combination is captured, Esc abandons the recording, and Backspace clears the binding. A command
can hold up to three shortcuts; the plus button adds one and each x removes one. The filter field
matches command names, group names, and bound shortcuts, so an occupied combination can be traced
to its owner.

A combination that is already taken is not applied silently. The editor names the command that
holds it and offers to reassign it or to keep the current one. Reset restores one command, and
Reset to defaults restores every command after a confirmation. Shortcut changes take effect
immediately and are stored in the settings file.

Movement, resizing, and zooming keep their modifier variants without separate bindings. Whatever
Move crop left is bound to, holding Shift moves farther and holding Ctrl moves by one source
pixel; the same applies to Ctrl with the crop resizing commands. Previous and next preset size,
and backward ratio cycling, are commands in their own right and can be bound freely.

## Clipboard and file manager

Copy places the current crop on the system clipboard as an image, using the same pixels and the
same optional output resize as an export, without writing a file. Encoding runs on a background
worker, so a large crop never stalls the canvas, and the status bar reports the copied dimensions.
On Linux the clipboard is served by the running application, so a copied crop stays available to
other programs until CropDeck exits, which is how X11 and Wayland selections work.

Reveal appears in the status bar once a capture has completed and shows that file in the platform
file manager: Explorer with the file selected on Windows, and on Linux the desktop's file manager
through the freedesktop ShowItems interface, falling back to opening the containing folder. A
desktop without either still exports normally; only the reveal is reported as failed.

## Output behavior

Exports can retain the crop's source dimensions or resize to configured output dimensions. Naming templates support source and crop information, and collision resolution creates a new filename instead of replacing an existing export. Completed crops remain visible as indexed history overlays during the current source session.
