use eframe::egui;

egui_phosphor::subset! {
    pub(super) mod phosphor {
        use regular::{LINK, LINK_BREAK};
    }
}

pub(super) use phosphor::regular::{LINK, LINK_BREAK};

pub(super) fn install(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    phosphor::regular::add_to_fonts(&mut fonts);
    context.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renders_chain_glyphs(context: &egui::Context) -> bool {
        let mut output = context.run_ui(egui::RawInput::default(), |_ui| {});
        output.textures_delta.clear();
        context.fonts_mut(|fonts| {
            let font = egui::FontId::proportional(14.0);
            fonts.has_glyphs(&font, LINK) && fonts.has_glyphs(&font, LINK_BREAK)
        })
    }

    #[test]
    fn chain_glyphs_render_in_ordinary_text_only_after_installation() {
        let bare = egui::Context::default();
        let installed = egui::Context::default();

        install(&installed);

        assert!(!renders_chain_glyphs(&bare));
        assert!(renders_chain_glyphs(&installed));
    }
}
