//! Colors for the popup form, taken from Herdr's `config.toml`.
//!
//! Herdr already owns the popup chrome (title + border). We only color the
//! inner widgets so they match `theme.name` and `[theme.custom]`.

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Theme {
    pub panel_bg: Color,
    pub text: Color,
    pub subtext: Color,
    pub overlay: Color,
    pub accent: Color,
    pub yellow: Color,
    pub green: Color,
    pub red: Color,
    pub surface: Color,
}

impl Theme {
    pub fn load() -> Self {
        let (name, custom) = read_herdr_theme();
        let mut t = builtin(&name);
        apply_custom(&mut t, &custom);
        t
    }

    pub fn base(&self) -> Style {
        Style::default().bg(self.panel_bg).fg(self.text)
    }

    pub fn muted(&self) -> Style {
        Style::default().fg(self.overlay)
    }

    pub fn input(&self, focused: bool) -> Style {
        if focused {
            Style::default().fg(self.text)
        } else {
            Style::default().fg(self.subtext)
        }
    }

    pub fn input_border(&self, focused: bool) -> Style {
        if focused {
            Style::default()
                .fg(self.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.overlay)
        }
    }

    pub fn suggest(&self, selected: bool) -> Style {
        if selected {
            Style::default().fg(self.panel_bg).bg(self.accent)
        } else {
            Style::default().fg(self.subtext)
        }
    }

    pub fn card_border(&self, focused: bool, selected: bool) -> Style {
        if focused {
            Style::default()
                .fg(self.accent)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(self.yellow)
        } else {
            Style::default().fg(self.overlay)
        }
    }

    pub fn button(&self, focused: bool, emphasize: bool) -> Style {
        if focused {
            Style::default()
                .fg(self.panel_bg)
                .bg(self.accent)
                .add_modifier(Modifier::BOLD)
        } else if emphasize {
            Style::default().fg(self.green)
        } else {
            Style::default().fg(self.subtext)
        }
    }

    pub fn error(&self) -> Style {
        Style::default().fg(self.yellow)
    }
}

#[derive(Debug, Default, Deserialize)]
struct HerdrToml {
    theme: Option<ThemeSection>,
}

#[derive(Debug, Default, Deserialize)]
struct ThemeSection {
    name: Option<String>,
    custom: Option<HashMap<String, String>>,
}

fn read_herdr_theme() -> (String, HashMap<String, String>) {
    let path = herdr_config_path();
    let Ok(raw) = fs::read_to_string(&path) else {
        return ("catppuccin".into(), HashMap::new());
    };
    let parsed: HerdrToml = toml::from_str(&raw).unwrap_or_default();
    let theme = parsed.theme.unwrap_or_default();
    (
        theme.name.unwrap_or_else(|| "catppuccin".into()),
        theme.custom.unwrap_or_default(),
    )
}

fn herdr_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("HERDR_CONFIG_PATH") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    home.join(".config/herdr/config.toml")
}

fn apply_custom(t: &mut Theme, custom: &HashMap<String, String>) {
    for (key, value) in custom {
        let Some(color) = parse_color(value) else {
            continue;
        };
        match key.as_str() {
            "panel_bg" | "sidebar_bg" => t.panel_bg = color,
            "text" => t.text = color,
            "subtext0" | "subtext" => t.subtext = color,
            "overlay0" | "overlay1" => t.overlay = color,
            "accent" | "mauve" => t.accent = color,
            "yellow" => t.yellow = color,
            "green" => t.green = color,
            "red" => t.red = color,
            "surface0" | "surface1" => t.surface = color,
            _ => {}
        }
    }
    // sidebar_bg should not override a more specific panel_bg.
    if let Some(panel) = custom.get("panel_bg").and_then(|v| parse_color(v)) {
        t.panel_bg = panel;
    }
}

pub fn parse_color(raw: &str) -> Option<Color> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("reset") {
        return Some(Color::Reset);
    }
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(inner) = s
        .strip_prefix("rgb(")
        .and_then(|t| t.strip_suffix(')'))
    {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() == 3 {
            let r = parts[0].parse().ok()?;
            let g = parts[1].parse().ok()?;
            let b = parts[2].parse().ok()?;
            return Some(Color::Rgb(r, g, b));
        }
    }
    named(s)
}

fn parse_hex(hex: &str) -> Option<Color> {
    if hex.len() == 6 {
        let n = u32::from_str_radix(hex, 16).ok()?;
        Some(Color::Rgb(
            ((n >> 16) & 0xff) as u8,
            ((n >> 8) & 0xff) as u8,
            (n & 0xff) as u8,
        ))
    } else if hex.len() == 3 {
        let n = u32::from_str_radix(hex, 16).ok()?;
        let r = (((n >> 8) & 0xf) * 0x11) as u8;
        let g = (((n >> 4) & 0xf) * 0x11) as u8;
        let b = ((n & 0xf) * 0x11) as u8;
        Some(Color::Rgb(r, g, b))
    } else {
        None
    }
}

fn named(s: &str) -> Option<Color> {
    Some(match s.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "white" => Color::White,
        _ => return None,
    })
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

fn builtin(name: &str) -> Theme {
    match name {
        "gruvbox" => Theme {
            panel_bg: rgb(0x1d, 0x20, 0x21),
            text: rgb(0xeb, 0xdb, 0xb2),
            subtext: rgb(0xd5, 0xc4, 0xa1),
            overlay: rgb(0x92, 0x83, 0x74),
            accent: rgb(0xfa, 0xbd, 0x2f),
            yellow: rgb(0xfa, 0xbd, 0x2f),
            green: rgb(0xb8, 0xbb, 0x26),
            red: rgb(0xfb, 0x49, 0x34),
            surface: rgb(0x3c, 0x38, 0x36),
        },
        "tokyo-night" => Theme {
            panel_bg: rgb(0x1a, 0x1b, 0x26),
            text: rgb(0xc0, 0xca, 0xf5),
            subtext: rgb(0xa9, 0xb1, 0xd6),
            overlay: rgb(0x56, 0x5f, 0x89),
            accent: rgb(0x7a, 0xa2, 0xf7),
            yellow: rgb(0xe0, 0xaf, 0x68),
            green: rgb(0x9e, 0xce, 0x6a),
            red: rgb(0xf7, 0x76, 0x8e),
            surface: rgb(0x24, 0x28, 0x3b),
        },
        "dracula" => Theme {
            panel_bg: rgb(0x28, 0x2a, 0x36),
            text: rgb(0xf8, 0xf8, 0xf2),
            subtext: rgb(0xbf, 0xbf, 0xb2),
            overlay: rgb(0x62, 0x72, 0xa4),
            accent: rgb(0xbd, 0x93, 0xf9),
            yellow: rgb(0xf1, 0xfa, 0x8c),
            green: rgb(0x50, 0xfa, 0x7b),
            red: rgb(0xff, 0x55, 0x55),
            surface: rgb(0x44, 0x47, 0x5a),
        },
        "nord" => Theme {
            panel_bg: rgb(0x2e, 0x34, 0x40),
            text: rgb(0xec, 0xee, 0xf4),
            subtext: rgb(0xd8, 0xde, 0xe9),
            overlay: rgb(0x4c, 0x56, 0x6a),
            accent: rgb(0x88, 0xc0, 0xd0),
            yellow: rgb(0xeb, 0xcb, 0x8b),
            green: rgb(0xa3, 0xbe, 0x8c),
            red: rgb(0xbf, 0x61, 0x6a),
            surface: rgb(0x3b, 0x42, 0x52),
        },
        "one-dark" => Theme {
            panel_bg: rgb(0x28, 0x2c, 0x34),
            text: rgb(0xab, 0xb2, 0xbf),
            subtext: rgb(0x9d, 0xa5, 0xb4),
            overlay: rgb(0x5c, 0x63, 0x70),
            accent: rgb(0x61, 0xaf, 0xef),
            yellow: rgb(0xe5, 0xc0, 0x7b),
            green: rgb(0x98, 0xc3, 0x79),
            red: rgb(0xe0, 0x6c, 0x75),
            surface: rgb(0x3e, 0x44, 0x51),
        },
        "kanagawa" => Theme {
            panel_bg: rgb(0x1f, 0x1f, 0x28),
            text: rgb(0xdc, 0xd7, 0xba),
            subtext: rgb(0xc8, 0xc0, 0x93),
            overlay: rgb(0x72, 0x71, 0x69),
            accent: rgb(0x7e, 0x9c, 0xd8),
            yellow: rgb(0xc0, 0xa3, 0x6e),
            green: rgb(0x76, 0x94, 0x6a),
            red: rgb(0xc3, 0x40, 0x43),
            surface: rgb(0x2a, 0x2a, 0x37),
        },
        "rose-pine" => Theme {
            panel_bg: rgb(0x19, 0x17, 0x24),
            text: rgb(0xe0, 0xde, 0xf4),
            subtext: rgb(0x90, 0x8c, 0xaa),
            overlay: rgb(0x6e, 0x6a, 0x86),
            accent: rgb(0xc4, 0xa7, 0xe7),
            yellow: rgb(0xf6, 0xc1, 0x77),
            green: rgb(0x9c, 0xcf, 0xd8),
            red: rgb(0xeb, 0x6f, 0x92),
            surface: rgb(0x26, 0x23, 0x3a),
        },
        "vesper" => Theme {
            panel_bg: rgb(0x10, 0x10, 0x10),
            text: rgb(0xff, 0xff, 0xff),
            subtext: rgb(0xa0, 0xa0, 0xa0),
            overlay: rgb(0x60, 0x60, 0x60),
            accent: rgb(0xff, 0xc7, 0x99),
            yellow: rgb(0xff, 0xc7, 0x99),
            green: rgb(0x99, 0xff, 0xe4),
            red: rgb(0xff, 0x80, 0x80),
            surface: rgb(0x1c, 0x1c, 0x1c),
        },
        "solarized" => Theme {
            panel_bg: rgb(0x00, 0x2b, 0x36),
            text: rgb(0x83, 0x94, 0x96),
            subtext: rgb(0x65, 0x7b, 0x83),
            overlay: rgb(0x58, 0x6e, 0x75),
            accent: rgb(0x26, 0x8b, 0xd2),
            yellow: rgb(0xb5, 0x89, 0x00),
            green: rgb(0x85, 0x99, 0x00),
            red: rgb(0xdc, 0x32, 0x2f),
            surface: rgb(0x07, 0x36, 0x42),
        },
        "terminal" => Theme {
            panel_bg: Color::Reset,
            text: Color::Reset,
            subtext: Color::Gray,
            overlay: Color::DarkGray,
            accent: Color::Cyan,
            yellow: Color::Yellow,
            green: Color::Green,
            red: Color::Red,
            surface: Color::Reset,
        },
        // catppuccin (default) and unknown names
        _ => Theme {
            panel_bg: rgb(0x1e, 0x1e, 0x2e),
            text: rgb(0xcd, 0xd6, 0xf4),
            subtext: rgb(0xa6, 0xad, 0xc8),
            overlay: rgb(0x6c, 0x70, 0x86),
            accent: rgb(0xc6, 0xa0, 0xf6),
            yellow: rgb(0xf9, 0xe2, 0xaf),
            green: rgb(0xa6, 0xe3, 0xa1),
            red: rgb(0xf3, 0x8b, 0xa8),
            surface: rgb(0x31, 0x32, 0x44),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_rgb() {
        assert_eq!(parse_color("#1c1c1c"), Some(Color::Rgb(0x1c, 0x1c, 0x1c)));
        assert_eq!(parse_color("#fff"), Some(Color::Rgb(0xff, 0xff, 0xff)));
        assert_eq!(parse_color("rgb(250, 189, 47)"), Some(Color::Rgb(250, 189, 47)));
        assert_eq!(parse_color("reset"), Some(Color::Reset));
        assert_eq!(parse_color("cyan"), Some(Color::Cyan));
    }

    #[test]
    fn custom_panel_bg_wins_over_sidebar() {
        let mut t = builtin("gruvbox");
        let mut custom = HashMap::new();
        custom.insert("sidebar_bg".into(), "#1c1c1c".into());
        custom.insert("panel_bg".into(), "#282828".into());
        apply_custom(&mut t, &custom);
        assert_eq!(t.panel_bg, Color::Rgb(0x28, 0x28, 0x28));
    }

    #[test]
    fn sidebar_bg_applies_when_no_panel() {
        let mut t = builtin("gruvbox");
        let mut custom = HashMap::new();
        custom.insert("sidebar_bg".into(), "#1c1c1c".into());
        apply_custom(&mut t, &custom);
        assert_eq!(t.panel_bg, Color::Rgb(0x1c, 0x1c, 0x1c));
    }
}
