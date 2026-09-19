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
    // https://primer.style — open-emphasis / closed-emphasis (light)
    state_open_bg: Color32::from_rgb(0x1f, 0x88, 0x3d),
    state_open_fg: Color32::WHITE,
    state_closed_bg: Color32::from_rgb(0xcf, 0x22, 0x2e),
    state_closed_fg: Color32::WHITE,
    chip_bg: Color32::from_rgb(0xf6, 0xf8, 0xfa),
    chip_fg: Color32::from_rgb(0x65, 0x6d, 0x76),
    selection_inactive_bg: Color32::from_rgb(0xd0, 0xd7, 0xde),
};
