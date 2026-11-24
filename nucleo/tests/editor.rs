//! Testes das operações do editor (Tratar Imagem).

use nucleo::editor::*;

/// Fundo branco 20×20 com um quadrado vermelho 8×8 no centro.
fn imagem_teste() -> (Vec<u8>, usize, usize) {
    let (w, h) = (20usize, 20usize);
    let mut d = vec![255u8; w * h * 4];
    for y in 6..14 {
        for x in 6..14 {
            let i = (y * w + x) * 4;
            d[i] = 220;
            d[i + 1] = 30;
            d[i + 2] = 30;
        }
    }
    (d, w, h)
}

#[test]
fn border_color_detecta_branco() {
    let (d, w, h) = imagem_teste();
    assert_eq!(border_color(&d, w, h), Some([255, 255, 255]));
}

#[test]
fn remove_fundo_preserva_centro() {
    let (mut d, w, h) = imagem_teste();
    remove_background(&mut d, w, h, 30.0).unwrap();
    // Canto ficou transparente.
    assert_eq!(d[3], 0);
    // Centro continua opaco e vermelho.
    let i = (10 * w + 10) * 4;
    assert_eq!(d[i + 3], 255);
    assert!(d[i] > 200 && d[i + 1] < 80);
    // A caixa de conteúdo encolheu para o quadrado central.
    let (x, y, cw, ch) = alpha_box(&d, w, h).unwrap();
    assert!((x, y) == (6, 6) && (cw, ch) == (8, 8), "caixa: {x},{y} {cw}x{ch}");
}

#[test]
fn remove_cor_especifica() {
    let (mut d, w, h) = imagem_teste();
    remove_color(&mut d, w, h, [220, 30, 30], 30.0);
    let i = (10 * w + 10) * 4;
    assert_eq!(d[i + 3], 0, "vermelho devia sumir");
    assert_eq!(d[3], 255, "fundo branco fica");
}

#[test]
fn troca_cor_preserva_tom() {
    let (mut d, w, h) = imagem_teste();
    // Um pixel vermelho mais escuro (sombra) perto do centro.
    let i = (8 * w + 8) * 4;
    d[i] = 180;
    d[i + 1] = 20;
    d[i + 2] = 20;
    replace_color(&mut d, [220, 30, 30], [0, 0, 200], 60.0);
    // Centro: 220→0, 30→0(+0-30 clamp? não: 0+30-30=0... verifica deslocamento)
    let c = (10 * w + 10) * 4;
    assert_eq!([d[c], d[c + 1], d[c + 2]], [0, 0, 200]);
    // Sombra: 180+0-220 = -40 → 0; azul: 20+200-30=190 → mais escuro que 200? (clamped)
    assert!(d[i + 2] < 200, "tom preservado: {}", d[i + 2]);
}

#[test]
fn mono_e_limiar() {
    let (mut d, w, h) = imagem_teste();
    mono(&mut d, [10, 20, 30]);
    let i = (10 * w + 10) * 4;
    assert_eq!([d[i], d[i + 1], d[i + 2]], [10, 20, 30]);
    alpha_threshold(&mut d, 200);
    assert!(d[i + 3] == 255);
}

#[test]
fn upscale_dobra_e_recorta() {
    let (d, w, h) = imagem_teste();
    let (mut up, nw, nh) = upscale(&d, w, h, 2.0).unwrap();
    assert_eq!((nw, nh), (40, 40));
    // Recorta depois de remover o fundo (a imagem de teste é toda opaca).
    remove_background(&mut up, nw, nh, 30.0).unwrap();
    let (crop, cw, ch) = crop_alpha(&up, nw, nh).unwrap();
    // Bilinear borra ~1 px de borda: a caixa fica entre 16 e 18.
    assert!((16..=18).contains(&cw) && (16..=18).contains(&ch), "caixa: {cw}x{ch}");
    assert_eq!(crop.len(), cw * ch * 4);
    // Uma imagem já no tamanho máximo dá erro, sem alocar nada gigante.
    let grande = vec![255u8; 12000 * 4 * 4];
    assert!(upscale(&grande, 12000, 4, 2.0).is_err());
}
