# CropDeck Product Direction

## Purpose

CropDeck is a native, offline crop extraction workstation for tall sequential images such as comics, webnovels, storyboards, contact sheets, and concept sheets. Its primary measure of success is the time and number of interactions required to produce a useful crop.

The core workflow is to open a source or folder, position a ratio constrained crop, capture it, advance through the source, and repeat. Features should make that loop faster without turning the application into a general purpose image editor.

## Current product

CropDeck accepts JPEG, PNG, and WebP sources and exports PNG, JPEG, or lossy WebP. Folder sources are naturally sorted and can be scanned recursively. Images open at native resolution and scale down only when wider than the viewport. Only the tiles near the viewport are resident on the GPU, so source height does not limit what can be opened.

The workspace keeps crop geometry in source pixels. Users can move the crop with the keyboard or pointer, trigger progressive edge scrolling while dragging, pan the document, zoom the viewport, and capture without blocking the interface. Completed crops remain visible as indexed history overlays.

The format picker presents every catalog ratio by orientation and uses a discrete XS through XXL resolution rail. Catalog labels are nominal ratios, while each size tier carries authoritative pixel dimensions aligned for generation workflows. Custom ratios use the same tier system.

Exports use a configurable tokenized name, configurable output size and quality, and create new file semantics. Existing files are never overwritten silently. Settings and the most recent source are persisted locally without network access.

Two explicit output actions sit beside the export: the current crop can be copied to the system clipboard, and the last completed export can be revealed in the platform file manager. Both are optional, are isolated behind small platform interfaces, and report failure without affecting export behavior.

## Interaction principles

The source image should dominate the window, and nothing that is adjusted repeatedly while looking at it may cover it or close on its own. The top bar is a permanent format bar: the File menu, numbered quick ratio chips, a catalog picker, the size rail, the current crop dimensions, custom ratio fields, and Capture. It stays in place with no image open so the ratio can be chosen before opening a folder and the layout never reflows. Controls that cannot act in the current state are disabled rather than hidden. The footer is a read-only status bar with contextual queue navigation, viewport sizing, crop position, capture count, and the latest message.

Every surface has one dismissal contract. Docked surfaces never close. Menus and the catalog picker close on a pick, a click outside, or Esc. Settings and About are modal dialogs that dim the canvas and close on Esc or Close. Workspace shortcuts are suppressed only while a text field has focus or a modal is open; a shortcut pressed while a menu is open closes the menu and then runs.

Normal scrolling navigates the document. Shift with the wheel resizes the crop, while Ctrl with the wheel zooms the viewport. Magnetic size snapping should accelerate entry into common generation sizes and require deliberate movement to leave them. Keyboard and pointer paths must remain equally capable.

Every command exposed through a keyboard shortcut must also have a compact pointer accessible action unless direct manipulation already provides the same exact operation.

## Product boundaries

CropDeck is not a painting, compositing, asset management, generation, or video editing tool. The core should not acquire layers, brushes, typography, masks, generative fill, model runtimes, cloud accounts, mandatory telemetry, or an embedded generation interface.

Image cleanup and downstream processing should remain optional and modular. A future external processing contract is preferable to embedding large model runtimes or recreating established image processing suites.

## Supported platforms

Windows x64, Linux x64, and Linux ARM64 are project targets. Windows is the local development platform. Linux x64 and Linux ARM64 are built by native GitHub Actions jobs and require CI or real device evidence before platform specific work is considered complete.