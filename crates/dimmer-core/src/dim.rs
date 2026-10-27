//! Dimming and tint maths shared by the UI, the D-Bus state and tests.

/// Maximum dim percentage, matching the original slider range (0–90).
pub const MAX_DIM: f64 = 90.0;

/// Clamps a dim percentage, mapping NaN/infinities to zero.
#[must_use]
pub const fn clamp_dim(dim: f64) -> f64 {
    if dim.is_finite() { dim.clamp(0.0, MAX_DIM) } else { 0.0 }
}

/// Effective opacity percentage for a screen.  A disabled screen is fully clear.
#[must_use]
pub const fn effective_dim(on: bool, dim: f64) -> f64 {
    if on { clamp_dim(dim) } else { 0.0 }
}

/// Converts a dim percentage to a compositor opacity in the `0.0..=1.0` range.
#[must_use]
pub const fn overlay_alpha(dim: f64) -> f64 {
    clamp_dim(dim) / 100.0
}

/// Parses `RRGGBB` or `#RRGGBB` and returns uppercase `RRGGBB`.
#[must_use]
pub fn normalize_tint(value: &str) -> Option<String> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(value.to_ascii_uppercase())
}

/// Parses a tint into red, green and blue components.
#[must_use]
pub fn parse_tint(value: &str) -> Option<(u8, u8, u8)> {
    let value = normalize_tint(value)?;
    let red = u8::from_str_radix(&value[0..2], 16).ok()?;
    let green = u8::from_str_radix(&value[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&value[4..6], 16).ok()?;
    Some((red, green, blue))
}

/// Formats a tint from RGB components.
#[must_use]
pub fn format_tint(red: u8, green: u8, blue: u8) -> String {
    format!("{red:02X}{green:02X}{blue:02X}")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::field_reassign_with_default
    )]
    use super::*;

    #[test]
    fn clamps_dim() {
        assert_eq!(clamp_dim(-1.0), 0.0);
        assert_eq!(clamp_dim(0.0), 0.0);
        assert_eq!(clamp_dim(42.5), 42.5);
        assert_eq!(clamp_dim(90.0), 90.0);
        assert_eq!(clamp_dim(91.0), 90.0);
        assert_eq!(clamp_dim(f64::NAN), 0.0);
        assert_eq!(clamp_dim(f64::INFINITY), 0.0);
    }

    #[test]
    fn disabled_screen_is_clear() {
        assert_eq!(effective_dim(false, 80.0), 0.0);
        assert_eq!(effective_dim(true, 80.0), 80.0);
    }

    #[test]
    fn opacity_matches_percentage() {
        assert_eq!(overlay_alpha(0.0), 0.0);
        assert_eq!(overlay_alpha(30.0), 0.3);
        assert_eq!(overlay_alpha(200.0), 0.9);
    }

    #[test]
    fn tint_round_trip() {
        assert_eq!(normalize_tint("a0b1c2").as_deref(), Some("A0B1C2"));
        assert_eq!(normalize_tint("#A0B1C2").as_deref(), Some("A0B1C2"));
        assert_eq!(parse_tint("FF8000"), Some((255, 128, 0)));
        assert_eq!(format_tint(255, 128, 0), "FF8000");
        assert_eq!(normalize_tint("GG0000"), None);
        assert_eq!(normalize_tint("12345"), None);
        assert_eq!(normalize_tint(""), None);
    }
}
