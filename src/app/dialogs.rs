use std::time::Instant;

use eframe::egui;

use crate::config::ExportFormat;
use crate::filesystem::DialogKind;

use super::CropDeckApp;
use super::path_field::{PathField, PathFieldState};

const DIALOG_WIDTH: f32 = 460.0;
const PATH_BUTTON_AREA: f32 = 210.0;

fn dialog_footer(ui: &mut egui::Ui) -> bool {
    ui.separator();
    ui.horizontal(|ui| {
        ui.weak("Esc closes");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.button("Close").clicked()
        })
        .inner
    })
    .inner
}

impl CropDeckApp {
    pub(super) fn settings_dialog(&mut self, context: &egui::Context) {
        let modal = egui::Modal::new(egui::Id::new("cropdeck_settings")).show(context, |ui| {
            ui.set_width(DIALOG_WIDTH);
            ui.heading("Settings");
            ui.add_space(4.0);
            self.capture_settings(ui);
            ui.separator();
            self.performance_settings(ui);
            ui.separator();
            self.export_settings(ui);
            dialog_footer(ui)
        });
        if modal.inner || modal.should_close() {
            self.settings_open = false;
        }
    }

    pub(super) fn about_dialog(&mut self, context: &egui::Context) {
        let modal = egui::Modal::new(egui::Id::new("cropdeck_about")).show(context, |ui| {
            ui.set_width(DIALOG_WIDTH);
            ui.heading(format!("CropDeck {}", env!("CARGO_PKG_VERSION")));
            ui.label(env!("CARGO_PKG_DESCRIPTION"));
            ui.separator();
            ui.label("A fast native crop extraction workstation.");
            ui.label("Licensed under MIT.");
            dialog_footer(ui)
        });
        if modal.inner || modal.should_close() {
            self.about_open = false;
        }
    }

    fn capture_settings(&mut self, ui: &mut egui::Ui) {
        ui.strong("Capture");
        let mut advance = self.config.capture_advance_percent();
        if ui
            .add(egui::Slider::new(&mut advance, 1..=100).text("Advance percent"))
            .changed()
            && let Err(error) = self.config.set_capture_advance_percent(advance)
        {
            self.report_error(error.to_string());
        }
        let mut recursive = self.config.recursive_scan();
        if ui
            .checkbox(&mut recursive, "Scan folders recursively")
            .changed()
        {
            self.config.set_recursive_scan(recursive);
        }
    }

    fn performance_settings(&mut self, ui: &mut egui::Ui) {
        ui.strong("Performance");
        let mut cache_budget = self.config.cache_budget_megabytes();
        if ui
            .add(
                egui::Slider::new(&mut cache_budget, 128..=16_384)
                    .text("Decoded image cache (MiB)"),
            )
            .changed()
            && let Err(error) = self.config.set_cache_budget_megabytes(cache_budget)
        {
            self.report_error(error.to_string());
        }
    }

    fn export_settings(&mut self, ui: &mut egui::Ui) {
        ui.strong("Export");
        let mut format = self.config.export().format();
        egui::ComboBox::from_label("Format")
            .selected_text(format.extension().to_uppercase())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut format, ExportFormat::WebP, "WebP");
                ui.selectable_value(&mut format, ExportFormat::Jpeg, "JPEG");
                ui.selectable_value(&mut format, ExportFormat::Png, "PNG");
            });
        self.config.export_mut().set_format(format);
        let mut quality = self.config.export().quality();
        if ui
            .add(egui::Slider::new(&mut quality, 1..=100).text("Quality"))
            .changed()
            && let Err(error) = self.config.export_mut().set_quality(quality)
        {
            self.report_error(error.to_string());
        }
        if ui
            .checkbox(&mut self.preserve_output_size, "Keep original crop pixels")
            .changed()
        {
            let output_size =
                (!self.preserve_output_size).then_some((self.output_width, self.output_height));
            if let Err(error) = self.config.export_mut().set_output_size(output_size) {
                self.report_error(error.to_string());
            }
        }
        ui.add_enabled_ui(!self.preserve_output_size, |ui| {
            ui.horizontal(|ui| {
                let width_changed = ui
                    .add(egui::DragValue::new(&mut self.output_width).range(1..=65_535))
                    .changed();
                ui.label("x");
                let height_changed = ui
                    .add(egui::DragValue::new(&mut self.output_height).range(1..=65_535))
                    .changed();
                if (width_changed || height_changed)
                    && let Err(error) = self
                        .config
                        .export_mut()
                        .set_output_size(Some((self.output_width, self.output_height)))
                {
                    self.report_error(error.to_string());
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Filename");
            if ui
                .text_edit_singleline(&mut self.filename_template)
                .lost_focus()
            {
                self.filename_template_error = self
                    .config
                    .export_mut()
                    .set_filename_template(self.filename_template.clone())
                    .err()
                    .map(|error| error.to_string());
            }
        });
        if let Some(error) = &self.filename_template_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.separator();
        self.source_row(ui);
        ui.add_space(6.0);
        self.destination_row(ui);
    }

    fn source_row(&mut self, ui: &mut egui::Ui) {
        ui.strong("Source");
        let mut open_image = false;
        let mut open_folder = false;
        ui.horizontal(|ui| {
            let response = path_edit(ui, &mut self.source_field, "Paste a folder or image path");
            if response.changed() {
                self.source_field.mark_edited(Instant::now());
            }
            if response.lost_focus() {
                self.source_field.request_commit();
            }
            let idle = self.dialog_in_flight.is_none();
            open_image = ui
                .add_enabled(idle, egui::Button::new("Image..."))
                .on_hover_text("Choose a single image")
                .clicked();
            open_folder = ui
                .add_enabled(idle, egui::Button::new("Folder..."))
                .on_hover_text("Choose a folder of images")
                .clicked();
        });
        path_status(ui, &self.source_field.state());
        if open_image {
            self.open_dialog(DialogKind::SourceImage);
        }
        if open_folder {
            self.open_dialog(DialogKind::SourceFolder);
        }
    }

    fn destination_row(&mut self, ui: &mut egui::Ui) {
        ui.strong("Destination");
        let mut choose = false;
        let mut use_source = false;
        ui.horizontal(|ui| {
            let response = path_edit(
                ui,
                &mut self.destination_field,
                "Paste an export folder path",
            );
            if response.changed() {
                self.destination_field.mark_edited(Instant::now());
            }
            if response.lost_focus() {
                self.destination_field.request_commit();
            }
            choose = ui
                .add_enabled(
                    self.dialog_in_flight.is_none(),
                    egui::Button::new("Choose destination"),
                )
                .clicked();
            use_source = ui
                .button("Use source folder")
                .on_hover_text("Export beside each source image")
                .clicked();
        });
        path_status(ui, &self.destination_field.state());
        if choose {
            self.open_dialog(DialogKind::ExportDestination);
        }
        if use_source {
            self.destination_field.set_committed(None);
            self.config.export_mut().set_destination(None);
            self.capture_plan = None;
        }
    }
}

fn path_edit(ui: &mut egui::Ui, field: &mut PathField, hint: &str) -> egui::Response {
    let width = (ui.available_width() - PATH_BUTTON_AREA).max(120.0);
    ui.add(
        egui::TextEdit::singleline(field.draft_mut())
            .desired_width(width)
            .hint_text(hint),
    )
}

fn path_status(ui: &mut egui::Ui, state: &PathFieldState) {
    let color = if state.is_rejected() {
        ui.visuals().error_fg_color
    } else {
        ui.visuals().weak_text_color()
    };
    ui.colored_label(color, state.note());
}
