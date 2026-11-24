//! Testes do núcleo: encaixe retangular, encaixe por contorno, repetição
//! máxima e cores. Usam as mesmas medidas dos exemplos do Montador DTF.

use nucleo::contorno::{mask_from_alpha, pack_contorno, ContourItem, MASK_SUPERSAMPLE};
use nucleo::cores::*;
use nucleo::modelo::*;
use nucleo::repeticao::repeticao_maxima_rect;
use nucleo::retangular::{item_variants, pack_rect, PackItem};
use std::time::Duration;

fn settings() -> Settings {
    Settings::default().sanitized() // 58 cm, margem 0,5, gap 0,3, 300 dpi
}

/// As 3 artes de exemplo do Montador (addSamples): selo, triângulo e faixa.
fn sample_items() -> Vec<PackItem> {
    let s = settings();
    let specs = [
        ItemSpec { name: "selo".into(), w_cm: 15.0, h_cm: 15.0, qty: 6, rotate: true, fill: false, ratio: 1 },
        ItemSpec { name: "triângulo".into(), w_cm: 17.0, h_cm: 14.9, qty: 4, rotate: true, fill: false, ratio: 1 },
        ItemSpec { name: "faixa".into(), w_cm: 32.0, h_cm: 9.0, qty: 2, rotate: true, fill: false, ratio: 1 },
    ];
    specs
        .iter()
        .map(|sp| PackItem { spec: sp.clone(), variants: item_variants(sp, &s) })
        .collect()
}

fn overlaps(a: &Placement, b: &Placement) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn check_sheet(s: &Settings, res: &PackResult, total_qty: usize) {
    assert!(!res.sheets.is_empty(), "devia gerar ao menos uma folha");
    let margin = cm_to_px(s.margin_cm, s.dpi);
    let film_w = cm_to_px(s.width_cm, s.dpi);
    let max_len = cm_to_px(s.max_len_cm, s.dpi);
    let mut count = 0usize;
    for sh in &res.sheets {
        assert_eq!(sh.width_px, film_w);
        assert!(sh.length_px <= max_len, "folha passou do comprimento máximo");
        for p in &sh.placements {
            count += 1;
            assert!(p.x >= margin - 1, "peça invadiu a margem esquerda");
            assert!(p.y >= margin - 1, "peça invadiu a margem superior");
            assert!(p.x + p.w <= film_w - margin + 1, "peça passou da margem direita");
        }
        for i in 0..sh.placements.len() {
            for j in i + 1..sh.placements.len() {
                assert!(
                    !overlaps(&sh.placements[i], &sh.placements[j]),
                    "peças sobrepostas: {:?} x {:?}",
                    sh.placements[i],
                    sh.placements[j]
                );
            }
        }
    }
    assert_eq!(count, total_qty, "quantidade total de peças");
}

#[test]
fn retangular_encaixa_exemplos() {
    let s = settings();
    let items = sample_items();
    let res = pack_rect(&s, &items, Duration::from_millis(300));
    assert!(res.too_big.is_empty());
    check_sheet(&s, &res, 12);
    // Os exemplos do Montador cabem em uma única folha de 58 cm.
    assert_eq!(res.sheets.len(), 1);
}

#[test]
fn retangular_respeita_giro_desligado() {
    let mut s = settings();
    let sp = ItemSpec { name: "fixa".into(), w_cm: 50.0, h_cm: 10.0, qty: 2, rotate: false, fill: false, ratio: 1 };
    let items = vec![PackItem { spec: sp.clone(), variants: item_variants(&sp, &s) }];
    let res = pack_rect(&s, &items, Duration::from_millis(100));
    assert!(res.too_big.is_empty());
    for p in &res.sheets[0].placements {
        assert_eq!(p.angle, 0.0);
    }
    // Largura útil = 57 cm: duas peças de 50 cm não cabem lado a lado.
    s.grouped = false;
    assert!(res.sheets[0].length_px > cm_to_px(10.0, s.dpi));
}

#[test]
fn retangular_marcar_grande_demais() {
    let mut s = settings();
    s.max_len_cm = 100.0; // nem girada a arte cabe: 250 cm > 99 cm de folha
    let sp = ItemSpec { name: "gigante".into(), w_cm: 250.0, h_cm: 30.0, qty: 1, rotate: true, fill: false, ratio: 1 };
    let items = vec![PackItem { spec: sp.clone(), variants: item_variants(&sp, &s) }];
    let res = pack_rect(&s, &items, Duration::from_millis(50));
    assert_eq!(res.too_big, vec![0]);
    assert!(res.sheets.is_empty());
}

/// Máscara circular sintética de diâmetro `d_px` na resolução da célula.
fn circle_cand(d_cm: f64, s: &Settings, cell: f64, d: usize) -> (Variant, Cand) {
    let px = cm_to_px(d_cm, s.dpi);
    let v = variant_box(px, px, 0.0);
    let s_f = MASK_SUPERSAMPLE;
    let cw = ((v.w as f64 / cell).ceil() as usize).max(1);
    let ch = ((v.h as f64 / cell).ceil() as usize).max(1);
    let (w, h) = (cw * s_f, ch * s_f);
    let mut alpha = vec![0u8; w * h];
    let (cx, cy, r) = (w as f64 / 2.0, h as f64 / 2.0, w.min(h) as f64 / 2.0 - 1.0);
    for y in 0..h {
        for x in 0..w {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            if (dx * dx + dy * dy).sqrt() <= r {
                alpha[y * w + x] = 255;
            }
        }
    }
    let m = mask_from_alpha(&alpha, w, h, d);
    (v, Cand { m, members: vec![Member { v, dx: 0, dy: 0 }] })
}

#[test]
fn contorno_encaixa_circulos() {
    let s = settings();
    let cell = contour_cell(&s);
    let d = (s.gap_cm / CM_PER_IN * s.dpi / 2.0 / cell - 1e-9).ceil().max(0.0) as usize;
    let mut s2 = s.clone();
    s2.contour = true;

    let specs = [
        ItemSpec { name: "círculo P".into(), w_cm: 12.0, h_cm: 12.0, qty: 5, rotate: true, fill: false, ratio: 1 },
        ItemSpec { name: "círculo G".into(), w_cm: 20.0, h_cm: 20.0, qty: 3, rotate: true, fill: false, ratio: 1 },
    ];
    let items: Vec<ContourItem> = specs
        .iter()
        .map(|sp| {
            let (v, cand) = circle_cand(sp.w_cm, &s2, cell, d);
            let _ = v;
            ContourItem { spec: sp.clone(), cands: vec![cand] }
        })
        .collect();
    let res = pack_contorno(&s2, &items, Duration::from_millis(500), Some(cell));
    assert!(res.too_big.is_empty());
    let total: u32 = specs.iter().map(|sp| sp.qty).sum();
    let margin = cm_to_px(s2.margin_cm, s2.dpi);
    let film_w = cm_to_px(s2.width_cm, s2.dpi);
    let mut count = 0u32;
    // No contorno as caixas podem se sobrepor (as máscaras se encaixam);
    // o que não pode é duas máscaras ocuparem a mesma célula.
    let mut occupied: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    for sh in &res.sheets {
        for p in &sh.placements {
            count += 1;
            assert!(p.x >= margin - 1 && p.y >= margin - 1, "peça invadiu a margem");
            assert!(p.x + p.w <= film_w - margin + cell as i64 + 2, "peça passou da margem direita");
            let gx = ((p.x - margin) as f64 / cell).round() as i64;
            let gy = ((p.y - margin) as f64 / cell).round() as i64;
            let m = &items[p.item].cands[0].m;
            for cy in 0..m.h {
                for cx in 0..m.w {
                    if m.at(cx, cy) {
                        let key = (gx + cx as i64, gy + cy as i64);
                        assert!(occupied.insert(key), "máscaras sobrepostas na célula {key:?}");
                    }
                }
            }
        }
    }
    assert_eq!(count, total);
}

#[test]
fn repeticao_encontra_quantidade() {
    let s = settings();
    let sp = ItemSpec { name: "selo".into(), w_cm: 15.0, h_cm: 15.0, qty: 1, rotate: true, fill: true, ratio: 1 };
    let items = vec![PackItem { spec: sp.clone(), variants: item_variants(&sp, &s) }];
    let out = repeticao_maxima_rect(&s, &items, 100.0, false);
    assert!(!out.fixed_too_long);
    assert_eq!(out.quantities.len(), 1);
    let q = out.quantities[0];
    assert!(q > 1, "esperava várias cópias, veio {q}");
    // Largura útil 57 cm → 3 por linha; 100 cm → ~6 linhas (com espaçamento).
    assert!(q <= 24, "não pode passar do que cabe em 1 m: {q}");
    assert_eq!(out.result.sheets.len(), 1);
    assert!(out.result.sheets[0].length_px <= cm_to_px(100.0, s.dpi));
    let count: usize = out.result.sheets[0].placements.len();
    assert_eq!(count, q as usize);
}

#[test]
fn cores_blend_e_hex() {
    let g = blend_colors("#000000", "#ffffff", 3);
    assert_eq!(g.len(), 3);
    assert_eq!(g[0], "#000000");
    assert_eq!(g[2], "#ffffff");
    let mid = hex_to_rgb(&g[1]).unwrap();
    // O meio do gradiente em OKLab é ~99 (perceptualmente uniforme), não 128.
    assert!((90..=110).contains(&mid[0]), "cinza inesperado: {mid:?}");
    assert_eq!(mid[0], mid[1]);
    assert_eq!(mid[1], mid[2]);

    assert_eq!(hex_to_rgb("#00789e"), Some([0x00, 0x78, 0x9e]));
    assert_eq!(rgb_hex([0x00, 0x78, 0x9e]), "#00789e");
    assert!(color_gap([0, 0, 0], [0, 0, 0]) < 1e-9);
    assert!(color_gap([255, 255, 255], [0, 0, 0]) > 250.0);
}

#[test]
fn rng_mulberry32_sequencia_conhecida() {
    // Sequência de referência do mulberry32 com semente 42 (mesma do JS).
    let mut r = nucleo::rng::Rng::new(42);
    let v: Vec<f64> = (0..3).map(|_| r.next()).collect();
    let esperado = [0.6011037519, 0.4482905590, 0.8524657935];
    for (a, b) in v.iter().zip(esperado) {
        assert!((a - b).abs() < 1e-9, "rng divergiu: {a} vs {b}");
    }
}
