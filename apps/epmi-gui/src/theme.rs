//! Theme tokens, KDE system typography detection, and PhotoCraft-inspired
//! color palettes for `epmi-gui` (ported from LumaPrint / PhotoCraft).
//!
//! Includes:
//! - Automatic KDE Plasma (`~/.config/kdeglobals`) font family & point-size detection
//! - Wayland-safe logical pixel scaling (`pt * (96.0 / 72.0)`)
//! - 12 PhotoCraft themes (`BreezeDark`, `BreezeLight`, `Pro`, `ProMedium`,
//!   `ProLightGray`, `ProLight`, `Studio`, `StudioLight`, `AdwaitaDark`,
//!   `Adwaita`, `SolarizedDark`, `Classic`)

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    Style, TextStyle, Vec2, Visuals,
    style::{Selection, WidgetVisuals, Widgets},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeKind {
    /// KDE Breeze Dark (matched to KDE Plasma 6 Breeze Dark palette).
    #[default]
    BreezeDark,
    /// KDE Breeze Light (matched to KDE Plasma 6 Breeze Light palette).
    BreezeLight,
    /// Neutral charcoal dark (`#323232` canvas, `#252525` panels) — PhotoCraft Pro default.
    Pro,
    /// Medium dark gray (`#535353` panels, `#454545` chrome).
    ProMedium,
    /// Light gray studio (`#b8b8b8` panels, `#a6a6a6` chrome).
    ProLightGray,
    /// Neutral light (`#f0f0f0` panels, `#e2e2e2` chrome).
    ProLight,
    /// Cool slate dark with refined blue accent.
    Studio,
    /// Cool slate light with crisp contrast.
    StudioLight,
    /// GNOME Adwaita Dark.
    AdwaitaDark,
    /// GNOME Adwaita Light.
    Adwaita,
    /// Solarized Dark (Ethan Schoonover's precision palette).
    SolarizedDark,
    /// Classic high-contrast dark (`#1a1d24`).
    Classic,
}

impl ThemeKind {
    pub const ALL: &'static [ThemeKind] = &[
        ThemeKind::BreezeDark,
        ThemeKind::BreezeLight,
        ThemeKind::Pro,
        ThemeKind::ProMedium,
        ThemeKind::ProLightGray,
        ThemeKind::ProLight,
        ThemeKind::Studio,
        ThemeKind::StudioLight,
        ThemeKind::AdwaitaDark,
        ThemeKind::Adwaita,
        ThemeKind::SolarizedDark,
        ThemeKind::Classic,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::BreezeDark => "KDE Breeze Dark",
            Self::BreezeLight => "KDE Breeze Light",
            Self::Pro => "Pro Dunkel (Anthrazit)",
            Self::ProMedium => "Pro Mittelgrau",
            Self::ProLightGray => "Pro Hellgrau",
            Self::ProLight => "Pro Hell",
            Self::Studio => "Studio Dunkel (Schiefer)",
            Self::StudioLight => "Studio Hell",
            Self::AdwaitaDark => "GNOME Adwaita Dunkel",
            Self::Adwaita => "GNOME Adwaita Hell",
            Self::SolarizedDark => "Solarized Dunkel",
            Self::Classic => "Klassisch Dunkel",
        }
    }

    #[must_use]
    pub const fn is_dark(self) -> bool {
        match self {
            Self::BreezeDark
            | Self::Pro
            | Self::ProMedium
            | Self::Studio
            | Self::AdwaitaDark
            | Self::SolarizedDark
            | Self::Classic => true,
            Self::BreezeLight | Self::ProLightGray | Self::ProLight | Self::StudioLight | Self::Adwaita => {
                false
            }
        }
    }

    #[must_use]
    pub const fn tokens(self) -> Tokens {
        match self {
            Self::BreezeDark => Tokens::BREEZE_DARK,
            Self::BreezeLight => Tokens::BREEZE_LIGHT,
            Self::Pro => Tokens::PRO,
            Self::ProMedium => Tokens::PRO_MEDIUM,
            Self::ProLightGray => Tokens::PRO_LIGHT_GRAY,
            Self::ProLight => Tokens::PRO_LIGHT,
            Self::Studio => Tokens::STUDIO,
            Self::StudioLight => Tokens::STUDIO_LIGHT,
            Self::AdwaitaDark => Tokens::ADWAITA_DARK,
            Self::Adwaita => Tokens::ADWAITA,
            Self::SolarizedDark => Tokens::SOLARIZED_DARK,
            Self::Classic => Tokens::CLASSIC,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    /// Outer window / menu bar / status bar surface.
    pub chrome: Color32,
    /// Central workspace surround behind tables/cards.
    pub canvas: Color32,
    /// Side dock / panel background.
    pub dock: Color32,
    /// Section card fill inside dock or workspace.
    pub card: Color32,
    /// Subtle border around section cards.
    pub card_border: Color32,
    /// Card header band background.
    pub card_header: Color32,
    /// Input field / combo background.
    pub field: Color32,
    /// Input field border.
    pub field_border: Color32,
    /// Hovered widget background.
    pub hover: Color32,
    /// Active / pressed widget background.
    pub pressed: Color32,
    /// Primary foreground text.
    pub text: Color32,
    /// Secondary / label text.
    pub text_dim: Color32,
    /// Muted / hint / disabled text.
    pub text_faint: Color32,
    /// Primary accent (active tool, primary button, focused ring).
    pub accent: Color32,
    /// Translucent accent fill for selected rows.
    pub accent_soft: Color32,
    /// Foreground text on top of `accent`.
    pub accent_text: Color32,
    /// 1px divider between panels and sections.
    pub separator: Color32,
    /// Destructive / error indicator.
    pub danger: Color32,
    /// Warning indicator.
    pub warning: Color32,
    /// Success / ready indicator.
    pub success: Color32,
    /// Small corner radius (inputs, buttons).
    pub radius_sm: u8,
    /// Medium corner radius (cards, popups).
    pub radius: u8,
    /// Large corner radius (dialogs).
    pub radius_lg: u8,
}

impl Tokens {
    /// KDE Plasma 6 Breeze Dark palette (`#202326` window, `#292c30` view, `#3daee9` accent).
    pub const BREEZE_DARK: Self = Self {
        chrome: Color32::from_rgb(0x1b, 0x1e, 0x20),
        canvas: Color32::from_rgb(0x16, 0x19, 0x1b),
        dock: Color32::from_rgb(0x20, 0x23, 0x26),
        card: Color32::from_rgb(0x28, 0x2c, 0x30),
        card_border: Color32::from_rgb(0x3b, 0x40, 0x46),
        card_header: Color32::from_rgb(0x2e, 0x33, 0x38),
        field: Color32::from_rgb(0x18, 0x1b, 0x1d),
        field_border: Color32::from_rgb(0x43, 0x49, 0x50),
        hover: Color32::from_rgb(0x32, 0x37, 0x3d),
        pressed: Color32::from_rgb(0x3d, 0xae, 0xe9),
        text: Color32::from_rgb(0xfc, 0xfc, 0xfc),
        text_dim: Color32::from_rgb(0xc4, 0xc8, 0xcc),
        text_faint: Color32::from_rgb(0x7f, 0x8c, 0x8d),
        accent: Color32::from_rgb(0x3d, 0xae, 0xe9),
        accent_soft: Color32::from_rgba_premultiplied(0x14, 0x3a, 0x4e, 0x90),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0x35, 0x3a, 0x40),
        danger: Color32::from_rgb(0xda, 0x44, 0x53),
        warning: Color32::from_rgb(0xf6, 0x74, 0x00),
        success: Color32::from_rgb(0x27, 0xae, 0x60),
        radius_sm: 4,
        radius: 6,
        radius_lg: 8,
    };

    /// KDE Plasma 6 Breeze Light palette (`#eff0f1` window, `#fcfcfc` view, `#3daee9` accent).
    pub const BREEZE_LIGHT: Self = Self {
        chrome: Color32::from_rgb(0xe3, 0xe5, 0xe7),
        canvas: Color32::from_rgb(0xd4, 0xd7, 0xda),
        dock: Color32::from_rgb(0xef, 0xf0, 0xf1),
        card: Color32::from_rgb(0xfc, 0xfc, 0xfc),
        card_border: Color32::from_rgb(0xc8, 0xcc, 0xd0),
        card_header: Color32::from_rgb(0xe8, 0xea, 0xec),
        field: Color32::from_rgb(0xff, 0xff, 0xff),
        field_border: Color32::from_rgb(0xb8, 0xbe, 0xc4),
        hover: Color32::from_rgb(0xdc, 0xe2, 0xe8),
        pressed: Color32::from_rgb(0x3d, 0xae, 0xe9),
        text: Color32::from_rgb(0x23, 0x26, 0x29),
        text_dim: Color32::from_rgb(0x4d, 0x54, 0x5b),
        text_faint: Color32::from_rgb(0x7f, 0x8c, 0x8d),
        accent: Color32::from_rgb(0x29, 0x80, 0xb9),
        accent_soft: Color32::from_rgba_premultiplied(0x3d, 0xae, 0xe9, 0x38),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0xc8, 0xcc, 0xd0),
        danger: Color32::from_rgb(0xda, 0x44, 0x53),
        warning: Color32::from_rgb(0xe6, 0x7e, 0x22),
        success: Color32::from_rgb(0x27, 0xae, 0x60),
        radius_sm: 4,
        radius: 6,
        radius_lg: 8,
    };

    /// PhotoCraft Pro dark neutral palette.
    pub const PRO: Self = Self {
        chrome: Color32::from_rgb(0x1f, 0x1f, 0x1f),
        canvas: Color32::from_rgb(0x18, 0x18, 0x18),
        dock: Color32::from_rgb(0x25, 0x25, 0x25),
        card: Color32::from_rgb(0x2d, 0x2d, 0x2d),
        card_border: Color32::from_rgb(0x3d, 0x3d, 0x3d),
        card_header: Color32::from_rgb(0x34, 0x34, 0x34),
        field: Color32::from_rgb(0x1c, 0x1c, 0x1c),
        field_border: Color32::from_rgb(0x45, 0x45, 0x45),
        hover: Color32::from_rgb(0x38, 0x38, 0x38),
        pressed: Color32::from_rgb(0x14, 0x73, 0xe6),
        text: Color32::from_rgb(0xf0, 0xf0, 0xf0),
        text_dim: Color32::from_rgb(0xb8, 0xb8, 0xb8),
        text_faint: Color32::from_rgb(0x78, 0x78, 0x78),
        accent: Color32::from_rgb(0x26, 0x80, 0xeb),
        accent_soft: Color32::from_rgba_premultiplied(0x10, 0x38, 0x68, 0x88),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0x38, 0x38, 0x38),
        danger: Color32::from_rgb(0xe3, 0x48, 0x50),
        warning: Color32::from_rgb(0xe6, 0x86, 0x19),
        success: Color32::from_rgb(0x2d, 0x9d, 0x78),
        radius_sm: 3,
        radius: 5,
        radius_lg: 7,
    };

    /// PhotoCraft Pro Medium Gray.
    pub const PRO_MEDIUM: Self = Self {
        chrome: Color32::from_rgb(0x42, 0x42, 0x42),
        canvas: Color32::from_rgb(0x33, 0x33, 0x33),
        dock: Color32::from_rgb(0x4d, 0x4d, 0x4d),
        card: Color32::from_rgb(0x56, 0x56, 0x56),
        card_border: Color32::from_rgb(0x68, 0x68, 0x68),
        card_header: Color32::from_rgb(0x5e, 0x5e, 0x5e),
        field: Color32::from_rgb(0x3c, 0x3c, 0x3c),
        field_border: Color32::from_rgb(0x6e, 0x6e, 0x6e),
        hover: Color32::from_rgb(0x63, 0x63, 0x63),
        pressed: Color32::from_rgb(0x14, 0x73, 0xe6),
        text: Color32::from_rgb(0xf5, 0xf5, 0xf5),
        text_dim: Color32::from_rgb(0xd4, 0xd4, 0xd4),
        text_faint: Color32::from_rgb(0x9a, 0x9a, 0x9a),
        accent: Color32::from_rgb(0x37, 0x8e, 0xf0),
        accent_soft: Color32::from_rgba_premultiplied(0x14, 0x42, 0x78, 0x88),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0x3c, 0x3c, 0x3c),
        danger: Color32::from_rgb(0xe3, 0x48, 0x50),
        warning: Color32::from_rgb(0xe6, 0x86, 0x19),
        success: Color32::from_rgb(0x2d, 0x9d, 0x78),
        radius_sm: 3,
        radius: 5,
        radius_lg: 7,
    };

    /// PhotoCraft Pro Light Gray.
    pub const PRO_LIGHT_GRAY: Self = Self {
        chrome: Color32::from_rgb(0xa6, 0xa6, 0xa6),
        canvas: Color32::from_rgb(0x88, 0x88, 0x88),
        dock: Color32::from_rgb(0xb8, 0xb8, 0xb8),
        card: Color32::from_rgb(0xc6, 0xc6, 0xc6),
        card_border: Color32::from_rgb(0x98, 0x98, 0x98),
        card_header: Color32::from_rgb(0xbe, 0xbe, 0xbe),
        field: Color32::from_rgb(0xd8, 0xd8, 0xd8),
        field_border: Color32::from_rgb(0x8e, 0x8e, 0x8e),
        hover: Color32::from_rgb(0xce, 0xce, 0xce),
        pressed: Color32::from_rgb(0x14, 0x73, 0xe6),
        text: Color32::from_rgb(0x1e, 0x1e, 0x1e),
        text_dim: Color32::from_rgb(0x3a, 0x3a, 0x3a),
        text_faint: Color32::from_rgb(0x5c, 0x5c, 0x5c),
        accent: Color32::from_rgb(0x0d, 0x66, 0xd0),
        accent_soft: Color32::from_rgba_premultiplied(0x14, 0x73, 0xe6, 0x35),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0x96, 0x96, 0x96),
        danger: Color32::from_rgb(0xc9, 0x25, 0x2d),
        warning: Color32::from_rgb(0xcb, 0x6f, 0x10),
        success: Color32::from_rgb(0x12, 0x80, 0x5c),
        radius_sm: 3,
        radius: 5,
        radius_lg: 7,
    };

    /// PhotoCraft Pro Light.
    pub const PRO_LIGHT: Self = Self {
        chrome: Color32::from_rgb(0xe2, 0xe2, 0xe2),
        canvas: Color32::from_rgb(0xc8, 0xc8, 0xc8),
        dock: Color32::from_rgb(0xf0, 0xf0, 0xf0),
        card: Color32::from_rgb(0xf9, 0xf9, 0xf9),
        card_border: Color32::from_rgb(0xd0, 0xd0, 0xd0),
        card_header: Color32::from_rgb(0xea, 0xea, 0xea),
        field: Color32::from_rgb(0xff, 0xff, 0xff),
        field_border: Color32::from_rgb(0xc2, 0xc2, 0xc2),
        hover: Color32::from_rgb(0xe4, 0xe4, 0xe4),
        pressed: Color32::from_rgb(0x14, 0x73, 0xe6),
        text: Color32::from_rgb(0x1c, 0x1c, 0x1c),
        text_dim: Color32::from_rgb(0x4a, 0x4a, 0x4a),
        text_faint: Color32::from_rgb(0x76, 0x76, 0x76),
        accent: Color32::from_rgb(0x0d, 0x66, 0xd0),
        accent_soft: Color32::from_rgba_premultiplied(0x14, 0x73, 0xe6, 0x30),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0xd0, 0xd0, 0xd0),
        danger: Color32::from_rgb(0xd7, 0x37, 0x3f),
        warning: Color32::from_rgb(0xda, 0x7b, 0x11),
        success: Color32::from_rgb(0x12, 0x80, 0x5c),
        radius_sm: 3,
        radius: 5,
        radius_lg: 7,
    };

    /// Studio Slate Dark.
    pub const STUDIO: Self = Self {
        chrome: Color32::from_rgb(0x14, 0x17, 0x1d),
        canvas: Color32::from_rgb(0x10, 0x12, 0x17),
        dock: Color32::from_rgb(0x19, 0x1d, 0x24),
        card: Color32::from_rgb(0x21, 0x26, 0x30),
        card_border: Color32::from_rgb(0x31, 0x38, 0x46),
        card_header: Color32::from_rgb(0x26, 0x2c, 0x37),
        field: Color32::from_rgb(0x13, 0x16, 0x1c),
        field_border: Color32::from_rgb(0x37, 0x3f, 0x4f),
        hover: Color32::from_rgb(0x2b, 0x31, 0x3e),
        pressed: Color32::from_rgb(0x3b, 0x82, 0xf6),
        text: Color32::from_rgb(0xf1, 0xf5, 0xf9),
        text_dim: Color32::from_rgb(0xb6, 0xc2, 0xd2),
        text_faint: Color32::from_rgb(0x64, 0x74, 0x8b),
        accent: Color32::from_rgb(0x3b, 0x82, 0xf6),
        accent_soft: Color32::from_rgba_premultiplied(0x15, 0x30, 0x5b, 0x90),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0x2a, 0x30, 0x3c),
        danger: Color32::from_rgb(0xef, 0x44, 0x44),
        warning: Color32::from_rgb(0xf5, 0x9e, 0x0b),
        success: Color32::from_rgb(0x10, 0xb9, 0x81),
        radius_sm: 5,
        radius: 7,
        radius_lg: 10,
    };

    /// Studio Slate Light.
    pub const STUDIO_LIGHT: Self = Self {
        chrome: Color32::from_rgb(0xe2, 0xe8, 0xf0),
        canvas: Color32::from_rgb(0xcb, 0xd5, 0xe1),
        dock: Color32::from_rgb(0xf1, 0xf5, 0xf9),
        card: Color32::from_rgb(0xff, 0xff, 0xff),
        card_border: Color32::from_rgb(0xcb, 0xd5, 0xe1),
        card_header: Color32::from_rgb(0xe8, 0xee, 0xf5),
        field: Color32::from_rgb(0xff, 0xff, 0xff),
        field_border: Color32::from_rgb(0xb8, 0xc4, 0xd4),
        hover: Color32::from_rgb(0xe2, 0xe8, 0xf0),
        pressed: Color32::from_rgb(0x25, 0x63, 0xeb),
        text: Color32::from_rgb(0x0f, 0x17, 0x2a),
        text_dim: Color32::from_rgb(0x33, 0x41, 0x55),
        text_faint: Color32::from_rgb(0x64, 0x74, 0x8b),
        accent: Color32::from_rgb(0x25, 0x63, 0xeb),
        accent_soft: Color32::from_rgba_premultiplied(0x25, 0x63, 0xeb, 0x30),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0xcb, 0xd5, 0xe1),
        danger: Color32::from_rgb(0xdc, 0x26, 0x26),
        warning: Color32::from_rgb(0xd9, 0x77, 0x06),
        success: Color32::from_rgb(0x05, 0x96, 0x69),
        radius_sm: 5,
        radius: 7,
        radius_lg: 10,
    };

    /// GNOME Adwaita Dark.
    pub const ADWAITA_DARK: Self = Self {
        chrome: Color32::from_rgb(0x1e, 0x1e, 0x1e),
        canvas: Color32::from_rgb(0x18, 0x18, 0x18),
        dock: Color32::from_rgb(0x24, 0x24, 0x24),
        card: Color32::from_rgb(0x30, 0x30, 0x30),
        card_border: Color32::from_rgb(0x42, 0x42, 0x42),
        card_header: Color32::from_rgb(0x36, 0x36, 0x36),
        field: Color32::from_rgb(0x1d, 0x1d, 0x1d),
        field_border: Color32::from_rgb(0x4a, 0x4a, 0x4a),
        hover: Color32::from_rgb(0x3b, 0x3b, 0x3b),
        pressed: Color32::from_rgb(0x35, 0x84, 0xe4),
        text: Color32::from_rgb(0xff, 0xff, 0xff),
        text_dim: Color32::from_rgb(0xc8, 0xc8, 0xc8),
        text_faint: Color32::from_rgb(0x88, 0x88, 0x88),
        accent: Color32::from_rgb(0x35, 0x84, 0xe4),
        accent_soft: Color32::from_rgba_premultiplied(0x14, 0x32, 0x58, 0x90),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0x38, 0x38, 0x38),
        danger: Color32::from_rgb(0xe0, 0x1b, 0x24),
        warning: Color32::from_rgb(0xff, 0x78, 0x00),
        success: Color32::from_rgb(0x2e, 0xc2, 0x7e),
        radius_sm: 6,
        radius: 8,
        radius_lg: 12,
    };

    /// GNOME Adwaita Light.
    pub const ADWAITA: Self = Self {
        chrome: Color32::from_rgb(0xeb, 0xeb, 0xeb),
        canvas: Color32::from_rgb(0xd8, 0xd8, 0xd8),
        dock: Color32::from_rgb(0xf6, 0xf5, 0xf4),
        card: Color32::from_rgb(0xff, 0xff, 0xff),
        card_border: Color32::from_rgb(0xd5, 0xd3, 0xcf),
        card_header: Color32::from_rgb(0xee, 0xed, 0xeb),
        field: Color32::from_rgb(0xff, 0xff, 0xff),
        field_border: Color32::from_rgb(0xc6, 0xc4, 0xc0),
        hover: Color32::from_rgb(0xe6, 0xe5, 0xe3),
        pressed: Color32::from_rgb(0x1c, 0x71, 0xd8),
        text: Color32::from_rgb(0x24, 0x1f, 0x31),
        text_dim: Color32::from_rgb(0x50, 0x4e, 0x55),
        text_faint: Color32::from_rgb(0x77, 0x76, 0x7b),
        accent: Color32::from_rgb(0x1c, 0x71, 0xd8),
        accent_soft: Color32::from_rgba_premultiplied(0x1c, 0x71, 0xd8, 0x30),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0xd8, 0xd6, 0xd2),
        danger: Color32::from_rgb(0xc0, 0x1c, 0x28),
        warning: Color32::from_rgb(0xe6, 0x61, 0x00),
        success: Color32::from_rgb(0x26, 0xa2, 0x69),
        radius_sm: 6,
        radius: 8,
        radius_lg: 12,
    };

    /// Solarized Dark.
    pub const SOLARIZED_DARK: Self = Self {
        chrome: Color32::from_rgb(0x00, 0x21, 0x2b),
        canvas: Color32::from_rgb(0x00, 0x1b, 0x24),
        dock: Color32::from_rgb(0x00, 0x2b, 0x36),
        card: Color32::from_rgb(0x07, 0x36, 0x42),
        card_border: Color32::from_rgb(0x18, 0x4e, 0x5c),
        card_header: Color32::from_rgb(0x0c, 0x3f, 0x4d),
        field: Color32::from_rgb(0x00, 0x23, 0x2c),
        field_border: Color32::from_rgb(0x28, 0x5c, 0x6a),
        hover: Color32::from_rgb(0x11, 0x45, 0x54),
        pressed: Color32::from_rgb(0x26, 0x8b, 0xd2),
        text: Color32::from_rgb(0xfd, 0xf6, 0xe3),
        text_dim: Color32::from_rgb(0x93, 0xa1, 0xa1),
        text_faint: Color32::from_rgb(0x65, 0x7b, 0x83),
        accent: Color32::from_rgb(0x26, 0x8b, 0xd2),
        accent_soft: Color32::from_rgba_premultiplied(0x0d, 0x38, 0x56, 0x90),
        accent_text: Color32::from_rgb(0xfd, 0xf6, 0xe3),
        separator: Color32::from_rgb(0x14, 0x46, 0x54),
        danger: Color32::from_rgb(0xdc, 0x32, 0x2f),
        warning: Color32::from_rgb(0xcb, 0x4b, 0x16),
        success: Color32::from_rgb(0x85, 0x99, 0x00),
        radius_sm: 4,
        radius: 6,
        radius_lg: 8,
    };

    /// Classic high-contrast dark (`#1a1d24`).
    pub const CLASSIC: Self = Self {
        chrome: Color32::from_rgb(0x16, 0x19, 0x1f),
        canvas: Color32::from_rgb(0x12, 0x14, 0x19),
        dock: Color32::from_rgb(0x1c, 0x20, 0x27),
        card: Color32::from_rgb(0x24, 0x29, 0x33),
        card_border: Color32::from_rgb(0x36, 0x3e, 0x4d),
        card_header: Color32::from_rgb(0x2a, 0x30, 0x3c),
        field: Color32::from_rgb(0x14, 0x17, 0x1d),
        field_border: Color32::from_rgb(0x3c, 0x45, 0x56),
        hover: Color32::from_rgb(0x2f, 0x36, 0x44),
        pressed: Color32::from_rgb(0x28, 0x78, 0xdc),
        text: Color32::from_rgb(0xec, 0xef, 0xf4),
        text_dim: Color32::from_rgb(0xb8, 0xc0, 0xce),
        text_faint: Color32::from_rgb(0x78, 0x82, 0x96),
        accent: Color32::from_rgb(0x28, 0x78, 0xdc),
        accent_soft: Color32::from_rgba_premultiplied(0x12, 0x34, 0x62, 0x90),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0x2e, 0x35, 0x42),
        danger: Color32::from_rgb(0xdc, 0x3c, 0x46),
        warning: Color32::from_rgb(0xe6, 0x8c, 0x1e),
        success: Color32::from_rgb(0x28, 0xb4, 0x64),
        radius_sm: 4,
        radius: 6,
        radius_lg: 8,
    };
}

/// Detected KDE / desktop font settings.
#[derive(Debug, Clone)]
pub struct KdeTypography {
    /// Primary UI font family name (e.g. `"Noto Sans"`).
    pub family: String,
    /// Configured point size from `~/.config/kdeglobals` `[General] font=` (e.g. `10.0`).
    pub point_size: f32,
    /// Optional resolved `.ttf` / `.otf` file path on disk.
    pub font_path: Option<PathBuf>,
}

impl Default for KdeTypography {
    fn default() -> Self {
        Self {
            family: String::from("Noto Sans"),
            point_size: 10.0,
            font_path: None,
        }
    }
}

/// Reads `~/.config/kdeglobals` `[General] font=Family,PointSize,...` and resolves
/// the font file via `fc-match` (with standard fallback paths).
#[must_use]
pub fn detect_kde_typography() -> KdeTypography {
    let mut cfg = KdeTypography::default();

    if let Ok(home) = std::env::var("HOME") {
        let kdeglobals = PathBuf::from(&home).join(".config/kdeglobals");
        if let Ok(text) = std::fs::read_to_string(kdeglobals)
            && let Some((family, pt)) = parse_kdeglobals_font(&text)
        {
            cfg.family = family;
            cfg.point_size = pt;
        }
    }

    cfg.font_path = resolve_system_font_path(&cfg.family);
    cfg
}

fn parse_kdeglobals_font(content: &str) -> Option<(String, f32)> {
    let mut in_general = false;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_general = line.eq_ignore_ascii_case("[General]");
            continue;
        }
        if !in_general {
            continue;
        }
        if let Some(rest) = line.strip_prefix("font=") {
            let mut parts = rest.split(',');
            let family = parts.next()?.trim();
            let pt_str = parts.next()?.trim();
            if !family.is_empty()
                && let Ok(pt) = pt_str.parse::<f32>()
                && (7.0..=28.0).contains(&pt)
            {
                return Some((family.to_string(), pt));
            }
        }
    }
    None
}

fn resolve_system_font_path(family: &str) -> Option<PathBuf> {
    if let Ok(out) = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", family])
        .output()
        && out.status.success()
    {
        let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        if p.is_file() {
            return Some(p);
        }
    }

    for candidate in [
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ] {
        let p = PathBuf::from(candidate);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn resolve_symbol_fallback_fonts() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for candidate in [
        "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
        "/usr/share/fonts/TTF/NotoSansSymbols2-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansSymbols[wght].ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ] {
        let p = PathBuf::from(candidate);
        if p.is_file() {
            out.push(p);
        }
    }
    out
}

/// Loads the user's KDE system font (e.g. `Noto Sans`) plus symbol fallback fonts
/// into `egui` so typography matches native KDE applications and Unicode symbols never render as tofu (`□`).
pub fn install_kde_system_fonts(ctx: &Context) -> KdeTypography {
    let kde = detect_kde_typography();
    let mut fonts = FontDefinitions::default();

    if let Some(ref path) = kde.font_path
        && let Ok(bytes) = std::fs::read(path)
    {
        fonts.font_data.insert(
            String::from("kde_system_sans"),
            Arc::new(FontData::from_owned(bytes)),
        );
        if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
            prop.insert(0, String::from("kde_system_sans"));
        }
    }

    for (idx, sym_path) in resolve_symbol_fallback_fonts().into_iter().enumerate() {
        if let Ok(bytes) = std::fs::read(&sym_path) {
            let key = format!("kde_symbols_fallback_{idx}");
            fonts
                .font_data
                .insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
            if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
                let insert_pos = usize::from(prop.first().is_some_and(|f| f == "kde_system_sans"));
                prop.insert(insert_pos, key.clone());
            }
            if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
                mono.push(key);
            }
        }
    }

    ctx.set_fonts(fonts);
    kde
}

/// Applies the selected `ThemeKind` tokens and KDE-scaled typography to `ctx`.
///
/// In KDE/Qt on Linux/Wayland, font sizes in `kdeglobals` are given in **typographic points (72 pt/inch)**,
/// whereas logical desktop pixels operate at **96 logical DPI** (`1 pt = 96/72 = 1.3333 logical px`),
/// and the Wayland compositor then applies the display's hardware scale factor on top.
pub fn apply_theme_and_typography(
    ctx: &Context,
    kind: ThemeKind,
    kde: &KdeTypography,
    ui_scale: f32,
) {
    let t = kind.tokens();
    let scale = ui_scale.clamp(0.80, 1.60);

    // Convert KDE point size (72 DPI) -> logical desktop pixels (96 DPI):
    // e.g. 10 pt in KDE = 13.33 logical px.
    let body_px = (kde.point_size * scale) * (96.0 / 72.0);
    let small_px = (body_px * 0.86).max(10.5);
    let heading_px = body_px * 1.22;
    let mono_px = (body_px * 0.94).max(11.0);

    let mut style = Style {
        visuals: if kind.is_dark() {
            Visuals::dark()
        } else {
            Visuals::light()
        },
        ..Style::default()
    };

    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(small_px, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(body_px, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(body_px, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(heading_px, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(mono_px, FontFamily::Monospace),
    );

    // Generous, breathable spacing (matches LumaPrint / KDE Breeze / PhotoCraft)
    style.spacing.item_spacing = Vec2::new(8.0 * scale, 6.0 * scale);
    style.spacing.window_margin = Margin::same((12.0 * scale) as i8);
    style.spacing.button_padding = Vec2::new(10.0 * scale, 5.0 * scale);
    style.spacing.interact_size = Vec2::new(44.0 * scale, 26.0 * scale);
    style.spacing.indent = 16.0 * scale;
    style.spacing.slider_width = 150.0 * scale;
    style.spacing.combo_width = 180.0 * scale;
    style.spacing.scroll.bar_width = 8.0 * scale;

    let r_sm = CornerRadius::same(t.radius_sm);
    let r_md = CornerRadius::same(t.radius);
    let r_lg = CornerRadius::same(t.radius_lg);

    style.visuals.dark_mode = kind.is_dark();
    style.visuals.override_text_color = Some(t.text);
    style.visuals.panel_fill = t.dock;
    style.visuals.window_fill = t.card;
    style.visuals.window_stroke = Stroke::new(1.0_f32, t.card_border);
    style.visuals.window_corner_radius = r_lg;
    style.visuals.menu_corner_radius = r_md;
    style.visuals.faint_bg_color = t.card_header;
    style.visuals.extreme_bg_color = t.field;
    style.visuals.code_bg_color = t.field;
    style.visuals.warn_fg_color = t.warning;
    style.visuals.error_fg_color = t.danger;
    style.visuals.hyperlink_color = t.accent;

    style.visuals.selection = Selection {
        bg_fill: t.accent_soft,
        stroke: Stroke::new(1.0_f32, t.accent),
    };

    style.visuals.widgets = Widgets {
        noninteractive: WidgetVisuals {
            bg_fill: t.dock,
            weak_bg_fill: t.card,
            bg_stroke: Stroke::new(1.0_f32, t.separator),
            corner_radius: r_sm,
            fg_stroke: Stroke::new(1.0_f32, t.text_dim),
            expansion: 0.0,
        },
        inactive: WidgetVisuals {
            bg_fill: t.field,
            weak_bg_fill: t.card_header,
            bg_stroke: Stroke::new(1.0_f32, t.field_border),
            corner_radius: r_sm,
            fg_stroke: Stroke::new(1.0_f32, t.text),
            expansion: 0.0,
        },
        hovered: WidgetVisuals {
            bg_fill: t.hover,
            weak_bg_fill: t.hover,
            bg_stroke: Stroke::new(1.0_f32, t.accent),
            corner_radius: r_sm,
            fg_stroke: Stroke::new(1.5_f32, t.text),
            expansion: 1.0,
        },
        active: WidgetVisuals {
            bg_fill: t.pressed,
            weak_bg_fill: t.pressed,
            bg_stroke: Stroke::new(1.5_f32, t.accent),
            corner_radius: r_sm,
            fg_stroke: Stroke::new(1.5_f32, t.accent_text),
            expansion: 1.0,
        },
        open: WidgetVisuals {
            bg_fill: t.card_header,
            weak_bg_fill: t.card_header,
            bg_stroke: Stroke::new(1.0_f32, t.accent),
            corner_radius: r_sm,
            fg_stroke: Stroke::new(1.0_f32, t.text),
            expansion: 0.0,
        },
    };

    ctx.set_style(style);
}
