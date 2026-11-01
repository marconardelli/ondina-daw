//! Design tokens ("Graphite Tide") and the egui visual style built from them.
//!
//! Graphite neutrals in layers, a single water-blue accent for selection and focus, state colors that never
//! overlap with the twelve track colors. The older constant names (`BG`, `PANEL`, `LANE`, `TEXT_DIM`, `ACCENT`, ...)
//! stay as aliases of the new tokens so the views keep compiling while they are redesigned one by one.

// The whole token set is defined up front; the views adopt the remaining tokens in later steps of the redesign.
#![allow(dead_code)]

use eframe::egui::{self, Color32, FontFamily, FontId, Rounding, Stroke, TextStyle};

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

// ---- surfaces, darkest to lightest ----
/// Window ground, behind the arrange area and the editors.
pub const GROUND: Color32 = hex(0x121417);
/// Track lanes and long rows.
pub const LANE_BG: Color32 = hex(0x15171A);
/// Panels: toolbar, inspector, mixer, dialogs.
pub const PANEL_BG: Color32 = hex(0x1A1D21);
/// Raised elements on a panel: buttons at rest, cards.
pub const RAISED: Color32 = hex(0x202328);
/// Controls inside a raised element: effect slots, knob bodies.
pub const CONTROL: Color32 = hex(0x23272D);
/// Hairline borders of controls and panels.
pub const BORDER: Color32 = hex(0x2A2E34);
/// Border of a hovered control.
pub const BORDER_HOVER: Color32 = hex(0x3A4048);
/// Dark dividers between panels.
pub const SEPARATOR: Color32 = hex(0x0C0D0F);
/// Background of the position display and of meters and fader tracks.
pub const LCD: Color32 = hex(0x0C0E10);
/// Sunken wells: segmented controls, text fields.
pub const INSET: Color32 = hex(0x0F1113);
/// Fill of a hovered button.
pub const HOVER: Color32 = hex(0x2A2E35);
/// The selected segment of a segmented control.
pub const SEGMENT_ON: Color32 = hex(0x2C3138);
/// A selected track header or mixer strip.
pub const SELECTED_BG: Color32 = hex(0x252B33);
/// The lane of the selected track.
pub const SELECTED_LANE: Color32 = hex(0x20252C);
/// The unlit part of a knob arc, dashed borders of empty slots.
pub const TRACK_OFF: Color32 = hex(0x343941);

// ---- text ----
pub const TEXT: Color32 = hex(0xE6E8EB);
pub const TEXT_2: Color32 = hex(0xA3A9B3);
pub const TEXT_3: Color32 = hex(0x858C97);
pub const TEXT_DISABLED: Color32 = hex(0x6E7580);
/// Icons and soft labels on panels.
pub const ICON: Color32 = hex(0xC9CDD3);

// ---- accent ----
pub const ACCENT: Color32 = hex(0x4CC2D6);
pub const ACCENT_HOVER: Color32 = hex(0x7FD6E4);
/// Background of an active (toggled-on) control.
pub const ACCENT_BG: Color32 = hex(0x1D3338);
/// Border of an active control.
pub const ACCENT_BORDER: Color32 = hex(0x2F5961);
/// Text on the LCD and of an active control.
pub const ACCENT_TEXT: Color32 = hex(0x7FDCEA);
/// Text on a filled accent (primary) button.
pub const ON_ACCENT: Color32 = hex(0x0B2328);

// ---- states ----
pub const PLAY: Color32 = hex(0x5FD39A);
pub const REC: Color32 = hex(0xE5484D);
pub const SOLO: Color32 = hex(0xF2C14E);
pub const MUTE: Color32 = hex(0x6F86F0);
pub const MONITOR: Color32 = hex(0xF29D4E);
pub const AI: Color32 = hex(0xA78BFA);
pub const PLAYHEAD: Color32 = hex(0xF4F5F7);
/// Text drawn on a lit state button, one per state.
pub const ON_MUTE: Color32 = hex(0x0E1430);
pub const ON_SOLO: Color32 = hex(0x2A2006);
pub const ON_REC: Color32 = hex(0xFFFFFF);
pub const ON_MONITOR: Color32 = hex(0x2A1606);
/// Lower end of the meter gradient.
pub const METER_LOW: Color32 = hex(0x3FAF7A);

/// The twelve colors a user can give a track (Tokens: "colori traccia").
pub const TRACK_COLORS: [Color32; 12] = [
    hex(0x4A90D9),
    hex(0x4CAF7D),
    hex(0xD9A441),
    hex(0xE0705F),
    hex(0x9A7BD9),
    hex(0x3FB0A8),
    hex(0xD46BA3),
    hex(0xC98B5A),
    hex(0x7FA650),
    hex(0x5F7FD9),
    hex(0xB8B8B8),
    hex(0x8C6BD9),
];
/// Text on a track-colored header.
pub const ON_TRACK: Color32 = hex(0x0E1013);

// ---- legacy names, mapped onto the tokens ----
pub const BG: Color32 = GROUND;
pub const PANEL: Color32 = PANEL_BG;
/// Lighter rows and "off" fills drawn by hand.
pub const LANE: Color32 = hex(0x1C1F23);
/// Darker alternate rows.
pub const LANE_ALT: Color32 = hex(0x16181C);
pub const GRID: Color32 = hex(0x22262B);
pub const GRID_BAR: Color32 = TRACK_OFF;
pub const TEXT_DIM: Color32 = TEXT_2;
/// Loop/cycle band over the ruler (accent, translucent).
pub const CYCLE: Color32 = Color32::from_rgba_premultiplied(23, 58, 64, 150);
/// Outline of selected regions, notes and points: near white so it reads on every track color.
pub const SELECT: Color32 = PLAYHEAD;

// ---- spacing (px) ----
pub const SP_1: f32 = 2.0;
pub const SP_2: f32 = 4.0;
pub const SP_3: f32 = 6.0;
pub const SP_4: f32 = 8.0;
pub const SP_5: f32 = 12.0;
pub const SP_6: f32 = 16.0;

// ---- corner radii (px) ----
/// State buttons, effect slots, small chips.
pub const R_SM: f32 = 4.0;
/// Segments, regions, menu entries.
pub const R_MD: f32 = 5.0;
/// Buttons and fields.
pub const R_LG: f32 = 6.0;
/// Panels, cards, windows.
pub const R_XL: f32 = 8.0;

// ---- type scale ----
pub const SIZE_TITLE: f32 = 14.0;
pub const SIZE_BODY: f32 = 12.0;
pub const SIZE_LABEL: f32 = 10.0;
pub const SIZE_NUMBER: f32 = 11.0;
pub const SIZE_LCD: f32 = 18.0;

const INTER: &str = "Inter";
const INTER_SEMIBOLD: &str = "Inter-SemiBold";
const MONO: &str = "JetBrainsMono";

/// The semibold UI family (titles, caps labels, state buttons).
pub fn semibold() -> FontFamily {
    FontFamily::Name(INTER_SEMIBOLD.into())
}

/// Title 14, semibold.
pub fn title_font() -> FontId {
    FontId::new(SIZE_TITLE, semibold())
}

/// Body 12.
pub fn body_font() -> FontId {
    FontId::proportional(SIZE_BODY)
}

/// Caps label 10, semibold: write the text in capitals (see [`label_text`]).
pub fn label_font() -> FontId {
    FontId::new(SIZE_LABEL, semibold())
}

/// Numbers 11, monospace.
pub fn number_font() -> FontId {
    FontId::monospace(SIZE_NUMBER)
}

/// LCD 18, monospace.
pub fn lcd_font() -> FontId {
    FontId::monospace(SIZE_LCD)
}

/// A section label in the label style: capitals, 10 px semibold, tertiary text.
pub fn label_text(s: &str) -> egui::RichText {
    egui::RichText::new(s.to_uppercase())
        .font(label_font())
        .color(TEXT_3)
}

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn darker(c: Color32, f: f32) -> Color32 {
    Color32::from_rgb(
        (c.r() as f32 * f) as u8,
        (c.g() as f32 * f) as u8,
        (c.b() as f32 * f) as u8,
    )
}

/// Inter for the UI and JetBrains Mono for numbers, both embedded; egui's own fonts and the Windows symbol/emoji
/// fonts stay behind them only as fallbacks for glyphs the bundled fonts lack.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (INTER, &include_bytes!("../assets/fonts/Inter-Regular.ttf")[..]),
        (INTER_SEMIBOLD, &include_bytes!("../assets/fonts/Inter-SemiBold.ttf")[..]),
        (MONO, &include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf")[..]),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), egui::FontData::from_static(bytes));
    }
    let egui_fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut system = Vec::new();
    let dir = std::env::var("WINDIR")
        .map(|w| std::path::PathBuf::from(w).join("Fonts"))
        .unwrap_or_default();
    for (name, file) in [
        ("segoe_symbol", "seguisym.ttf"),
        ("segoe_emoji", "seguiemj.ttf"),
    ] {
        if let Ok(bytes) = std::fs::read(dir.join(file)) {
            fonts
                .font_data
                .insert(name.to_owned(), egui::FontData::from_owned(bytes));
            system.push(name.to_owned());
        }
    }
    let chain = |first: &str| {
        let mut v = vec![first.to_owned()];
        v.extend(egui_fallbacks.iter().cloned());
        v.extend(system.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, chain(INTER));
    fonts.families.insert(semibold(), chain(INTER_SEMIBOLD));
    let mut mono = vec![MONO.to_owned()];
    mono.extend(chain(INTER));
    fonts.families.insert(FontFamily::Monospace, mono);
    ctx.set_fonts(fonts);
}

fn widget(bg: Color32, border: Color32, fg: Color32) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke: Stroke::new(1.0_f32, border),
        rounding: Rounding::same(R_MD),
        fg_stroke: Stroke::new(1.0_f32, fg),
        expansion: 0.0,
    }
}

/// The egui visuals of the theme. Hover only lightens the neutral; the accent tint is reserved for pressed and
/// selected (toggled-on) widgets, so the two states never look the same.
pub fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL_BG;
    v.window_fill = PANEL_BG;
    v.faint_bg_color = LANE_BG;
    v.extreme_bg_color = INSET;
    v.code_bg_color = LCD;
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = MONITOR;
    v.error_fg_color = REC;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.window_rounding = Rounding::same(R_XL);
    v.menu_rounding = Rounding::same(R_LG);
    v.window_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 6.0),
        blur: 18.0,
        spread: 0.0,
        color: Color32::from_black_alpha(110),
    };
    v.popup_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 4.0),
        blur: 12.0,
        spread: 0.0,
        color: Color32::from_black_alpha(100),
    };
    v.selection.bg_fill = ACCENT_BG;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT_TEXT);
    v.text_cursor.stroke = Stroke::new(2.0_f32, ACCENT);
    v.widgets.noninteractive = widget(PANEL_BG, BORDER, ICON);
    v.widgets.inactive = widget(RAISED, BORDER, TEXT);
    // `bg_fill` of a resting widget is the slider rail and the checkbox well: one step lighter than the button
    // fill (`weak_bg_fill`) so rails stay visible on lanes and strips.
    v.widgets.inactive.bg_fill = TRACK_OFF;
    v.widgets.hovered = widget(HOVER, BORDER_HOVER, PLAYHEAD);
    v.widgets.hovered.bg_fill = BORDER_HOVER;
    v.widgets.active = widget(ACCENT_BG, ACCENT_BORDER, TEXT);
    v.widgets.open = widget(CONTROL, BORDER_HOVER, TEXT);
    v
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_visuals(visuals());
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(SIZE_LABEL)),
        (TextStyle::Body, body_font()),
        (TextStyle::Button, body_font()),
        (TextStyle::Heading, title_font()),
        (TextStyle::Monospace, number_font()),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(SP_3, SP_2);
    style.spacing.button_padding = egui::vec2(7.0, 3.0);
    style.spacing.menu_margin = egui::Margin::same(SP_3);
    style.spacing.window_margin = egui::Margin::same(10.0);
    ctx.set_style(style);
}

pub fn velocity_color(base: Color32, velocity: u8) -> Color32 {
    let f = 0.45 + 0.55 * (velocity as f32 / 127.0);
    Color32::from_rgb(
        (base.r() as f32 * f) as u8,
        (base.g() as f32 * f) as u8,
        (base.b() as f32 * f) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_tokens_decode_to_the_design_values() {
        assert_eq!(GROUND, Color32::from_rgb(0x12, 0x14, 0x17));
        assert_eq!(ACCENT, Color32::from_rgb(0x4C, 0xC2, 0xD6));
        assert_eq!(TRACK_COLORS[11], Color32::from_rgb(0x8C, 0x6B, 0xD9));
    }

    #[test]
    fn hover_and_selected_widgets_never_look_the_same() {
        let v = visuals();
        let hover = v.widgets.hovered;
        assert_ne!(hover.bg_fill, v.selection.bg_fill);
        assert_ne!(hover.bg_fill, v.widgets.active.bg_fill);
        assert_ne!(hover.bg_stroke.color, v.widgets.active.bg_stroke.color);
        // hover lightens the resting neutral
        let luma = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
        assert!(luma(hover.weak_bg_fill) > luma(v.widgets.inactive.weak_bg_fill));
    }

    #[test]
    fn state_colors_stay_apart_from_the_track_palette() {
        for s in [ACCENT, PLAY, REC, SOLO, MUTE, MONITOR, AI] {
            assert!(!TRACK_COLORS.contains(&s), "{s:?}");
        }
    }

    #[test]
    fn surfaces_get_lighter_layer_by_layer() {
        let luma = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
        let layers = [SEPARATOR, GROUND, LANE_BG, PANEL_BG, RAISED, CONTROL, BORDER];
        for w in layers.windows(2) {
            assert!(luma(w[0]) < luma(w[1]), "{:?} vs {:?}", w[0], w[1]);
        }
    }
}
