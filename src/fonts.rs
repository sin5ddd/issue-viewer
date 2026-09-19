use eframe::egui::{FontData, FontDefinitions, FontFamily};

pub fn install(ctx: &eframe::egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "noto_jp".into(),
        FontData::from_static(include_bytes!("../assets/fonts/NotoSansJP-Regular.ttf")).into(),
    );
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "noto_jp".into());
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_font_is_nonempty() {
        let bytes = include_bytes!("../assets/fonts/NotoSansJP-Regular.ttf");
        assert!(bytes.len() > 1000);
    }
}
