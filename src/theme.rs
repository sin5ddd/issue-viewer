use eframe::egui::Color32;

/// GitHub Primer light issue colors. Swap this struct when themes land.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub state_open_bg: Color32,
    pub state_open_fg: Color32,
    pub state_closed_bg: Color32,
    pub state_closed_fg: Color32,
    pub chip_bg: Color32,
    pub chip_fg: Color32,
    pub selection_inactive_bg: Color32,
}

pub const THEME: Theme = Theme {
    // Primer success/danger subtle — pale fill, colored text.
    state_open_bg: Color32::from_rgb(0xda, 0xfb, 0xe1),
    state_open_fg: Color32::from_rgb(0x1a, 0x7f, 0x37),
    state_closed_bg: Color32::from_rgb(0xff, 0xeb, 0xe9),
    state_closed_fg: Color32::from_rgb(0xcf, 0x22, 0x2e),
    chip_bg: Color32::from_rgb(0xf6, 0xf8, 0xfa),
    chip_fg: Color32::from_rgb(0x65, 0x6d, 0x76),
    selection_inactive_bg: Color32::from_rgb(0xd0, 0xd7, 0xde),
};

/// Foreground, background. Label and type colors are washed toward white.
pub fn chip_colors(raw: &str) -> (Color32, Color32) {
    base_color(raw)
        .map(wash)
        .unwrap_or((THEME.chip_fg, THEME.chip_bg))
}

fn base_color(raw: &str) -> Option<Color32> {
    let key = raw.trim().trim_start_matches('#');
    if key.len() == 6 && key.chars().all(|c| c.is_ascii_hexdigit()) {
        let n = u32::from_str_radix(key, 16).ok()?;
        return Some(Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8));
    }
    Some(match key.to_ascii_lowercase().as_str() {
        "gray" | "grey" => Color32::from_rgb(0x6e, 0x77, 0x81),
        "blue" => Color32::from_rgb(0x09, 0x69, 0xda),
        "green" => Color32::from_rgb(0x1a, 0x7f, 0x37),
        "yellow" => Color32::from_rgb(0x9a, 0x67, 0x00),
        "orange" => Color32::from_rgb(0xbc, 0x4c, 0x00),
        "red" => Color32::from_rgb(0xcf, 0x22, 0x2e),
        "pink" => Color32::from_rgb(0xbf, 0x39, 0x89),
        "purple" => Color32::from_rgb(0x82, 0x50, 0xdf),
        _ => return None,
    })
}

/// Pale fill (about one third of the hue) and darker ink of the same hue.
fn wash(color: Color32) -> (Color32, Color32) {
    let bg = mix(color, Color32::WHITE, 0.34);
    let mut fg = mix(color, Color32::from_rgb(0x1f, 0x23, 0x28), 0.62);
    for _ in 0..4 {
        if luminance(fg) + 48 < luminance(bg) {
            break;
        }
        fg = mix(fg, Color32::from_rgb(0x1f, 0x23, 0x28), 0.7);
    }
    (fg, bg)
}

fn mix(a: Color32, b: Color32, weight_a: f32) -> Color32 {
    let t = weight_a.clamp(0.0, 1.0);
    let u = 1.0 - t;
    Color32::from_rgb(
        (f32::from(a.r()) * t + f32::from(b.r()) * u).round() as u8,
        (f32::from(a.g()) * t + f32::from(b.g()) * u).round() as u8,
        (f32::from(a.b()) * t + f32::from(b.b()) * u).round() as u8,
    )
}

fn luminance(c: Color32) -> u32 {
    (u32::from(c.r()) * 299 + u32::from(c.g()) * 587 + u32::from(c.b()) * 114) / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chips_are_pale_and_readable() {
        let (fg, bg) = chip_colors("d73a4a");
        assert!(luminance(bg) > luminance(Color32::from_rgb(0xd7, 0x3a, 0x4a)));
        assert!(luminance(fg) + 48 < luminance(bg));
        let (fg, bg) = chip_colors("BLUE");
        assert!(luminance(bg) > luminance(Color32::from_rgb(0x09, 0x69, 0xda)));
        assert!(luminance(fg) < luminance(bg));
        let (fg, bg) = chip_colors("#000000");
        assert!(luminance(bg) > 160);
        assert!(luminance(fg) < luminance(bg));
        assert_eq!(chip_colors("nope"), (THEME.chip_fg, THEME.chip_bg));
    }

    #[test]
    fn state_pills_are_pale() {
        assert!(luminance(THEME.state_open_bg) > luminance(THEME.state_open_fg));
        assert!(luminance(THEME.state_closed_bg) > luminance(THEME.state_closed_fg));
        assert!(luminance(THEME.state_open_bg) > 180);
        assert!(luminance(THEME.state_closed_bg) > 180);
    }
}
