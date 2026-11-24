//! Vetorização de artes (motor VTracer, nativo e offline) e rasterização do
//! SVG resultante de volta para a resolução de impressão.
//!
//! Substitui o ImageTracer (JS via CDN) do Montador DTF: aqui o motor é o
//! VTracer compilado no núcleo — mesma mecânica de "cores chapadas + redesenho
//! em alta resolução", sem depender de internet.

use visioncortex::PathSimplifyMode;
use vtracer::{ColorImage, ColorMode, Config, Hierarchical};

/// Resultado da vetorização: o texto SVG e o tamanho original da arte.
pub struct Vetor {
    pub svg: String,
    pub w: u32,
    pub h: u32,
}

/// Vetoriza um buffer RGBA8 em SVG de cores chapadas.
///
/// `precisao_cor` (1–8, padrão 6) controla quantas cores a vetorização separa:
/// valores maiores preservam mais tons, menores deixam a arte mais "chapada".
pub fn vetorizar(d: &[u8], w: usize, h: usize, precisao_cor: u8) -> Result<Vetor, String> {
    if d.len() != w * h * 4 {
        return Err("buffer RGBA inconsistente".into());
    }
    let img = ColorImage {
        pixels: d.to_vec(),
        width: w,
        height: h,
    };
    let cfg = Config {
        color_mode: ColorMode::Color,
        hierarchical: Hierarchical::Stacked,
        filter_speckle: 4,
        color_precision: precisao_cor.clamp(1, 8) as i32,
        layer_difference: 16,
        mode: PathSimplifyMode::Spline,
        corner_threshold: 60,
        length_threshold: 4.0,
        max_iterations: 10,
        splice_threshold: 45,
        path_precision: Some(2),
    };
    let svg = vtracer::convert(img, cfg).map_err(|e| format!("falha ao vetorizar: {e}"))?;
    Ok(Vetor {
        svg: svg.to_string(),
        w: w as u32,
        h: h as u32,
    })
}

/// Rasteriza um SVG (texto) para RGBA8 não-premultiplicado, escalando para
/// caber em `lado_max` no maior lado (o SVG é vetorial, então renderizar maior
/// = mais resolução de impressão de graça).
pub fn rasterizar_svg(svg: &str, lado_max: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(svg, &opt)
        .map_err(|e| format!("SVG inválido: {e}"))?;
    let size = tree.size();
    let (sw, sh) = (size.width(), size.height());
    if sw <= 0.0 || sh <= 0.0 {
        return Err("SVG sem dimensões".into());
    }
    let escala = if sw >= sh { lado_max as f32 / sw } else { lado_max as f32 / sh };
    let escala = escala.max(0.01);
    let w = (sw * escala).round().max(1.0) as u32;
    let h = (sh * escala).round().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("falha ao alocar imagem")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(escala, escala),
        &mut pixmap.as_mut(),
    );
    // tiny-skia guarda RGBA pré-multiplicado; desfaz para RGBA8 padrão.
    let mut px = pixmap.take();
    for c in px.chunks_exact_mut(4) {
        let a = c[3] as u32;
        if a > 0 && a < 255 {
            for k in 0..3 {
                c[k] = ((c[k] as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok((px, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quadrado() -> (Vec<u8>, usize, usize) {
        // 40×40: transparente com quadrado vermelho sólido no centro.
        let (w, h) = (40, 40);
        let mut d = vec![0u8; w * h * 4];
        for y in 10..30 {
            for x in 10..30 {
                let i = (y * w + x) * 4;
                d[i] = 200;
                d[i + 1] = 30;
                d[i + 2] = 40;
                d[i + 3] = 255;
            }
        }
        (d, w, h)
    }

    #[test]
    fn vetoriza_quadrado() {
        let (d, w, h) = quadrado();
        let v = vetorizar(&d, w, h, 6).unwrap();
        assert!(v.svg.contains("<svg"), "saída não é SVG: {}", &v.svg[..80.min(v.svg.len())]);
        assert!(v.svg.contains("path"), "SVG sem contornos");
        assert!(v.svg.to_lowercase().contains("c8"), "SVG sem o vermelho da arte");
    }

    #[test]
    fn rasteriza_svg_redondo() {
        let (d, w, h) = quadrado();
        let v = vetorizar(&d, w, h, 6).unwrap();
        let (px, rw, rh) = rasterizar_svg(&v.svg, 400).unwrap();
        assert_eq!((rw, rh), (400, 400));
        assert_eq!(px.len(), (rw * rh * 4) as usize);
        let i = ((200 * rw + 200) * 4) as usize;
        assert!(px[i] > 150 && px[i + 1] < 100, "centro deveria ser vermelho");
        let canto = 3;
        assert_eq!(px[canto], 0, "canto deveria ser transparente");
    }
}
