use anyhow::{Context, Result};
use cropdeck::app::CropDeckApp;

fn main() -> Result<()> {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "CropDeck",
        native_options,
        Box::new(|creation_context| Ok(Box::new(CropDeckApp::new(creation_context)))),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))
    .context("CropDeck native window failed")
}
