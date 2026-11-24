//! Cores: conversões sRGB ↔ OKLab, distância "redmean" e gradientes.
//! Portado do Montador DTF (toOklab/fromOklab/blendColors/colorGap).

use crate::modelo::clamp;

fn srgb_to_lin(v: f64) -> f64 {
    let v = v / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn lin_to_srgb(v: f64) -> u8 {
    let s = if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (255.0 * clamp(s, 0.0, 1.0)).round() as u8
}

pub fn to_oklab(rgb: [u8; 3]) -> [f64; 3] {
    let r = srgb_to_lin(rgb[0] as f64);
    let g = srgb_to_lin(rgb[1] as f64);
    let b = srgb_to_lin(rgb[2] as f64);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

pub fn from_oklab(lab: [f64; 3]) -> [u8; 3] {
    let (ll, aa, bb) = (lab[0], lab[1], lab[2]);
    let l = (ll + 0.3963377774 * aa + 0.2158037573 * bb).powi(3);
    let m = (ll - 0.1055613458 * aa - 0.0638541728 * bb).powi(3);
    let s = (ll - 0.0894841775 * aa - 1.2914855480 * bb).powi(3);
    [
        lin_to_srgb(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        lin_to_srgb(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        lin_to_srgb(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
    ]
}

/// "#rrggbb" → [r, g, b]. Aceita com ou sem '#'.
pub fn hex_to_rgb(h: &str) -> Option<[u8; 3]> {
    let h = h.strip_prefix('#').unwrap_or(h);
    if h.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

pub fn rgb_hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// Distância entre cores (redmean / 3), igual ao colorGap do Montador.
pub fn color_gap(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (a, b) = (
        [a[0] as f64, a[1] as f64, a[2] as f64],
        [b[0] as f64, b[1] as f64, b[2] as f64],
    );
    let rm = (a[0] + b[0]) / 2.0;
    let dr = a[0] - b[0];
    let dg = a[1] - b[1];
    let db = a[2] - b[2];
    ((2.0 + rm / 256.0) * dr * dr + 4.0 * dg * dg + (2.0 + (255.0 - rm) / 256.0) * db * db).sqrt() / 3.0
}

/// Gradiente de `n` cores entre duas cores hex, interpolando em OKLab.
pub fn blend_colors(from: &str, to: &str, n: usize) -> Vec<String> {
    let a = to_oklab(hex_to_rgb(from).unwrap_or([0, 0, 0]));
    let b = to_oklab(hex_to_rgb(to).unwrap_or([0, 0, 0]));
    (0..n)
        .map(|k| {
            let t = if n == 1 { 0.0 } else { k as f64 / (n - 1) as f64 };
            rgb_hex(from_oklab([
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]))
        })
        .collect()
}
