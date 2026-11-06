//! Custom accent colours.
//!
//! Each theme in `assets/static/theme.css` ships a hand-tuned amber accent.
//! A user can replace it with any RGB colour. From that one colour this
//! module derives every accent token for all three themes, so the header's
//! theme switcher keeps working without recomputing anything:
//!
//! - `--primary` (links, active states) is the colour pushed towards white on
//!   dark or towards black on light/sepia until it reaches 4.5:1 contrast
//!   with the theme surface, so text stays readable whatever was picked;
//! - `--primary-container` is the picked colour itself (gradient end);
//! - `--on-primary` is whichever of a near-black or white reads best on both;
//! - `--primary-soft`, `--upvote` and `--cover-a` are tints of the above.

/// Name of the accent cookie. Absent means the theme's own accent.
pub const ACCENT_COOKIE: &str = "taverna_accent";

/// An sRGB colour with 8-bit channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Parses `#rrggbb` (case-insensitive); `None` for anything else.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let hex = value.trim().strip_prefix('#')?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Self(channel(0)?, channel(2)?, channel(4)?))
    }

    /// Lower-case `#rrggbb`.
    #[must_use]
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// WCAG relative luminance.
    fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.0722_f64.mul_add(
            lin(self.2),
            0.2126_f64.mul_add(lin(self.0), 0.7152 * lin(self.1)),
        )
    }

    /// WCAG contrast ratio between two colours.
    fn contrast(self, other: Self) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// `self` moved `amount` (0..=1) of the way towards `other`.
    fn mix(self, other: Self, amount: f64) -> Self {
        let ch = |a: u8, b: u8| {
            let v = (f64::from(b) - f64::from(a)).mul_add(amount, f64::from(a));
            // Clamped to 0..=255 first, so the cast cannot truncate.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = v.round().clamp(0.0, 255.0) as u8;
            v
        };
        Self(
            ch(self.0, other.0),
            ch(self.1, other.1),
            ch(self.2, other.2),
        )
    }

    fn rgba(self, alpha: f64) -> String {
        format!("rgba({}, {}, {}, {alpha})", self.0, self.1, self.2)
    }
}

const WHITE: Rgb = Rgb(255, 255, 255);
const BLACK: Rgb = Rgb(0, 0, 0);
const MIN_TEXT_CONTRAST: f64 = 4.5;

/// The parts of a theme an accent is derived against.
struct ThemeBase {
    name: &'static str,
    /// Surface the accent is read on as text.
    surface: Rgb,
    /// Colour `--primary` is pushed towards to gain contrast.
    toward: Rgb,
    /// Dark text colour used for `--on-primary` when it reads better.
    on_dark: Rgb,
    /// Background `--cover-a` is tinted from, and how much accent it gets.
    cover_base: Rgb,
    cover_amount: f64,
    soft_alpha: f64,
}

const THEMES: [ThemeBase; 3] = [
    ThemeBase {
        name: "dark",
        surface: Rgb(0x1f, 0x20, 0x22),
        toward: WHITE,
        on_dark: Rgb(0x1a, 0x14, 0x0c),
        cover_base: Rgb(0x12, 0x13, 0x14),
        cover_amount: 0.3,
        soft_alpha: 0.14,
    },
    ThemeBase {
        name: "light",
        surface: Rgb(0xf3, 0xf1, 0xed),
        toward: BLACK,
        on_dark: Rgb(0x1d, 0x1b, 0x19),
        cover_base: WHITE,
        cover_amount: 0.45,
        soft_alpha: 0.1,
    },
    ThemeBase {
        name: "sepia",
        surface: Rgb(0xed, 0xe2, 0xc9),
        toward: BLACK,
        on_dark: Rgb(0x3b, 0x2f, 0x1e),
        cover_base: Rgb(0xe5, 0xd7, 0xb9),
        cover_amount: 0.4,
        soft_alpha: 0.12,
    },
];

/// Pushes `color` towards `toward` until it reaches text contrast with
/// `surface` (or becomes `toward` itself).
fn readable_on(color: Rgb, surface: Rgb, toward: Rgb) -> Rgb {
    (0..=20)
        .map(|step| color.mix(toward, f64::from(step) * 0.05))
        .find(|c| c.contrast(surface) >= MIN_TEXT_CONTRAST)
        .unwrap_or(toward)
}

/// The CSS that overrides every theme's accent tokens with `accent`.
#[must_use]
pub fn accent_css(accent: Rgb) -> String {
    THEMES
        .iter()
        .map(|t| {
            let primary = readable_on(accent, t.surface, t.toward);
            let container = accent;
            let worst = |text: Rgb| text.contrast(primary).min(text.contrast(container));
            let on_primary = if worst(WHITE) >= worst(t.on_dark) {
                WHITE
            } else {
                t.on_dark
            };
            let cover = t.cover_base.mix(accent, t.cover_amount);
            format!(
                "[data-theme=\"{name}\"]{{--primary:{p};--primary-container:{c};\
                 --on-primary:{on};--primary-soft:{soft};--upvote:{p};--cover-a:{cover};}}",
                name = t.name,
                p = primary.to_hex(),
                c = container.to_hex(),
                on = on_primary.to_hex(),
                soft = primary.rgba(t.soft_alpha),
                cover = cover.to_hex(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalises_hex() {
        assert_eq!(Rgb::parse("#2F80ED"), Some(Rgb(0x2f, 0x80, 0xed)));
        assert_eq!(Rgb::parse(" #000000 "), Some(BLACK));
        assert_eq!(Rgb::parse("#2f80ed").unwrap().to_hex(), "#2f80ed");
    }

    #[test]
    fn rejects_anything_but_six_digit_hex() {
        for bad in [
            "",
            "#",
            "2f80ed",
            "#fff",
            "#2f80ed00",
            "#zzzzzz",
            "red",
            "#12345é",
            "#2f80ed;}",
        ] {
            assert_eq!(Rgb::parse(bad), None, "{bad:?} should be rejected");
        }
    }

    #[test]
    fn primary_is_readable_for_any_accent() {
        for accent in [
            WHITE,
            BLACK,
            Rgb(255, 255, 0),
            Rgb(0, 0, 255),
            Rgb(128, 128, 128),
        ] {
            for t in &THEMES {
                let p = readable_on(accent, t.surface, t.toward);
                assert!(
                    p.contrast(t.surface) >= MIN_TEXT_CONTRAST,
                    "{accent:?} on {}",
                    t.name
                );
            }
        }
    }

    #[test]
    fn css_covers_all_three_themes() {
        let css = accent_css(Rgb(0x2f, 0x80, 0xed));
        for theme in ["dark", "light", "sepia"] {
            assert!(css.contains(&format!("[data-theme=\"{theme}\"]")));
        }
        assert!(css.contains("--primary-container:#2f80ed"));
        assert!(!css.contains('<'));
    }
}
