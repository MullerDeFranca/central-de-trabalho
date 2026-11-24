//! Editor de imagem (aba Tratar Imagem): remover fundo, remover/trocar cor,
//! monocromia, limiar de alpha, recorte do conteúdo e upscale com sharpen.
//! Portado fielmente das operações do editor do Montador DTF (OPS/borderColor/
//! eraseMask/upscale/alphaBox). Buffers RGBA8 intercalados.

use crate::modelo::clamp;

pub const MAX_EDIT_SIDE: u32 = 12000;

/// Distância euclidiana RGB (o `colorDist` do Montador).
#[inline]
pub fn color_dist(d: &[u8], i: usize, c: [u8; 3]) -> f64 {
    let dr = d[i] as f64 - c[0] as f64;
    let dg = d[i + 1] as f64 - c[1] as f64;
    let db = d[i + 2] as f64 - c[2] as f64;
    (dr * dr + dg * dg + db * db).sqrt()
}

/// Cor mais comum na borda da imagem (o "fundo"). Ignora pixels transparentes.
pub fn border_color(d: &[u8], w: usize, h: usize) -> Option<[u8; 3]> {
    use std::collections::HashMap;
    let mut buckets: HashMap<u16, (u64, u64, u64, u64)> = HashMap::new(); // key → (n, r, g, b)
    let mut add = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        if d[i + 3] < 128 {
            return;
        }
        let k = ((d[i] as u16 >> 4) << 8) | ((d[i + 1] as u16 >> 4) << 4) | (d[i + 2] as u16 >> 4);
        let b = buckets.entry(k).or_default();
        b.0 += 1;
        b.1 += d[i] as u64;
        b.2 += d[i + 1] as u64;
        b.3 += d[i + 2] as u64;
    };
    for x in 0..w {
        add(x, 0);
        add(x, h - 1);
    }
    for y in 0..h {
        add(0, y);
        add(w - 1, y);
    }
    buckets
        .values()
        .max_by_key(|b| b.0)
        .map(|&(n, r, g, b)| [(r / n) as u8, (g / n) as u8, (b / n) as u8])
}

/// Apaga os pixels marcados e suaviza a borda dos vizinhos com cor parecida.
pub fn erase_mask(d: &mut [u8], w: usize, h: usize, mask: &[u8], color: [u8; 3], tol: f64) {
    for i in 0..w * h {
        if mask[i] != 0 {
            d[i * 4 + 3] = 0;
        }
    }
    if tol <= 0.0 {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if mask[i] != 0 || d[i * 4 + 3] == 0 {
                continue;
            }
            let near = (x > 0 && mask[i - 1] != 0)
                || (x < w - 1 && mask[i + 1] != 0)
                || (y > 0 && mask[i - w] != 0)
                || (y < h - 1 && mask[i + w] != 0);
            if !near {
                continue;
            }
            let dd = color_dist(d, i * 4, color);
            if dd < tol * 2.0 {
                let f = clamp((dd - tol) / tol, 0.0, 1.0);
                d[i * 4 + 3] = (d[i * 4 + 3] as f64 * f).round() as u8;
            }
        }
    }
}

/// Remove o fundo por varredura a partir da borda (flood fill), com tolerância.
pub fn remove_background(d: &mut [u8], w: usize, h: usize, tol: f64) -> Result<(), String> {
    let color = border_color(d, w, h).ok_or("a borda da imagem já é transparente")?;
    let tol = tol.max(1.0);
    let mut mask = vec![0u8; w * h];
    let mut stack: Vec<usize> = Vec::with_capacity(w * 2 + h * 2);
    let pass = |i: usize| d[i * 4 + 3] < 8 || color_dist(d, i * 4, color) <= tol;
    macro_rules! push {
        ($i:expr) => {
            let i = $i;
            if mask[i] == 0 && pass(i) {
                mask[i] = 1;
                stack.push(i);
            }
        };
    }
    for x in 0..w {
        push!(x);
        push!((h - 1) * w + x);
    }
    for y in 0..h {
        push!(y * w);
        push!(y * w + w - 1);
    }
    while let Some(i) = stack.pop() {
        let x = i % w;
        if x > 0 {
            push!(i - 1);
        }
        if x < w - 1 {
            push!(i + 1);
        }
        if i >= w {
            push!(i - w);
        }
        if i < w * (h - 1) {
            push!(i + w);
        }
    }
    erase_mask(d, w, h, &mask, color, tol);
    Ok(())
}

/// Remove uma cor específica (onde quer que ela esteja).
pub fn remove_color(d: &mut [u8], w: usize, h: usize, color: [u8; 3], tol: f64) {
    let tol = tol.max(1.0);
    let mut mask = vec![0u8; w * h];
    for i in 0..w * h {
        if d[i * 4 + 3] != 0 && color_dist(d, i * 4, color) <= tol {
            mask[i] = 1;
        }
    }
    erase_mask(d, w, h, &mask, color, tol);
}

/// Troca uma cor, mantendo a variação de tom (sombras/antisserrilhado).
pub fn replace_color(d: &mut [u8], from: [u8; 3], to: [u8; 3], tol: f64) {
    let tol = tol.max(1.0);
    for i in (0..d.len()).step_by(4) {
        if d[i + 3] != 0 && color_dist(d, i, from) <= tol {
            for k in 0..3 {
                d[i + k] = clamp(to[k] as f64 + d[i + k] as f64 - from[k] as f64, 0.0, 255.0) as u8;
            }
        }
    }
}

/// Pinta todos os pixels opacos com uma cor única.
pub fn mono(d: &mut [u8], to: [u8; 3]) {
    for i in (0..d.len()).step_by(4) {
        if d[i + 3] != 0 {
            d[i] = to[0];
            d[i + 1] = to[1];
            d[i + 2] = to[2];
        }
    }
}

/// Limiar de alpha: vira 255 ou 0.
pub fn alpha_threshold(d: &mut [u8], t: u8) {
    for i in (3..d.len()).step_by(4) {
        d[i] = if d[i] >= t { 255 } else { 0 };
    }
}

/// Caixa delimitadora dos pixels não transparentes.
pub fn alpha_box(d: &[u8], w: usize, h: usize) -> Option<(usize, usize, usize, usize)> {
    let (mut min_x, mut min_y) = (w, h);
    let (mut max_x, mut max_y) = (0i64, 0i64);
    for y in 0..h {
        for x in 0..w {
            if d[(y * w + x) * 4 + 3] != 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x as i64);
                max_y = max_y.max(y as i64);
            }
        }
    }
    if max_x < 0 {
        None
    } else {
        Some((min_x, min_y, (max_x as usize) - min_x + 1, (max_y as usize) - min_y + 1))
    }
}

/// Recorta a imagem na caixa do conteúdo (alpha > 0).
pub fn crop_alpha(d: &[u8], w: usize, h: usize) -> Option<(Vec<u8>, usize, usize)> {
    let (x, y, cw, ch) = alpha_box(d, w, h)?;
    if (x, y, cw, ch) == (0, 0, w, h) {
        return Some((d.to_vec(), w, h));
    }
    let mut out = vec![0u8; cw * ch * 4];
    for row in 0..ch {
        let src = ((y + row) * w + x) * 4;
        out[row * cw * 4..(row + 1) * cw * 4].copy_from_slice(&d[src..src + cw * 4]);
    }
    Some((out, cw, ch))
}

/// Redimensionamento bilinear em RGBA.
pub fn resize_bilinear(d: &[u8], w: usize, h: usize, nw: usize, nh: usize) -> Vec<u8> {
    let mut out = vec![0u8; nw * nh * 4];
    let sx = w as f64 / nw as f64;
    let sy = h as f64 / nh as f64;
    for y in 0..nh {
        let fy = (y as f64 + 0.5) * sy - 0.5;
        let y0 = fy.floor().max(0.0) as usize;
        let y1 = (y0 + 1).min(h - 1);
        let ty = (fy - y0 as f64).clamp(0.0, 1.0);
        for x in 0..nw {
            let fx = (x as f64 + 0.5) * sx - 0.5;
            let x0 = fx.floor().max(0.0) as usize;
            let x1 = (x0 + 1).min(w - 1);
            let tx = (fx - x0 as f64).clamp(0.0, 1.0);
            let o = (y * nw + x) * 4;
            let (a, b, c, e) = ((y0 * w + x0) * 4, (y0 * w + x1) * 4, (y1 * w + x0) * 4, (y1 * w + x1) * 4);
            for k in 0..4 {
                let top = d[a + k] as f64 * (1.0 - tx) + d[b + k] as f64 * tx;
                let bot = d[c + k] as f64 * (1.0 - tx) + d[e + k] as f64 * tx;
                out[o + k] = (top * (1.0 - ty) + bot * ty).round() as u8;
            }
        }
    }
    out
}

/// Desfoque de caixa 3×3 (aproxima o `blur(1px)` do canvas).
fn blur1(d: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 4;
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (xx, yy) = (x as i64 + dx, y as i64 + dy);
                    if xx < 0 || yy < 0 || xx >= w as i64 || yy >= h as i64 {
                        continue;
                    }
                    let i = (yy as usize * w + xx as usize) * 4;
                    for k in 0..4 {
                        acc[k] += d[i + k] as u32;
                    }
                    n += 1;
                }
            }
            for k in 0..4 {
                out[o + k] = (acc[k] / n) as u8;
            }
        }
    }
    out
}

/// Unsharp mask: realça a diferença contra o desfoque.
pub fn sharpen(d: &mut [u8], w: usize, h: usize, amount: f64) {
    let bl = blur1(d, w, h);
    for i in (0..d.len()).step_by(4) {
        for k in 0..3 {
            let v = d[i + k] as f64 + amount * (d[i + k] as f64 - bl[i + k] as f64);
            d[i + k] = clamp(v, 0.0, 255.0) as u8;
        }
    }
}

/// Upscale em passos de até 2× + sharpen 0,6 (o `upscale` do Montador).
pub fn upscale(d: &[u8], w: usize, h: usize, f: f64) -> Result<(Vec<u8>, usize, usize), String> {
    let f = f.min(MAX_EDIT_SIDE as f64 / w.max(h) as f64);
    if f < 1.05 {
        return Err(format!("a imagem já está no tamanho máximo ({MAX_EDIT_SIDE}px)"));
    }
    let mut buf = d.to_vec();
    let (mut cw, mut ch) = (w, h);
    let mut cur = 1.0;
    while cur < f - 1e-6 {
        let step = (2.0f64).min(f / cur);
        let nw = ((cw as f64 * step).round() as usize).max(1);
        let nh = ((ch as f64 * step).round() as usize).max(1);
        buf = resize_bilinear(&buf, cw, ch, nw, nh);
        cw = nw;
        ch = nh;
        cur *= step;
    }
    sharpen(&mut buf, cw, ch, 0.6);
    Ok((buf, cw, ch))
}
