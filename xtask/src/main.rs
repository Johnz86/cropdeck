mod screenshot;
#[cfg(unix)]
mod x11_capture;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "CropDeck developer tasks")]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    #[command(about = "Capture one application window and verify that it is the window")]
    Screenshot(screenshot::ScreenshotArgs),
}

fn main() -> ExitCode {
    match Cli::parse().task {
        Task::Screenshot(arguments) => screenshot::run(&arguments),
    }
}
