//! Tray-icon text rendering.
//!
//! macOS renders cost/token text beside the menu-bar icon; Windows tray icons
//! are too small for reliable inline text, so the app keeps an asset-backed
//! logo in the tray and puts full-precision values in the tooltip. The text
//! renderer is kept for tests/experiments, but is no longer used for the live
//! tray icon.

use std::collections::HashMap;
use std::sync::OnceLock;

pub const GLYPH_W: usize = 5;
pub const GLYPH_H: usize = 7;

/// 5×7 bitmap font, one byte per row, low 5 bits used (MSB = leftmost pixel).
fn glyphs() -> &'static HashMap<char, [u8; 7]> {
    static GLYPHS: OnceLock<HashMap<char, [u8; 7]>> = OnceLock::new();
    GLYPHS.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert('0', [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110]);
        m.insert('1', [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]);
        m.insert('2', [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111]);
        m.insert('3', [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110]);
        m.insert('4', [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010]);
        m.insert('5', [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]);
        m.insert('6', [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110]);
        m.insert('7', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000]);
        m.insert('8', [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]);
        m.insert('9', [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100]);
        m.insert('$', [0b00100, 0b01111, 0b10100, 0b01110, 0b00101, 0b11110, 0b00100]);
        m.insert('.', [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100]);
        m.insert('K', [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]);
        m.insert('M', [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001]);
        m.insert('B', [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110]);
        m
    })
}

/// Compact cost to ≤5 glyphs so it stays legible in a tray icon:
/// $0.12 / $1.23 / $12.3 / $117 / $1.2K / $12K / $117K
pub fn compact_cost(cost: f64) -> String {
    let c = cost.max(0.0);
    if c >= 100_000.0 {
        return format!("${}K", (c / 1000.0).round() as i64);
    }
    if c >= 1000.0 {
        let k = c / 1000.0;
        // 9999 → 9.999 would format as "$10.0K" (6 glyphs); promote to int.
        if (k * 10.0).round() / 10.0 >= 10.0 {
            return format!("${}K", k.round() as i64);
        }
        return format!("${k:.1}K");
    }
    if c >= 99.995 {
        return format!("${}", c.round() as i64);
    }
    if c >= 9.995 {
        return format!("${c:.1}");
    }
    format!("${c:.2}")
}

/// Compact token count to ≤5 glyphs: 950 / 45K / 1.2M / 196M / 1.2B
pub fn compact_tokens(tokens: i64) -> String {
    let t = tokens.max(0);
    if t >= 1_000_000_000 {
        let b = t as f64 / 1e9;
        if b >= 10.0 {
            return format!("{}B", b.round() as i64);
        }
        return format!("{b:.1}B");
    }
    if t >= 1_000_000 {
        let m = t as f64 / 1e6;
        if m >= 10.0 {
            return format!("{}M", m.round() as i64);
        }
        return format!("{m:.1}M");
    }
    if t >= 1_000 {
        let k = t as f64 / 1e3;
        if k >= 10.0 {
            return format!("{}K", k.round() as i64);
        }
        return format!("{k:.1}K");
    }
    format!("{t}")
}

fn line_width_px(line: &str) -> usize {
    if line.is_empty() {
        0
    } else {
        line.chars().count() * (GLYPH_W + 1) - 1
    }
}

/// Render 1-2 text lines into a square RGBA bitmap (row-major, 4 bytes/px).
/// White text with a 1px black shadow at +1,+1 for contrast on light themes.
/// Returns None when `lines` is empty (caller falls back to the logo icon).
pub fn render_tray_text(lines: &[String], size: usize) -> Option<Vec<u8>> {
    let lines: Vec<&String> = lines.iter().filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return None;
    }

    let line_gap = 2usize;
    let total_text_h = GLYPH_H * lines.len() + line_gap * (lines.len() - 1);
    let max_text_w = lines.iter().map(|l| line_width_px(l)).max().unwrap_or(0);
    if max_text_w == 0 {
        return None;
    }

    // Integer scale that fits both dimensions; at least 1.
    let scale = ((size / total_text_h).min(size / max_text_w)).max(1);

    let mut rgba = vec![0u8; size * size * 4];
    let scaled_h = total_text_h * scale;
    let y0 = (size.saturating_sub(scaled_h)) / 2;

    for (li, line) in lines.iter().enumerate() {
        let lw = line_width_px(line) * scale;
        let x0 = (size.saturating_sub(lw)) / 2;
        let ly = y0 + li * (GLYPH_H + line_gap) * scale;
        draw_line(&mut rgba, size, line, x0, ly, scale);
    }

    Some(rgba)
}

fn draw_line(rgba: &mut [u8], size: usize, text: &str, x0: usize, y0: usize, scale: usize) {
    let font = glyphs();
    let mut pen_x = x0;
    for ch in text.chars() {
        if let Some(rows) = font.get(&ch) {
            for (gy, row) in rows.iter().enumerate() {
                for gx in 0..GLYPH_W {
                    if row & (1 << (GLYPH_W - 1 - gx)) != 0 {
                        // shadow then glyph
                        fill_px(rgba, size, pen_x + gx * scale + 1, y0 + gy * scale + 1, scale, [0, 0, 0, 160]);
                    }
                }
            }
            for (gy, row) in rows.iter().enumerate() {
                for gx in 0..GLYPH_W {
                    if row & (1 << (GLYPH_W - 1 - gx)) != 0 {
                        fill_px(rgba, size, pen_x + gx * scale, y0 + gy * scale, scale, [255, 255, 255, 255]);
                    }
                }
            }
        }
        pen_x += (GLYPH_W + 1) * scale;
    }
}

fn fill_px(rgba: &mut [u8], size: usize, x: usize, y: usize, scale: usize, color: [u8; 4]) {
    for dy in 0..scale {
        for dx in 0..scale {
            let px = x + dx;
            let py = y + dy;
            if px < size && py < size {
                let idx = (py * size + px) * 4;
                // Don't let the shadow overwrite an already-white pixel.
                if color[3] == 255 || rgba[idx + 3] == 0 {
                    rgba[idx..idx + 4].copy_from_slice(&color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_cost_tiers() {
        assert_eq!(compact_cost(0.0), "$0.00");
        assert_eq!(compact_cost(0.12), "$0.12");
        assert_eq!(compact_cost(1.234), "$1.23");
        assert_eq!(compact_cost(12.34), "$12.3");
        assert_eq!(compact_cost(117.07), "$117");
        assert_eq!(compact_cost(1234.0), "$1.2K");
        assert_eq!(compact_cost(12345.0), "$12K");
        for v in [0.0, 0.5, 9.99, 99.9, 999.0, 9999.0, 99999.0] {
            assert!(compact_cost(v).chars().count() <= 5, "{v} → {}", compact_cost(v));
        }
    }

    #[test]
    fn compact_token_tiers() {
        assert_eq!(compact_tokens(0), "0");
        assert_eq!(compact_tokens(950), "950");
        assert_eq!(compact_tokens(45_200), "45K");
        assert_eq!(compact_tokens(1_230_000), "1.2M");
        assert_eq!(compact_tokens(196_600_000), "197M");
        assert_eq!(compact_tokens(1_200_000_000), "1.2B");
        for v in [0i64, 999, 9_999, 999_999, 9_999_999, 999_999_999, 9_999_999_999] {
            assert!(compact_tokens(v).chars().count() <= 5, "{v} → {}", compact_tokens(v));
        }
    }

    #[test]
    fn renders_non_empty_bitmap() {
        let img = render_tray_text(&["$117".to_string()], 32).unwrap();
        assert_eq!(img.len(), 32 * 32 * 4);
        let white_pixels = img.chunks(4).filter(|p| p[3] == 255).count();
        assert!(white_pixels > 20, "expected visible glyph pixels, got {white_pixels}");
    }

    #[test]
    fn renders_two_lines() {
        let img = render_tray_text(&["$117".to_string(), "197M".to_string()], 32).unwrap();
        assert_eq!(img.len(), 32 * 32 * 4);
        // Pixels must exist in both halves (two stacked lines).
        let top: usize = img[..32 * 16 * 4].chunks(4).filter(|p| p[3] == 255).count();
        let bottom: usize = img[32 * 16 * 4..].chunks(4).filter(|p| p[3] == 255).count();
        assert!(top > 0 && bottom > 0, "top={top} bottom={bottom}");
    }

    #[test]
    fn empty_lines_return_none() {
        assert!(render_tray_text(&[], 32).is_none());
        assert!(render_tray_text(&[String::new()], 32).is_none());
    }
}
