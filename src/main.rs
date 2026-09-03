#![windows_subsystem = "windows"]

use anyhow::{Context, Result};
use cropdeck::app::CropDeckApp;
use eframe::egui::{IconData, ViewportBuilder};

const APP_ID: &str = "cropdeck";
const WINDOW_ICON_PNG: &[u8] =
    include_bytes!("../assets/linux/icons/hicolor/256x256/apps/cropdeck.png");

fn window_icon() -> Result<IconData> {
    eframe::icon_data::from_png_bytes(WINDOW_ICON_PNG)
        .context("embedded window icon is not a valid PNG")
}

fn main() -> Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_icon(window_icon()?),
        ..Default::default()
    };
    eframe::run_native(
        "CropDeck",
        native_options,
        Box::new(|creation_context| Ok(Box::new(CropDeckApp::new(creation_context)))),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))
    .context("CropDeck native window failed")
}
