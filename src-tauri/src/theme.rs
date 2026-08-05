#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Palette {
    pub bg: String,
    pub surface: String,
    pub text: String,
    pub muted: String,
    pub accent: String,
    pub is_dark: bool,
}

impl Palette {
    /// Used whenever a theme cannot be read or parsed. The app must always
    /// start (spec §10).
    pub fn fallback_dark() -> Self {
        Self {
            bg: "#17171a".into(),
            surface: "#1e1e22".into(),
            text: "#e8e8ea".into(),
            muted: "#8a8a90".into(),
            accent: "#5b8def".into(),
            is_dark: true,
        }
    }
}

#[derive(Deserialize)]
struct AlacrittyFile {
    colors: Option<AlacrittyColors>,
}

#[derive(Deserialize)]
struct AlacrittyColors {
    primary: Option<AlacrittyPrimary>,
    normal: Option<AlacrittyNormal>,
}

#[derive(Deserialize)]
struct AlacrittyPrimary {
    background: Option<String>,
    foreground: Option<String>,
}

#[derive(Deserialize)]
struct AlacrittyNormal {
    blue: Option<String>,
    white: Option<String>,
}

/// Relative luminance of `#rrggbb`, used only to classify dark vs light.
fn luminance(hex: &str) -> Option<f32> {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()? as f32;
    let g = u8::from_str_radix(&h[2..4], 16).ok()? as f32;
    let b = u8::from_str_radix(&h[4..6], 16).ok()? as f32;
    Some((0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0)
}

/// Lightens or darkens `hex` by `amount` (-1.0..=1.0), used to derive a surface
/// colour one step away from the background.
fn shift(hex: &str, amount: f32) -> String {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return hex.to_string();
    }
    let ch = |i: usize| -> u8 {
        let v = u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f32;
        (v + 255.0 * amount).clamp(0.0, 255.0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", ch(0), ch(2), ch(4))
}

/// Parses an Alacritty theme into a palette. Returns `None` when the file is
/// not valid TOML or carries no background colour.
pub fn parse_alacritty(toml_src: &str) -> Option<Palette> {
    let file: AlacrittyFile = toml::from_str(toml_src).ok()?;
    let colors = file.colors?;
    let primary = colors.primary?;
    let bg = primary.background?;
    let text = primary.foreground.unwrap_or_else(|| "#e8e8ea".into());
    let is_dark = luminance(&bg).map(|l| l < 0.5).unwrap_or(true);
    let accent = colors
        .normal
        .as_ref()
        .and_then(|n| n.blue.clone())
        .unwrap_or_else(|| text.clone());
    let muted = colors
        .normal
        .as_ref()
        .and_then(|n| n.white.clone())
        .unwrap_or_else(|| shift(&text, if is_dark { -0.25 } else { 0.25 }));

    Some(Palette {
        surface: shift(&bg, if is_dark { 0.03 } else { -0.03 }),
        bg,
        text,
        muted,
        accent,
        is_dark,
    })
}

/// Resolves the active palette, following the spec §10 fallback chain:
/// `alacritty.toml` in the theme directory, then the built-in dark palette.
/// Never fails.
pub fn resolve(theme_dir: Option<&Path>) -> Palette {
    let Some(dir) = theme_dir else {
        return Palette::fallback_dark();
    };
    match std::fs::read_to_string(dir.join("alacritty.toml")) {
        Ok(src) => parse_alacritty(&src).unwrap_or_else(|| {
            tracing::warn!(?dir, "theme found but could not be parsed; using fallback");
            Palette::fallback_dark()
        }),
        Err(e) => {
            tracing::warn!(?dir, %e, "no readable theme; using fallback");
            Palette::fallback_dark()
        }
    }
}

/// The conventional Omarchy location. Returns `None` off Linux or when absent.
pub fn omarchy_theme_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    let p = Path::new(&home).join(".config/omarchy/current/theme");
    p.exists().then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/alacritty.toml");

    #[test]
    fn a_tokyo_night_alacritty_theme_parses() {
        let p = parse_alacritty(FIXTURE).unwrap();
        assert_eq!(p.bg, "#1a1b26");
        assert_eq!(p.text, "#c0caf5");
        assert_eq!(p.accent, "#7aa2f7");
        assert!(p.is_dark);
    }

    #[test]
    fn a_light_background_is_detected_as_light() {
        let src = r##"
[colors.primary]
background = "#eff1f5"
foreground = "#4c4f69"
[colors.normal]
blue = "#1e66f5"
"##;
        let p = parse_alacritty(src).unwrap();
        assert!(!p.is_dark);
    }

    #[test]
    fn a_theme_without_a_background_is_rejected() {
        assert!(parse_alacritty("[colors.normal]\nblue = \"#1e66f5\"").is_none());
    }

    #[test]
    fn malformed_toml_is_rejected_without_panicking() {
        assert!(parse_alacritty("this is not toml {{{").is_none());
    }

    #[test]
    fn a_missing_accent_falls_back_to_the_foreground() {
        let src = "[colors.primary]\nbackground = \"#1a1b26\"\nforeground = \"#c0caf5\"";
        let p = parse_alacritty(src).unwrap();
        assert_eq!(p.accent, "#c0caf5");
    }

    #[test]
    fn resolve_falls_back_when_the_directory_is_missing() {
        let p = resolve(Some(std::path::Path::new("/nonexistent/omarchy/theme")));
        assert_eq!(p, Palette::fallback_dark());
    }

    #[test]
    fn resolve_falls_back_when_given_nothing() {
        assert_eq!(resolve(None), Palette::fallback_dark());
    }
}
