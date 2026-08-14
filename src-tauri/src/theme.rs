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

/// The palette file Omarchy themes declare their colours in: flat keys,
/// `accent = "#7aa2f7"` first among them. Alacritty's `normal.blue` was only
/// ever a guess at this value — right for Tokyo Night, wrong for any theme
/// whose accent is not its terminal blue — so when the theme author wrote
/// the accent down, believe them.
#[derive(Deserialize)]
struct ColorsFile {
    accent: Option<String>,
}

/// The explicit accent from a theme's `colors.toml`, normalised, or `None`
/// when the file is not TOML, has no accent, or the accent is not a colour —
/// each of which means "keep guessing", never "break the theme".
pub fn parse_colors_accent(toml_src: &str) -> Option<String> {
    let file: ColorsFile = toml::from_str(toml_src).ok()?;
    parse_hex(&file.accent?).map(format_hex)
}

/// Parses `#rrggbb`, `0xrrggbb`, `0Xrrggbb`, or bare `rrggbb` into RGB bytes.
/// Returns None for anything else — no slicing panics, no partial parses.
fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let trimmed = if let Some(stripped) = s.strip_prefix('#') {
        stripped
    } else if let Some(stripped) = s.strip_prefix("0x") {
        stripped
    } else if let Some(stripped) = s.strip_prefix("0X") {
        stripped
    } else {
        s
    };

    // Require exactly 6 ASCII hex digits; validate before slicing.
    if trimmed.len() != 6 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    // Safe to slice: we've verified 6 ASCII chars.
    let r = u8::from_str_radix(&trimmed[0..2], 16).ok()?;
    let g = u8::from_str_radix(&trimmed[2..4], 16).ok()?;
    let b = u8::from_str_radix(&trimmed[4..6], 16).ok()?;

    Some([r, g, b])
}

/// Normalize RGB bytes to lowercase `#rrggbb`.
fn format_hex(bytes: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2])
}

/// Relative luminance of RGB bytes, used only to classify dark vs light.
fn luminance(bytes: [u8; 3]) -> f32 {
    let r = bytes[0] as f32;
    let g = bytes[1] as f32;
    let b = bytes[2] as f32;
    (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0
}

/// Lightens or darkens RGB by `amount` (-1.0..=1.0), used to derive a surface
/// colour one step away from the background.
fn shift(bytes: [u8; 3], amount: f32) -> String {
    let ch = |i: usize| -> u8 {
        let v = bytes[i] as f32;
        (v + 255.0 * amount).clamp(0.0, 255.0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", ch(0), ch(1), ch(2))
}

/// Parses an Alacritty theme into a palette. Returns `None` when the file is
/// not valid TOML, carries no background colour, or contains invalid hex.
pub fn parse_alacritty(toml_src: &str) -> Option<Palette> {
    let file: AlacrittyFile = toml::from_str(toml_src).ok()?;
    let colors = file.colors?;
    let primary = colors.primary?;
    let bg_str = primary.background?;
    let bg_bytes = parse_hex(&bg_str)?;
    let bg = format_hex(bg_bytes);

    let text_str = primary.foreground.unwrap_or_else(|| "#e8e8ea".into());
    let text_bytes = parse_hex(&text_str).unwrap_or([0xe8, 0xe8, 0xea]);
    let text = format_hex(text_bytes);

    let is_dark = luminance(bg_bytes) < 0.5;

    let accent = colors
        .normal
        .as_ref()
        .and_then(|n| n.blue.as_ref())
        .and_then(|blue_str| parse_hex(blue_str))
        .map(format_hex)
        .unwrap_or_else(|| text.clone());

    let muted = colors
        .normal
        .as_ref()
        .and_then(|n| n.white.as_ref())
        .and_then(|white_str| parse_hex(white_str))
        .map(format_hex)
        .unwrap_or_else(|| shift(text_bytes, if is_dark { -0.25 } else { 0.25 }));

    Some(Palette {
        surface: shift(bg_bytes, if is_dark { 0.03 } else { -0.03 }),
        bg,
        text,
        muted,
        accent,
        is_dark,
    })
}

/// Resolves the active palette, following the spec §10 fallback chain:
/// `alacritty.toml` in the theme directory, then the built-in dark palette —
/// and in either case, `colors.toml`'s explicit `accent` outranks whatever
/// the base produced. Never fails.
pub fn resolve(theme_dir: Option<&Path>) -> Palette {
    let Some(dir) = theme_dir else {
        return Palette::fallback_dark();
    };
    let mut palette = match std::fs::read_to_string(dir.join("alacritty.toml")) {
        Ok(src) => parse_alacritty(&src).unwrap_or_else(|| {
            tracing::warn!(?dir, "theme found but could not be parsed; using fallback");
            Palette::fallback_dark()
        }),
        Err(e) => {
            tracing::warn!(?dir, %e, "no readable theme; using fallback");
            Palette::fallback_dark()
        }
    };

    // Applied to the fallback too, deliberately: a theme whose alacritty.toml
    // is broken but whose colors.toml names an accent gets the fallback's
    // legible base wearing the theme's own accent — closer to intent than
    // either file alone.
    if let Ok(src) = std::fs::read_to_string(dir.join("colors.toml")) {
        if let Some(accent) = parse_colors_accent(&src) {
            palette.accent = accent;
        }
    }
    palette
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

    #[test]
    fn resolve_parses_a_real_theme_directory() {
        let temp_dir = std::env::temp_dir().join(format!(
            "omacal_test_{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&temp_dir);
        let theme_file = temp_dir.join("alacritty.toml");
        let toml_content = r##"
[colors.primary]
background = "#1a1b26"
foreground = "#c0caf5"
[colors.normal]
blue = "#7aa2f7"
white = "#a9b1d6"
"##;
        std::fs::write(&theme_file, toml_content).expect("write temp theme file");
        let p = resolve(Some(&temp_dir));
        assert_eq!(p.bg, "#1a1b26");
        assert_eq!(p.text, "#c0caf5");
        assert_eq!(p.accent, "#7aa2f7");
        assert_eq!(p.muted, "#a9b1d6");
        assert!(p.is_dark);
        assert_ne!(p.surface, p.bg, "surface should differ from bg");
        let _ = std::fs::remove_file(&theme_file);
        let _ = std::fs::remove_dir(&temp_dir);
    }

    /// The whole feature: the author's accent beats the blue we guessed.
    /// Distinct values on purpose — an orange accent over a blue guess is
    /// exactly the theme (e.g. anything warm) the guess got wrong.
    #[test]
    fn the_explicit_accent_outranks_alacrittys_blue() {
        let temp_dir = std::env::temp_dir().join(format!("omacal_accent_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        std::fs::write(
            temp_dir.join("alacritty.toml"),
            "[colors.primary]\nbackground = \"#1a1b26\"\nforeground = \"#c0caf5\"\n[colors.normal]\nblue = \"#7aa2f7\"",
        )
        .unwrap();
        std::fs::write(temp_dir.join("colors.toml"), "accent = \"#e0af68\"").unwrap();

        let p = resolve(Some(&temp_dir));
        assert_eq!(p.accent, "#e0af68", "the theme said orange; blue was a guess");
        assert_eq!(p.bg, "#1a1b26", "everything else still comes from alacritty");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    /// The other half, without which the override could be `accent = file
    /// contents`: garbage in colors.toml keeps the guess rather than
    /// breaking the palette.
    #[test]
    fn a_bad_or_absent_colors_accent_keeps_the_guess() {
        assert_eq!(parse_colors_accent("accent = \"#e0af68\""), Some("#e0af68".into()));
        assert_eq!(parse_colors_accent("accent = \"0xE0AF68\""), Some("#e0af68".into()));
        assert_eq!(parse_colors_accent("accent = \"not-a-colour\""), None);
        assert_eq!(parse_colors_accent("cursor = \"#ffffff\""), None, "no accent key");
        assert_eq!(parse_colors_accent("this is not toml {{{"), None);

        // And through `resolve`: a colors.toml with a broken accent leaves
        // alacritty's answer standing.
        let temp_dir = std::env::temp_dir().join(format!("omacal_badacc_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        std::fs::write(
            temp_dir.join("alacritty.toml"),
            "[colors.primary]\nbackground = \"#1a1b26\"\nforeground = \"#c0caf5\"\n[colors.normal]\nblue = \"#7aa2f7\"",
        )
        .unwrap();
        std::fs::write(temp_dir.join("colors.toml"), "accent = \"nope\"").unwrap();
        assert_eq!(resolve(Some(&temp_dir)).accent, "#7aa2f7");
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn zero_x_prefixed_colours_parse() {
        let src = r##"
[colors.primary]
background = "0x1a1b26"
foreground = "0xc0caf5"
[colors.normal]
blue = "0x7aa2f7"
"##;
        let p = parse_alacritty(src).unwrap();
        assert_eq!(p.bg, "#1a1b26", "0x prefix should normalize to #");
        assert_eq!(p.text, "#c0caf5", "0x prefix should normalize to #");
        assert_eq!(p.accent, "#7aa2f7", "0x prefix should normalize to #");
        assert!(p.is_dark);
        assert_ne!(p.surface, p.bg, "surface should differ from bg even with 0x format");
    }

    #[test]
    fn malformed_colours_do_not_panic() {
        // Table-driven: each variant should fail gracefully without panic.
        let test_cases = vec![
            (r##"[colors.primary]
background = ""
foreground = "#c0caf5""##, "empty string"),
            (r##"[colors.primary]
background = "#GGGGGG"
foreground = "#c0caf5""##, "invalid hex chars"),
            (r##"[colors.primary]
background = "#abc"
foreground = "#c0caf5""##, "3 digits instead of 6"),
            (r##"[colors.primary]
background = "#1a1b26ff"
foreground = "#c0caf5""##, "8 digits instead of 6"),
        ];

        for (src, label) in test_cases {
            let result = parse_alacritty(src);
            assert!(
                result.is_none(),
                "malformed colour {} should be rejected, not parsed",
                label
            );
        }
    }

    #[test]
    fn a_multibyte_background_does_not_panic() {
        let src = r##"
[colors.primary]
background = "€€"
foreground = "#c0caf5"
"##;
        // This should return None due to invalid hex, not panic on char boundary.
        let result = parse_alacritty(src);
        assert!(result.is_none(), "multibyte background should be rejected");
    }
}
