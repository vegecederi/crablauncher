use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const THEME_SCHEMA_VERSION: u32 = 1;

/// Manifesto declarativo TOML de um tema (`theme.toml`).
#[derive(Debug, Clone, Deserialize)]
pub struct ThemeManifest {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    pub metadata: ThemeMetadata,
    #[serde(default)]
    pub colors: ThemeColorsOverride,
    #[serde(default)]
    pub geometry: ThemeGeometryOverride,
    #[serde(default)]
    pub wallpaper: Option<ThemeWallpaperConfig>,
}

fn default_schema() -> u32 {
    THEME_SCHEMA_VERSION
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThemeMetadata {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub base: Option<String>,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeColorsOverride {
    pub background: Option<String>,
    pub surface: Option<String>,
    pub surface_elevated: Option<String>,
    pub border: Option<String>,
    pub border_subtle: Option<String>,
    pub accent: Option<String>,
    pub accent_hover: Option<String>,
    pub accent_pressed: Option<String>,
    pub accent_muted: Option<String>,
    pub accent_foreground: Option<String>,
    pub text_primary: Option<String>,
    pub text_secondary: Option<String>,
    pub text_disabled: Option<String>,
    pub hover: Option<String>,
    pub pressed: Option<String>,
    pub danger: Option<String>,
    pub success: Option<String>,
    pub warning: Option<String>,
    pub console_bg: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeGeometryOverride {
    pub border_radius: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThemeWallpaperConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub opacity: Option<f32>,
    #[serde(default)]
    pub blur: Option<u32>,
}

/// Cor RGBA pronta para aplicar no Slint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    pub fn parse_hex(raw: &str) -> Option<Self> {
        let hex = raw.trim().strip_prefix('#')?;
        match hex.len() {
            6 => Some(Self {
                r: u8::from_str_radix(&hex[0..2], 16).ok()?,
                g: u8::from_str_radix(&hex[2..4], 16).ok()?,
                b: u8::from_str_radix(&hex[4..6], 16).ok()?,
                a: 255,
            }),
            8 => Some(Self {
                r: u8::from_str_radix(&hex[0..2], 16).ok()?,
                g: u8::from_str_radix(&hex[2..4], 16).ok()?,
                b: u8::from_str_radix(&hex[4..6], 16).ok()?,
                a: u8::from_str_radix(&hex[6..8], 16).ok()?,
            }),
            _ => None,
        }
    }
}

/// Tema completamente resolvido e validado (com herança aplicada) pronto para consumo pela UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTheme {
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub is_builtin: bool,
    pub source_dir: Option<PathBuf>,

    pub bg_primary: RgbaColor,
    pub bg_secondary: RgbaColor,
    pub bg_interactive: RgbaColor,
    pub bg_interactive_hover: RgbaColor,
    pub bg_interactive_pressed: RgbaColor,
    pub border: RgbaColor,
    pub border_subtle: RgbaColor,
    pub accent: RgbaColor,
    pub accent_hover: RgbaColor,
    pub accent_pressed: RgbaColor,
    pub accent_muted: RgbaColor,
    pub text_on_accent: RgbaColor,
    pub text_primary: RgbaColor,
    pub text_secondary: RgbaColor,
    pub text_disabled: RgbaColor,
    pub danger: RgbaColor,
    pub success: RgbaColor,
    pub warning_text: RgbaColor,
    pub console_bg: RgbaColor,
    pub border_radius: u32,
}

impl Default for ResolvedTheme {
    fn default() -> Self {
        Self {
            id: "crab-dark".into(),
            name: "Crab Dark".into(),
            author: "Crab Team".into(),
            version: "1.0.0".into(),
            description: "Classic minimalist dark theme with salmon-orange accents.".into(),
            is_builtin: true,
            source_dir: None,
            bg_primary: RgbaColor::parse_hex("#1a1a1a").expect("valid"),
            bg_secondary: RgbaColor::parse_hex("#222222").expect("valid"),
            bg_interactive: RgbaColor::parse_hex("#2b2b2b").expect("valid"),
            bg_interactive_hover: RgbaColor::parse_hex("#343434").expect("valid"),
            bg_interactive_pressed: RgbaColor::parse_hex("#242424").expect("valid"),
            border: RgbaColor::parse_hex("#333333").expect("valid"),
            border_subtle: RgbaColor::parse_hex("#292929").expect("valid"),
            accent: RgbaColor::parse_hex("#ff751f").expect("valid"),
            accent_hover: RgbaColor::parse_hex("#ff863b").expect("valid"),
            accent_pressed: RgbaColor::parse_hex("#e06114").expect("valid"),
            accent_muted: RgbaColor::parse_hex("#ff751f24").expect("valid"),
            text_on_accent: RgbaColor::parse_hex("#141414").expect("valid"),
            text_primary: RgbaColor::parse_hex("#f2f2f2").expect("valid"),
            text_secondary: RgbaColor::parse_hex("#a0a0a0").expect("valid"),
            text_disabled: RgbaColor::parse_hex("#666666").expect("valid"),
            danger: RgbaColor::parse_hex("#d14343").expect("valid"),
            success: RgbaColor::parse_hex("#4caf50").expect("valid"),
            warning_text: RgbaColor::parse_hex("#f0b35a").expect("valid"),
            console_bg: RgbaColor::parse_hex("#141414").expect("valid"),
            border_radius: 4,
        }
    }
}
