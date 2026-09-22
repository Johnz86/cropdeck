# CropDeck Roadmap

## Status

CropDeck 0.1 implements the complete core loop: source discovery, natural queue navigation, source coordinate cropping, ratio and generation size selection, mouse and keyboard operation, capture and advance, background export, history overlays, naming, collision safety, and persistent settings.

The items below are intentionally not implemented. Each should be delivered independently and must preserve the product boundaries in product.md.

## Near term workflow improvements

### Drag and drop

The application currently opens sources through native dialogs or the recent source command. A future change can accept one image, multiple images, or one directory through native file drop events. Queue construction must reuse ImageQueue, preserve natural sorting, reject mixed unsupported inputs clearly, and include unit tested path classification separate from egui event handling.

### Session restoration

AppConfig remembers the ten most recent sources but not the queue index, crop, scroll position, or completed history. A resumable session should use a versioned data model, validate source identity and bounds, recover safely from missing files, and avoid embedding image data. Restoration must never overwrite the current settings file with partially serialized state.

### Configurable shortcuts

The current shortcut map is fixed. Remapping requires a typed command model, conflict detection, platform aware modifier display, persistence, reset to default behavior, and a discoverable editor. Text input and modal focus must continue to suppress workspace commands.

### Center crop horizontally

Add an unmodified C command and a compact pointer accessible footer action to center the current crop within the source width. The source coordinate result sets x to half the unused source width. When that width is odd, the extra pixel remains on the right. The command preserves y, crop dimensions, nominal and effective ratios, selected size preference, resize snapping state, history, zoom, and export state. It does nothing when no crop is loaded, while a modal or text field owns keyboard input, or while a pointer drag is active. The result must be brought into view.

Implement the geometry on CropRect in crop.rs and keep shortcut and view orchestration in src/app/shortcuts.rs and src/app/workspace.rs. Test even and odd horizontal slack, unchanged crop properties, source bounds, drag suppression, focus suppression, and pointer and keyboard paths.

### Additional crop placement commands

Still unimplemented placement commands include snapping to source edges, duplicating the previous crop position, and moving by exactly one crop height with configurable overlap. Each is an independent task and must place its source coordinate geometry on CropRect rather than in UI code.

## Output integrations

### Coordinate manifests

A JSON manifest can expose source paths, nominal ratios, effective dimensions, crop coordinates, and resolved output paths for dataset or pipeline use. The format requires a version field, deterministic ordering, create new output behavior, and round trip tests.

### Downstream profiles

Profiles can bind a nominal ratio, crop tier, export dimensions, format, quality, and naming template for a downstream model or video workflow. Profiles should reference catalog data rather than copy dimensions into UI code. Model specific metadata belongs in a versioned catalog extension and must not be inferred from a size alone.

### External post processing

A later opt in hook can pass completed exports to an external command. It must be disabled by default, avoid shell interpolation, expose arguments as separate values, bound concurrency, report failures without losing the crop receipt, and require explicit configuration. Embedded model runtimes remain outside the core.

## Optional format work

AVIF support and metadata preservation policies remain deferred. They should be feature gated if they introduce native dependencies or materially increase the binary. Metadata stripping remains the predictable default for generation inputs. 

## Explicit non goals

Painting, layers, text editing, masks, generative fill, embedded image generation, embedded ComfyUI, RAW development, video editing, asset databases, cloud accounts, and mandatory telemetry are not roadmap items. Proposals in these areas require a deliberate change to the product boundary rather than an incremental implementation task.
