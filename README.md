# CropDeck

**Fast native tool to slice massive images into strictly locked aspect ratio frames. Offline, keyboard driven, and built with Rust.**

CropDeck is a desktop application for crop extraction. It keeps crop geometry in source image pixels, renders only nearby sections of very tall sources as bounded GPU tiles, and performs decoding and encoding on background workers so the interface remains responsive.

## Highlights

- Locked catalog and custom aspect ratios with exact generation oriented size presets.
- Native resolution display that scales down only when the source exceeds the available width.
- Keyboard and pointer workflows with edge scrolling, viewport zoom, and magnetic size snapping.
- Naturally sorted image queues, recursive folder scanning, recent sources, crop history, and capture-and-advance.
- Docked format bar with numbered ratio chips and a size rail that never interrupts the canvas.
- Background image loading with neighbor prefetch and a configurable decoded-source cache.
- Visible-tile GPU residency with bounded per-frame uploads and distant-tile release.
- Half-resolution display copy for zoomed-out views of very wide sources.
- Non-blocking PNG, JPEG, and lossy WebP export with collision-safe filenames.
- Local configuration and fully offline operation with no accounts or telemetry.

## Supported formats

| Capability | Formats |
| --- | --- |
| Input | JPEG, PNG, WebP |
| Output | PNG, JPEG, lossy WebP |

## Build and run

CropDeck requires Rust 1.95 or newer. From a repository checkout, run:

```shell
cargo run --release
```

Release profile uses full link-time optimization. Native application uses the eframe `glow` renderer and targets Windows x64, Linux x64, and Linux ARM64.

## Basic operation

Open an image or folder, choose a ratio and size, position the crop, and press Space or Enter to
capture. The complete control reference, detailed workflows, and practical examples are in the
[workflow guide](docs/workflows.md).

The Performance section in Settings controls the decoded image cache from 128 MiB to 16 GiB. Its
default is 1 GiB.

## Documentation

| Document | Purpose |
| --- | --- |
| [Workflow guide](docs/workflows.md) | Use cases, operating instructions, controls, and examples |
| [Product direction](docs/product.md) | Product principles, boundaries, and current capabilities |
| [Development guide](docs/development.md) | Architecture, invariants, and agent workflow |
| [Roadmap](docs/roadmap.md) | Planned work that is not yet implemented |
| [Implementation decisions](docs/implementation.md) | Dependency choices, design rationale, and performance characteristics |

## Development

Read [AGENTS.md](AGENTS.md) and the [development guide](docs/development.md) before contributing.
The required local checks are:

```shell
cargo fmt --check
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

## License

CropDeck is licensed under either the MIT License or the Apache License 2.0, at your option.
