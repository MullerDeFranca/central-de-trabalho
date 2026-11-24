//! Bancada — app de desktop (Tauri 2) que expõe o núcleo de encaixe à interface.
//!
//! Comandos: importar artes, encaixar (retangular/contorno), repetição máxima,
//! exportar PNG com dpi embutido, e a biblioteca central (clientes + logos).

use base64::Engine;
use image::GenericImageView;
use nucleo::contorno::{mask_from_alpha, pack_contorno, ContourItem, MASK_SUPERSAMPLE};
use nucleo::modelo::*;
use nucleo::repeticao::{repeticao_maxima_contorno, repeticao_maxima_rect, FillOutcome};
use nucleo::retangular::{item_variants, pack_rect, PackItem};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::Manager;

mod pdf;

const PACK_BUDGET_MS: u64 = 300;
const CONTOUR_BUDGET_MS: u64 = 1500;

struct Imagem {
    rgba: image::RgbaImage,
    dpi: Option<f64>,
    nome: String,
}

struct EditorSessao {
    work: image::RgbaImage,
    nome: String,
    undo: Vec<image::RgbaImage>,
    svg: Option<String>, // SVG da última vetorização (invalidado por edições de pixel)
}

struct AppState {
    imagens: HashMap<u64, Imagem>,
    proximo_id: u64,
    ultimo: Option<(Settings, Vec<ItemEntrada>, PackResult)>,
    editor: Option<EditorSessao>,
    moldes: Vec<MoldeEstado>,
    db: Connection,
    dados_dir: PathBuf,
    config_path: PathBuf,
}

/// Molde de sublimação: página de PDF renderizada (RGBA) + análise em cache.
struct MoldeEstado {
    nome: String,
    rgba: Vec<u8>,
    w: u32,
    h: u32,
    ppm: f64,
    view: [f64; 4],
    cache: Option<(String, Vec<nucleo::moldes::Parte>)>,
}

#[derive(Deserialize)]
struct ArteUpload {
    name: String,
    b64: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArteInfo {
    id: u64,
    name: String,
    w: u32,
    h: u32,
    dpi: Option<f64>,
    cm_w: f64,
    cm_h: f64,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemEntrada {
    id: u64,
    name: String,
    w_cm: f64,
    h_cm: f64,
    qty: u32,
    rotate: bool,
    #[serde(default)]
    fill: bool,
    #[serde(default = "um")]
    ratio: u32,
}

fn um() -> u32 {
    1
}

/// dpi declarado no PNG (chunk pHYs), se houver.
fn png_dpi(bytes: &[u8]) -> Option<f64> {
    if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut i = 8usize;
    while i + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[i..i + 4].try_into().ok()?) as usize;
        let kind = &bytes[i + 4..i + 8];
        if kind == b"pHYs" && len >= 9 && i + 8 + 9 <= bytes.len() {
            let xppu = u32::from_be_bytes(bytes[i + 8..i + 12].try_into().ok()?);
            let unit = bytes[i + 16];
            if unit == 1 && xppu > 0 {
                return Some(xppu as f64 * 0.0254);
            }
            return None;
        }
        if kind == b"IDAT" {
            return None;
        }
        i += 12 + len;
    }
    None
}

#[tauri::command]
fn importar_artes(
    state: tauri::State<Mutex<AppState>>,
    artes: Vec<ArteUpload>,
) -> Result<Vec<ArteInfo>, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for a in artes {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&a.b64)
            .map_err(|e| format!("{}: base64 inválido ({e})", a.name))?;
        let dpi = png_dpi(&bytes);
        let img = image::load_from_memory(&bytes)
            .map_err(|e| format!("{}: não consegui abrir ({e})", a.name))?;
        let (w, h) = img.dimensions();
        let id = st.proximo_id;
        st.proximo_id += 1;
        st.imagens.insert(id, Imagem { rgba: img.to_rgba8(), dpi, nome: a.name.clone() });
        // Tamanho natural em cm: usa o dpi do arquivo, senão 300 dpi (padrão DTF).
        let d = dpi.unwrap_or(300.0);
        out.push(ArteInfo {
            id,
            name: a.name,
            w,
            h,
            dpi,
            cm_w: px_to_cm(w as f64, d),
            cm_h: px_to_cm(h as f64, d),
        });
    }
    Ok(out)
}

/// Monta os candidatos de contorno de uma arte: uma máscara por ângulo.
fn cands_para(img: &image::RgbaImage, spec: &ItemSpec, s: &Settings, cell: f64, d: usize) -> Vec<Cand> {
    let iw = cm_to_px(spec.w_cm, s.dpi);
    let ih = cm_to_px(spec.h_cm, s.dpi);
    angles_for(spec.rotate, s)
        .into_iter()
        .map(|a| {
            let v = variant_box(iw, ih, a);
            // Imagem já girada (múltiplos de 90°; outros ângulos usam a caixa sem girar).
            let rot = rotacionar(img, a);
            let s_am = MASK_SUPERSAMPLE;
            let cw = ((v.w as f64 / cell).ceil() as usize).max(1);
            let ch = ((v.h as f64 / cell).ceil() as usize).max(1);
            let (tw, th) = ((cw * s_am) as u32, (ch * s_am) as u32);
            let mini = image::imageops::resize(
                &rot,
                tw,
                th,
                image::imageops::FilterType::Triangle,
            );
            let alpha: Vec<u8> = mini.pixels().map(|p| p[3]).collect();
            let m = mask_from_alpha(&alpha, cw * s_am, ch * s_am, d);
            Cand { m, members: vec![Member { v, dx: 0, dy: 0 }] }
        })
        .collect()
}

fn rotacionar(img: &image::RgbaImage, angle: f64) -> image::RgbaImage {
    let a = angle.rem_euclid(360.0);
    if (a - 90.0).abs() < 1e-9 {
        image::imageops::rotate90(img)
    } else if (a - 180.0).abs() < 1e-9 {
        image::imageops::rotate180(img)
    } else if (a - 270.0).abs() < 1e-9 {
        image::imageops::rotate270(img)
    } else {
        img.clone()
    }
}

fn spec_de(e: &ItemEntrada) -> ItemSpec {
    ItemSpec {
        name: e.name.clone(),
        w_cm: e.w_cm,
        h_cm: e.h_cm,
        qty: e.qty,
        rotate: e.rotate,
        fill: e.fill,
        ratio: e.ratio,
    }
}

fn encaixar_com(
    st: &AppState,
    s: &Settings,
    itens: &[ItemEntrada],
    budget: Duration,
) -> Result<PackResult, String> {
    if s.contour {
        let cell = contour_cell(s);
        let d = (s.gap_cm / CM_PER_IN * s.dpi / 2.0 / cell - 1e-9).ceil().max(0.0) as usize;
        let mut cis = Vec::new();
        for e in itens {
            let img = st
                .imagens
                .get(&e.id)
                .ok_or_else(|| format!("arte {} não importada", e.name))?;
            cis.push(ContourItem {
                spec: spec_de(e),
                cands: cands_para(&img.rgba, &spec_de(e), s, cell, d),
            });
        }
        Ok(pack_contorno(s, &cis, budget, Some(cell)))
    } else {
        let pis: Vec<PackItem> = itens
            .iter()
            .map(|e| {
                let spec = spec_de(e);
                PackItem { variants: item_variants(&spec, s), spec }
            })
            .collect();
        Ok(pack_rect(s, &pis, budget))
    }
}

#[tauri::command]
fn encaixar(
    state: tauri::State<Mutex<AppState>>,
    settings: Settings,
    itens: Vec<ItemEntrada>,
) -> Result<PackResult, String> {
    let s = settings.sanitized();
    let budget = if s.contour {
        Duration::from_millis(CONTOUR_BUDGET_MS)
    } else {
        Duration::from_millis(PACK_BUDGET_MS)
    };
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let mut res = encaixar_com(&st, &s, &itens, budget)?;
    // Alguma arte marcada como "repetição máxima"? Usa a busca binária.
    if itens.iter().any(|e| e.fill && e.ratio > 0) {
        let len_cm = s.max_len_cm.min(300.0); // teto de repetição padrão: 3 m
        let out: FillOutcome = if s.contour {
            let cell = contour_cell(&s);
            let d = (s.gap_cm / CM_PER_IN * s.dpi / 2.0 / cell - 1e-9).ceil().max(0.0) as usize;
            let mut cis = Vec::new();
            for e in &itens {
                let img = st.imagens.get(&e.id).ok_or("arte não importada")?;
                cis.push(ContourItem {
                    spec: spec_de(e),
                    cands: cands_para(&img.rgba, &spec_de(e), &s, cell, d),
                });
            }
            repeticao_maxima_contorno(&s, &cis, len_cm, false, Some(cell))
        } else {
            let pis: Vec<PackItem> = itens
                .iter()
                .map(|e| {
                    let spec = spec_de(e);
                    PackItem { variants: item_variants(&spec, &s), spec }
                })
                .collect();
            repeticao_maxima_rect(&s, &pis, len_cm, false)
        };
        res = out.result;
        // Devolve as quantidades encontradas para a interface atualizar.
        let mut k = 0usize;
        let mut itens2 = itens.clone();
        for e in itens2.iter_mut() {
            if e.fill && e.ratio > 0 {
                e.qty = out.quantities[k];
                k += 1;
            }
        }
        st.ultimo = Some((s, itens2, res.clone()));
        return Ok(res);
    }
    st.ultimo = Some((s, itens, res.clone()));
    Ok(res)
}

#[tauri::command]
fn exportar_png(state: tauri::State<Mutex<AppState>>, pasta: String) -> Result<Vec<String>, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    let (s, itens, res) = st.ultimo.as_ref().ok_or("nada para exportar: encaixe primeiro")?;
    let mut arquivos = Vec::new();
    for (i, sheet) in res.sheets.iter().enumerate() {
        let nome = if res.sheets.len() > 1 {
            format!("folha-{}.png", i + 1)
        } else {
            "folha.png".to_string()
        };
        let caminho = PathBuf::from(&pasta).join(&nome);
        exportar_folha(&st, s, itens, sheet, &caminho)?;
        arquivos.push(caminho.to_string_lossy().into_owned());
    }
    Ok(arquivos)
}

/// Rasteriza a peça posicionada: arte girada (e espelhada, se o filme pede)
/// no tamanho em px da resolução do filme.
fn raster_da_peca(
    st: &AppState,
    itens: &[ItemEntrada],
    p: &Placement,
    mirror: bool,
) -> Result<image::RgbaImage, String> {
    let entrada = &itens[p.item];
    let img = st
        .imagens
        .get(&entrada.id)
        .ok_or_else(|| format!("arte {} não importada", entrada.name))?;
    let mut rot = rotacionar(&img.rgba, p.angle);
    if mirror {
        rot = image::imageops::flip_horizontal(&rot);
    }
    Ok(image::imageops::resize(
        &rot,
        p.iw.max(1) as u32,
        p.ih.max(1) as u32,
        image::imageops::FilterType::Lanczos3,
    ))
}

fn exportar_folha(
    st: &AppState,
    s: &Settings,
    itens: &[ItemEntrada],
    sheet: &Sheet,
    caminho: &PathBuf,
) -> Result<(), String> {
    let (w, h) = (sheet.width_px as u32, sheet.length_px as u32);
    let file = std::fs::File::create(caminho).map_err(|e| e.to_string())?;
    let wref = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(wref, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let ppu = (s.dpi / 0.0254).round() as u32;
    enc.set_pixel_dims(Some(png::PixelDimensions {
        xppu: ppu,
        yppu: ppu,
        unit: png::Unit::Meter,
    }));
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;

    // Desenha em faixas de ~16M px (como o Montador) para não explodir a memória.
    let faixa = ((16_000_000u64 / w.max(1) as u64) as u32).max(1).min(h);
    let mut y0 = 0u32;
    while y0 < h {
        let y1 = (y0 + faixa).min(h);
        let mut buf = vec![0u8; (w * (y1 - y0) * 4) as usize];
        for p in &sheet.placements {
            let alvo = raster_da_peca(st, itens, p, s.mirror)?;
            let (aw, ah) = (alvo.width() as i64, alvo.height() as i64);
            let dx = p.x + p.cx.round() as i64 - aw / 2;
            let dy = p.y + p.cy.round() as i64 - ah / 2;
            for iy in 0..ah {
                let fy = dy + iy;
                if fy < y0 as i64 || fy >= y1 as i64 {
                    continue;
                }
                for ix in 0..aw {
                    let fx = dx + ix;
                    if fx < 0 || fx >= w as i64 {
                        continue;
                    }
                    let px = alvo.get_pixel(ix as u32, iy as u32);
                    if px[3] < 13 {
                        continue; // dropGhosts
                    }
                    let o = (((fy - y0 as i64) as u32 * w + fx as u32) * 4) as usize;
                    buf[o] = px[0];
                    buf[o + 1] = px[1];
                    buf[o + 2] = px[2];
                    buf[o + 3] = px[3];
                }
            }
        }
        writer.write_image_data(&buf).map_err(|e| e.to_string())?;
        y0 = y1;
    }
    writer.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Exporta a montagem como PDF (uma página por folha, medidas exatas em pontos).
#[tauri::command]
fn exportar_pdf(state: tauri::State<Mutex<AppState>>, pasta: String) -> Result<Vec<String>, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    let (s, itens, res) = st.ultimo.as_ref().ok_or("nada para exportar: encaixe primeiro")?;
    let k = 72.0 / s.dpi;

    // Uma PdfImagem por (arte, ângulo) — compartilhada entre as cópias.
    let mut imagens: Vec<pdf::PdfImagem> = Vec::new();
    let mut mapa: HashMap<(usize, i64), usize> = HashMap::new();
    let mut paginas: Vec<pdf::PdfPagina> = Vec::new();

    for sheet in &res.sheets {
        let h_pt = sheet.length_px as f64 * k;
        let mut draws = Vec::new();
        for p in &sheet.placements {
            let chave = (p.item, p.angle.to_bits() as i64);
            let img_idx = match mapa.get(&chave) {
                Some(&i) => i,
                None => {
                    let r = raster_da_peca(&st, itens, p, s.mirror)?;
                    let (w, h) = (r.width(), r.height());
                    let raw = r.as_raw();
                    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
                    let mut alpha = Vec::with_capacity((w * h) as usize);
                    for px in raw.chunks_exact(4) {
                        // dropGhosts: alpha < 13 zera o pixel.
                        let a = if px[3] < 13 { 0 } else { px[3] };
                        if a == 0 {
                            rgb.extend_from_slice(&[0, 0, 0]);
                        } else {
                            rgb.extend_from_slice(&px[..3]);
                        }
                        alpha.push(a);
                    }
                    imagens.push(pdf::PdfImagem {
                        nome: format!("Im{}", imagens.len()),
                        w,
                        h,
                        rgb,
                        alpha,
                    });
                    mapa.insert(chave, imagens.len() - 1);
                    imagens.len() - 1
                }
            };
            draws.push(pdf::PdfDraw {
                img: img_idx,
                cx: (p.x as f64 + p.cx) * k,
                cy: h_pt - (p.y as f64 + p.cy) * k,
                w: p.iw as f64 * k,
                h: p.ih as f64 * k,
                angle: p.angle,
                mirror: s.mirror,
            });
        }
        paginas.push(pdf::PdfPagina { w_pt: sheet.width_px as f64 * k, h_pt, draws });
    }

    let bytes = pdf::gerar(&paginas, &imagens);
    let caminho = PathBuf::from(&pasta).join("montagem.pdf");
    std::fs::write(&caminho, &bytes).map_err(|e| e.to_string())?;
    Ok(vec![caminho.to_string_lossy().into_owned()])
}

/// Substitui as peças de uma folha após o ajuste manual (arrastar/girar/excluir).
#[tauri::command]
fn ajustar_layout(
    state: tauri::State<Mutex<AppState>>,
    folha: usize,
    placements: Vec<Placement>,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let (s, _, res) = st.ultimo.as_mut().ok_or("nada para ajustar: encaixe primeiro")?;
    let sheet = res.sheets.get_mut(folha).ok_or("folha inexistente")?;
    let margin = cm_to_px(s.margin_cm, s.dpi);
    let mut bottom = 0i64;
    for p in &placements {
        bottom = bottom.max(p.y + p.h);
    }
    sheet.placements = placements;
    sheet.length_px = bottom + margin;
    Ok(())
}

/* ------------------------- Folhas salvas (montagens) ------------------------- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FolhaCard {
    id: i64,
    nome: String,
    cliente: Option<String>,
    atualizado_em: i64,
    preview: Option<String>, // data URL da miniatura
    textos: String,
    hashes: Vec<String>, // dHash 64 bits de cada logo (busca por imagem)
}

fn agora_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Extrai os dHashes das logos dentro do backup JSON.
fn hashes_do_backup(dados: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(dados) else { return Vec::new() };
    v.get("items")
        .and_then(|i| i.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|it| it.get("dh").and_then(|d| d.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn folhas_salvar(
    state: tauri::State<Mutex<AppState>>,
    id: Option<i64>,
    nome: String,
    cliente: Option<String>,
    dados: String,
    textos: String,
    preview_b64: Option<String>,
) -> Result<i64, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    let dir = st.dados_dir.join("folhas");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // Grava a miniatura em disco.
    let mut preview_arq: Option<String> = None;
    if let Some(b64) = preview_b64 {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .map_err(|e| e.to_string())?;
        let arq = format!("folha-{}.png", agora_ms());
        std::fs::write(dir.join(&arq), &bytes).map_err(|e| e.to_string())?;
        preview_arq = Some(arq);
    }

    let agora = agora_ms();
    if let Some(id) = id {
        // Atualiza: mantém a miniatura antiga se não veio nova.
        if preview_arq.is_none() {
            preview_arq = st
                .db
                .query_row("SELECT preview FROM folhas WHERE id = ?1", params![id], |r| r.get(0))
                .ok();
        } else if let Ok(Some(antiga)) = st
            .db
            .query_row("SELECT preview FROM folhas WHERE id = ?1", params![id], |r| r.get::<_, Option<String>>(0))
        {
            let _ = std::fs::remove_file(dir.join(antiga));
        }
        st.db
            .execute(
                "UPDATE folhas SET nome=?1, cliente=?2, atualizado_em=?3, dados=?4, textos=?5, preview=?6 WHERE id=?7",
                params![nome.trim(), cliente, agora, dados, textos, preview_arq, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(id)
    } else {
        st.db
            .execute(
                "INSERT INTO folhas (nome, cliente, criado_em, atualizado_em, dados, textos, preview) VALUES (?1,?2,?3,?3,?4,?5,?6)",
                params![nome.trim(), cliente, agora, dados, textos, preview_arq],
            )
            .map_err(|e| e.to_string())?;
        Ok(st.db.last_insert_rowid())
    }
}

#[tauri::command]
fn folhas_list(
    state: tauri::State<Mutex<AppState>>,
    busca: Option<String>,
) -> Result<Vec<FolhaCard>, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    let like = format!("%{}%", busca.unwrap_or_default().to_lowercase());
    let mut q = st
        .db
        .prepare(
            "SELECT id, nome, cliente, atualizado_em, dados, COALESCE(textos,''), preview FROM folhas
             WHERE lower(nome) LIKE ?1 OR lower(COALESCE(textos,'')) LIKE ?1 OR lower(COALESCE(cliente,'')) LIKE ?1
             ORDER BY atualizado_em DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = q
        .query_map(params![like], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows.flatten() {
        let (id, nome, cliente, atualizado_em, dados, textos, preview) = r;
        let preview_url = preview.and_then(|arq| {
            std::fs::read(st.dados_dir.join("folhas").join(arq)).ok().map(|b| {
                format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))
            })
        });
        out.push(FolhaCard {
            id,
            nome,
            cliente,
            atualizado_em,
            preview: preview_url,
            textos,
            hashes: hashes_do_backup(&dados),
        });
    }
    Ok(out)
}

#[tauri::command]
fn folhas_abrir(state: tauri::State<Mutex<AppState>>, id: i64) -> Result<String, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    st.db
        .query_row("SELECT dados FROM folhas WHERE id = ?1", params![id], |r| r.get(0))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn folhas_excluir(state: tauri::State<Mutex<AppState>>, id: i64) -> Result<(), String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    if let Ok(Some(arq)) = st
        .db
        .query_row("SELECT preview FROM folhas WHERE id = ?1", params![id], |r| r.get::<_, Option<String>>(0))
    {
        let _ = std::fs::remove_file(st.dados_dir.join("folhas").join(arq));
    }
    st.db
        .execute("DELETE FROM folhas WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Abre uma montagem salva direto na memória (layout exato, sem recalcular).
#[tauri::command]
fn abrir_montagem(
    state: tauri::State<Mutex<AppState>>,
    settings: Settings,
    itens: Vec<ItemEntrada>,
    folhas: Vec<Vec<Placement>>,
) -> Result<(), String> {
    let s = settings.sanitized();
    let margin = cm_to_px(s.margin_cm, s.dpi);
    let film_w = cm_to_px(s.width_cm, s.dpi);
    let sheets = folhas
        .into_iter()
        .map(|placements| to_sheet(placements, film_w, margin))
        .collect();
    let res = PackResult { sheets, too_big: Vec::new(), cell: contour_cell(&s) };
    let mut st = state.lock().map_err(|e| e.to_string())?;
    st.ultimo = Some((s, itens, res));
    Ok(())
}

/* ------------------------- Biblioteca central (SQLite) ------------------------- */

#[derive(Serialize)]
struct Cliente {
    id: i64,
    nome: String,
}

#[derive(Serialize)]
struct Logo {
    id: i64,
    cliente_id: Option<i64>,
    nome: String,
    arquivo: String,
}

#[tauri::command]
fn clientes_list(state: tauri::State<Mutex<AppState>>) -> Result<Vec<Cliente>, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    let mut q = st
        .db
        .prepare("SELECT id, nome FROM clientes ORDER BY nome")
        .map_err(|e| e.to_string())?;
    let rows = q
        .query_map([], |r| Ok(Cliente { id: r.get(0)?, nome: r.get(1)? }))
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[tauri::command]
fn clientes_add(state: tauri::State<Mutex<AppState>>, nome: String) -> Result<i64, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    st.db
        .execute("INSERT INTO clientes (nome) VALUES (?1)", params![nome.trim()])
        .map_err(|e| e.to_string())?;
    Ok(st.db.last_insert_rowid())
}

#[tauri::command]
fn logos_list(
    state: tauri::State<Mutex<AppState>>,
    cliente_id: Option<i64>,
) -> Result<Vec<Logo>, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    let (sql, p): (&str, Vec<rusqlite::types::Value>) = match cliente_id {
        Some(c) => (
            "SELECT id, cliente_id, nome, arquivo FROM logos WHERE cliente_id = ?1 ORDER BY nome",
            vec![c.into()],
        ),
        None => ("SELECT id, cliente_id, nome, arquivo FROM logos ORDER BY nome", vec![]),
    };
    let mut q = st.db.prepare(sql).map_err(|e| e.to_string())?;
    let rows = q
        .query_map(rusqlite::params_from_iter(p), |r| {
            Ok(Logo { id: r.get(0)?, cliente_id: r.get(1)?, nome: r.get(2)?, arquivo: r.get(3)? })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[tauri::command]
fn logos_add(
    state: tauri::State<Mutex<AppState>>,
    cliente_id: Option<i64>,
    nome: String,
    b64: String,
) -> Result<i64, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .map_err(|e| e.to_string())?;
    let st = state.lock().map_err(|e| e.to_string())?;
    let dir = st.dados_dir.join("logos");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let seguro: String = nome
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let arquivo = format!("{}-{}.png", seguro, std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0));
    std::fs::write(dir.join(&arquivo), &bytes).map_err(|e| e.to_string())?;
    st.db
        .execute(
            "INSERT INTO logos (cliente_id, nome, arquivo) VALUES (?1, ?2, ?3)",
            params![cliente_id, nome.trim(), arquivo],
        )
        .map_err(|e| e.to_string())?;
    Ok(st.db.last_insert_rowid())
}

#[tauri::command]
fn logos_remover(state: tauri::State<Mutex<AppState>>, id: i64) -> Result<(), String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    let arquivo: Option<String> = st
        .db
        .query_row("SELECT arquivo FROM logos WHERE id = ?1", params![id], |r| r.get(0))
        .ok();
    if let Some(arquivo) = arquivo {
        let _ = std::fs::remove_file(st.dados_dir.join("logos").join(arquivo));
    }
    st.db
        .execute("DELETE FROM logos WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/* ------------------------------ Editor (Tratar Imagem) ------------------------------ */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EditorView {
    png: String, // data URL
    w: u32,
    h: u32,
    nome: String,
    undo: usize,
    tem_svg: bool,
}

fn png_data_url(img: &image::RgbaImage) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut enc = png::Encoder::new(&mut buf, img.width(), img.height());
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(img.as_raw()).map_err(|e| e.to_string())?;
    drop(w);
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&buf)
    ))
}

/* ------------------------- Encaixe de moldes (sublimação) ------------------------- */

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaginaMolde {
    nome: String,
    rgba: String, // base64 RGBA
    w: u32,
    h: u32,
    ppm: f64,
    view: [f64; 4],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MoldeInfo {
    id: usize,
    nome: String,
    wmm: f64,
    hmm: f64,
}

#[tauri::command]
fn moldes_importar(
    state: tauri::State<Mutex<AppState>>,
    paginas: Vec<PaginaMolde>,
) -> Result<Vec<MoldeInfo>, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for p in paginas {
        let rgba = base64::engine::general_purpose::STANDARD
            .decode(&p.rgba)
            .map_err(|e| format!("{}: rgba inválido ({e})", p.nome))?;
        if rgba.len() != (p.w as usize) * (p.h as usize) * 4 {
            return Err(format!("{}: tamanho do buffer não confere", p.nome));
        }
        let id = st.moldes.len();
        out.push(MoldeInfo {
            id,
            nome: p.nome.clone(),
            wmm: p.w as f64 / p.ppm,
            hmm: p.h as f64 / p.ppm,
        });
        st.moldes.push(MoldeEstado {
            nome: p.nome,
            rgba,
            w: p.w,
            h: p.h,
            ppm: p.ppm,
            view: p.view,
            cache: None,
        });
    }
    Ok(out)
}

#[tauri::command]
fn moldes_remover(state: tauri::State<Mutex<AppState>>, id: usize) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    if id < st.moldes.len() {
        // Zera o buffer para liberar memória; os índices dos demais não mudam.
        let m = &mut st.moldes[id];
        m.rgba = Vec::new();
        m.cache = None;
        m.nome.push_str(" (removido)");
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ParteInfo {
    wmm: f64,
    hmm: f64,
    cells: u32,
    area_mm2: f64,
    crop: [f64; 4],
    clip: Vec<[f64; 4]>,
    thumb: String, // data URL
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MoldePartes {
    id: usize,
    nome: String,
    partes: Vec<ParteInfo>,
}

/// Miniatura reduzida (máx. 160 px) para a interface.
fn thumb_png(p: &nucleo::moldes::Parte) -> Result<String, String> {
    let img = image::RgbaImage::from_raw(p.thumb_w, p.thumb_h, p.thumb_rgba.clone())
        .ok_or("thumb inválida")?;
    let maior = p.thumb_w.max(p.thumb_h);
    if maior <= 160 {
        return png_data_url(&img);
    }
    let esc = 160.0 / maior as f64;
    let nw = ((p.thumb_w as f64 * esc).round() as u32).max(1);
    let nh = ((p.thumb_h as f64 * esc).round() as u32).max(1);
    let pequena = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
    png_data_url(&pequena)
}

/// Garante a análise do molde para a configuração atual (com cache).
fn partes_do_molde<'a>(
    m: &'a mut MoldeEstado,
    cfg: &nucleo::moldes::CfgMolde,
) -> Result<&'a Vec<nucleo::moldes::Parte>, String> {
    let key = format!("{}|{}|{}", cfg.grid_mm, cfg.split, cfg.white);
    if m.cache.as_ref().map_or(true, |(k, _)| k != &key) {
        if m.rgba.is_empty() {
            return Err("molde removido".into());
        }
        let partes = nucleo::moldes::analisar(
            &m.rgba,
            m.w as usize,
            m.h as usize,
            m.ppm,
            m.view,
            cfg,
        );
        m.cache = Some((key, partes));
    }
    Ok(&m.cache.as_ref().unwrap().1)
}

#[tauri::command]
fn moldes_analisar(
    state: tauri::State<Mutex<AppState>>,
    grid_mm: f64,
    split: bool,
    white: bool,
) -> Result<Vec<MoldePartes>, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let cfg = nucleo::moldes::CfgMolde { grid_mm, split, white };
    let mut out = Vec::new();
    for (id, m) in st.moldes.iter_mut().enumerate() {
        if m.rgba.is_empty() {
            continue;
        }
        let partes = partes_do_molde(m, &cfg)?;
        let mut infos = Vec::new();
        for p in partes {
            infos.push(ParteInfo {
                wmm: p.wmm,
                hmm: p.hmm,
                cells: p.cells,
                area_mm2: p.area_mm2,
                crop: p.crop,
                clip: p.clip.clone(),
                thumb: thumb_png(p)?,
            });
        }
        out.push(MoldePartes { id, nome: m.nome.clone(), partes: infos });
    }
    Ok(out)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NestCfgIn {
    w_mm: f64,
    gap_mm: f64,
    max_len_mm: f64,
    grid_mm: f64,
    together: bool,
    split: bool,
    white: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoldeQtdIn {
    molde: usize,
    qtd: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PedidoIn {
    giro: String, // "none" | "180" | "all"
    moldes: Vec<MoldeQtdIn>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlacementMoldeOut {
    pedido: usize,
    molde: usize,
    parte: usize,
    x: i32,
    y: i32,
    deg: u16,
    off_x: f64,
    off_y: f64,
    rw: f64,
    rh: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BinMoldeOut {
    len_mm: i64,
    area_mm2: f64,
    items: Vec<PlacementMoldeOut>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NestMoldeOut {
    bins: Vec<BinMoldeOut>,
    erros: Vec<String>,
    total: usize,
}

#[tauri::command]
fn moldes_encaixar(
    state: tauri::State<Mutex<AppState>>,
    cfg: NestCfgIn,
    pedidos: Vec<PedidoIn>,
) -> Result<NestMoldeOut, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let cfg_ana = nucleo::moldes::CfgMolde { grid_mm: cfg.grid_mm, split: cfg.split, white: cfg.white };
    // Garante a análise de todos os moldes usados.
    let ids: Vec<usize> = pedidos.iter().flat_map(|p| p.moldes.iter().map(|m| m.molde)).collect();
    for id in &ids {
        let m = st.moldes.get_mut(*id).ok_or("molde inexistente")?;
        partes_do_molde(m, &cfg_ana)?;
    }
    let nomes: Vec<String> = st.moldes.iter().map(|m| m.nome.clone()).collect();
    let partes: Vec<Vec<nucleo::moldes::Parte>> = st
        .moldes
        .iter()
        .map(|m| m.cache.as_ref().map(|(_, p)| p.clone()).unwrap_or_default())
        .collect();
    let pedidos_n: Vec<nucleo::moldes::PedidoNest> = pedidos
        .iter()
        .map(|p| nucleo::moldes::PedidoNest {
            giro: match p.giro.as_str() {
                "180" => nucleo::moldes::Giro::G180,
                "all" => nucleo::moldes::Giro::Todos,
                _ => nucleo::moldes::Giro::Nao,
            },
            moldes: p
                .moldes
                .iter()
                .map(|m| nucleo::moldes::MoldeQtd { molde: m.molde, qtd: m.qtd })
                .collect(),
        })
        .collect();
    let nest = nucleo::moldes::NestCfg {
        w_mm: cfg.w_mm,
        gap_mm: cfg.gap_mm,
        max_len_mm: cfg.max_len_mm,
        grid_mm: cfg.grid_mm,
        together: cfg.together,
    };
    let r = nucleo::moldes::encaixar(&nest, &partes, &pedidos_n, &nomes);
    Ok(NestMoldeOut {
        bins: r
            .bins
            .into_iter()
            .map(|b| BinMoldeOut {
                len_mm: b.len_mm,
                area_mm2: b.area_mm2,
                items: b
                    .items
                    .into_iter()
                    .map(|p| PlacementMoldeOut {
                        pedido: p.pedido,
                        molde: p.molde,
                        parte: p.parte,
                        x: p.x,
                        y: p.y,
                        deg: p.deg,
                        off_x: p.off_x,
                        off_y: p.off_y,
                        rw: p.rw,
                        rh: p.rh,
                    })
                    .collect(),
            })
            .collect(),
        erros: r.erros,
        total: r.total,
    })
}

/// Escreve arquivos gerados no frontend (PDFs do encaixe) numa pasta.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArquivoSaida {
    nome: String,
    b64: String,
}

#[tauri::command]
fn salvar_arquivos(pasta: String, arquivos: Vec<ArquivoSaida>) -> Result<Vec<String>, String> {
    let dir = PathBuf::from(&pasta);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut salvos = Vec::new();
    for a in arquivos {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&a.b64)
            .map_err(|e| format!("{}: base64 inválido ({e})", a.nome))?;
        let caminho = dir.join(&a.nome);
        std::fs::write(&caminho, &bytes).map_err(|e| e.to_string())?;
        salvos.push(caminho.to_string_lossy().into_owned());
    }
    Ok(salvos)
}

/* ------------------------- Mockups (catálogo de fotos) ------------------------- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogoOut {
    imagens: Vec<String>,
    config: Option<String>,
}

/// Lista as fotos de uma pasta de catálogo e lê o JSON de configuração, se houver.
#[tauri::command]
fn catalogo_ler(pasta: String) -> Result<CatalogoOut, String> {
    let dir = PathBuf::from(&pasta);
    let mut imagens = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let nome = e.file_name().to_string_lossy().into_owned();
        let min = nome.to_lowercase();
        if min.ends_with(".jpg") || min.ends_with(".jpeg") || min.ends_with(".png") || min.ends_with(".webp") {
            imagens.push(nome);
        }
    }
    imagens.sort();
    let config = std::fs::read_to_string(dir.join("_bancada-mockups.json")).ok();
    Ok(CatalogoOut { imagens, config })
}

/// Lê um arquivo da pasta como base64 (foto do catálogo).
#[tauri::command]
fn ler_arquivo_b64(pasta: String, nome: String) -> Result<String, String> {
    let caminho = PathBuf::from(&pasta).join(&nome);
    let bytes = std::fs::read(&caminho).map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Escreve um arquivo de texto na pasta (configuração do catálogo).
#[tauri::command]
fn escrever_arquivo_texto(pasta: String, nome: String, texto: String) -> Result<(), String> {
    let caminho = PathBuf::from(&pasta).join(&nome);
    std::fs::write(&caminho, texto).map_err(|e| e.to_string())
}

fn editor_view(ed: &EditorSessao) -> Result<EditorView, String> {
    Ok(EditorView {
        png: png_data_url(&ed.work)?,
        w: ed.work.width(),
        h: ed.work.height(),
        nome: ed.nome.clone(),
        undo: ed.undo.len(),
        tem_svg: ed.svg.is_some(),
    })
}

#[tauri::command]
fn editor_abrir(state: tauri::State<Mutex<AppState>>, id: u64) -> Result<EditorView, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let img = st.imagens.get(&id).ok_or("arte não encontrada")?;
    let nome = img.nome.clone();
    st.editor = Some(EditorSessao { work: img.rgba.clone(), nome, undo: Vec::new(), svg: None });
    editor_view(st.editor.as_ref().unwrap())
}

#[tauri::command]
fn editor_op(
    state: tauri::State<Mutex<AppState>>,
    op: String,
    tol_percent: f64,
    cor: Option<String>,
    cor_nova: Option<String>,
    alpha_t: Option<u8>,
) -> Result<EditorView, String> {
    use nucleo::editor as ed;
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let sess = st.editor.as_mut().ok_or("nenhuma imagem aberta no editor")?;
    // Snapshot para desfazer (máx. 12, como o original) — antes de mutar.
    sess.undo.push(sess.work.clone());
    if sess.undo.len() > 12 {
        sess.undo.remove(0);
    }
    let (w, h) = (sess.work.width() as usize, sess.work.height() as usize);
    let d = sess.work.as_mut();
    // Qualquer edição de pixel invalida o SVG guardado da última vetorização.
    sess.svg = None;
    // Tolerância igual ao controle do Montador: 0–100 → 0–441,7.
    let tol = tol_percent / 100.0 * 441.7;
    let cor = cor.map(|c| nucleo::cores::hex_to_rgb(&c).unwrap_or([0, 0, 0]));
    let cor_nova = cor_nova.map(|c| nucleo::cores::hex_to_rgb(&c).unwrap_or([0, 0, 0]));

    match op.as_str() {
        "fundo" => ed::remove_background(d, w, h, tol)?,
        "removerCor" => ed::remove_color(d, w, h, cor.ok_or("escolha a cor com o conta-gotas")?, tol),
        "trocarCor" => ed::replace_color(
            d,
            cor.ok_or("escolha a cor de origem com o conta-gotas")?,
            cor_nova.ok_or("escolha a nova cor")?,
            tol,
        ),
        "mono" => ed::mono(d, cor_nova.ok_or("escolha a cor")?),
        "alpha" => ed::alpha_threshold(d, alpha_t.unwrap_or(128)),
        "recorte" => {
            let (buf, nw, nh) = ed::crop_alpha(d, w, h).ok_or("a imagem está toda transparente")?;
            sess.work = image::RgbaImage::from_raw(nw as u32, nh as u32, buf).unwrap();
        }
        _ => return Err(format!("operação desconhecida: {op}")),
    }
    editor_view(sess)
}

/// Localiza o motor Real-ESRGAN embutido: ao lado do exe (release) ou no
/// repositório (dev), sempre na pasta `bin/realesrgan`.
fn realesrgan_exe() -> Result<std::path::PathBuf, String> {
    let mut candidatos = vec![
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..\\bin\\realesrgan"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidatos.push(dir.join("realesrgan"));
            candidatos.push(dir.join("bin\\realesrgan"));
        }
    }
    for pasta in candidatos {
        let exe = pasta.join("realesrgan-ncnn-vulkan.exe");
        if exe.exists() && pasta.join("models").exists() {
            return Ok(exe);
        }
    }
    Err("motor de upscale IA não encontrado (pasta bin/realesrgan)".into())
}

#[tauri::command]
fn editor_upscale_ia(state: tauri::State<Mutex<AppState>>) -> Result<EditorView, String> {
    let exe = realesrgan_exe()?;
    let pasta_modelos = exe.parent().unwrap().join("models");
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let sess = st.editor.as_mut().ok_or("nenhuma imagem aberta no editor")?;
    let tmp = std::env::temp_dir();
    let entrada = tmp.join("bancada-up-entrada.png");
    let saida = tmp.join("bancada-up-saida.png");
    sess.work.save(&entrada).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&saida);
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("-i").arg(&entrada)
        .arg("-o").arg(&saida)
        .arg("-m").arg(&pasta_modelos)
        .arg("-n").arg("realesrgan-x4plus")
        .arg("-s").arg("4")
        .arg("-f").arg("png")
        .current_dir(exe.parent().unwrap());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| format!("falha ao iniciar o motor de upscale: {e}"))?;
    if !out.status.success() || !saida.exists() {
        let log = String::from_utf8_lossy(&out.stderr);
        return Err(format!("o motor de upscale falhou: {}", log.chars().take(300).collect::<String>()));
    }
    let nova = image::open(&saida).map_err(|e| e.to_string())?.to_rgba8();
    let _ = std::fs::remove_file(&entrada);
    let _ = std::fs::remove_file(&saida);
    // Snapshot para desfazer, seguindo a mesma regra das outras operações.
    sess.undo.push(std::mem::replace(&mut sess.work, nova));
    if sess.undo.len() > 12 {
        sess.undo.remove(0);
    }
    sess.svg = None;
    editor_view(sess)
}

#[tauri::command]
fn editor_vetorizar(
    state: tauri::State<Mutex<AppState>>,
    precisao_cor: Option<u8>,
) -> Result<EditorView, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let sess = st.editor.as_mut().ok_or("nenhuma imagem aberta no editor")?;
    let (w, h) = (sess.work.width() as usize, sess.work.height() as usize);
    let v = nucleo::vetor::vetorizar(sess.work.as_raw(), w, h, precisao_cor.unwrap_or(6))?;
    // Redesenha a arte vetorizada em alta resolução (máx. 4096 no maior lado),
    // como o Montador fazia ao redesenhar na resolução do filme.
    let (px, nw, nh) = nucleo::vetor::rasterizar_svg(&v.svg, 4096)?;
    let nova = image::RgbaImage::from_raw(nw, nh, px).ok_or("rasterização inválida")?;
    sess.undo.push(std::mem::replace(&mut sess.work, nova));
    if sess.undo.len() > 12 {
        sess.undo.remove(0);
    }
    sess.svg = Some(v.svg);
    editor_view(sess)
}

#[tauri::command]
fn editor_svg(state: tauri::State<Mutex<AppState>>) -> Result<Option<String>, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    Ok(st.editor.as_ref().and_then(|s| s.svg.clone()))
}

#[tauri::command]
fn editor_desfazer(state: tauri::State<Mutex<AppState>>) -> Result<Option<EditorView>, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let Some(sess) = st.editor.as_mut() else { return Ok(None) };
    let Some(prev) = sess.undo.pop() else { return Ok(None) };
    sess.work = prev;
    Ok(Some(editor_view(sess)?))
}

#[tauri::command]
fn editor_salvar(state: tauri::State<Mutex<AppState>>, dpi: Option<f64>) -> Result<ArteInfo, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let sess = st.editor.take().ok_or("nenhuma imagem aberta no editor")?;
    let id = st.proximo_id;
    st.proximo_id += 1;
    let (w, h) = (sess.work.width(), sess.work.height());
    let d = dpi.unwrap_or(300.0);
    let nome = sess.nome.clone();
    st.imagens.insert(id, Imagem { rgba: sess.work, dpi, nome: nome.clone() });
    Ok(ArteInfo {
        id,
        name: nome,
        w,
        h,
        dpi,
        cm_w: px_to_cm(w as f64, d),
        cm_h: px_to_cm(h as f64, d),
    })
}

#[tauri::command]
fn editor_descartar(state: tauri::State<Mutex<AppState>>) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    st.editor = None;
    Ok(())
}

fn abrir_db(dir: &std::path::Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let db = Connection::open(dir.join("bancada.sqlite")).map_err(|e| e.to_string())?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS clientes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            nome TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS logos (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cliente_id INTEGER REFERENCES clientes(id),
            nome TEXT NOT NULL,
            arquivo TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS folhas (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            nome TEXT NOT NULL,
            cliente TEXT,
            criado_em INTEGER NOT NULL,
            atualizado_em INTEGER NOT NULL,
            dados TEXT NOT NULL,
            textos TEXT,
            preview TEXT
        );",
    )
    .map_err(|e| e.to_string())?;
    Ok(db)
}

/* ------------------- Arquivos, dados e backup (§3) ------------------- */

use tauri_plugin_dialog::DialogExt;

/// Abre a janela nativa do Windows para escolher uma pasta.
#[tauri::command]
fn escolher_pasta(app: tauri::AppHandle, titulo: Option<String>) -> Option<String> {
    let mut d = app.dialog().file();
    if let Some(t) = titulo {
        d = d.set_title(t);
    }
    d.blocking_pick_folder().map(|p| p.to_string())
}

#[derive(Serialize, Deserialize, Default)]
struct AppConfig {
    pasta_dados: Option<String>,
}

fn ler_config(caminho: &std::path::Path) -> AppConfig {
    std::fs::read_to_string(caminho)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn gravar_config(caminho: &std::path::Path, cfg: &AppConfig) -> Result<(), String> {
    if let Some(pai) = caminho.parent() {
        std::fs::create_dir_all(pai).map_err(|e| e.to_string())?;
    }
    std::fs::write(caminho, serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn pasta_padrao_dados(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .document_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("CentralDTF")
}

fn copiar_pasta(origem: &std::path::Path, destino: &std::path::Path) -> Result<u64, String> {
    std::fs::create_dir_all(destino).map_err(|e| e.to_string())?;
    let mut n = 0;
    for e in std::fs::read_dir(origem).map_err(|e| e.to_string())? {
        let e = e.map_err(|e2| e2.to_string())?;
        let de = destino.join(e.file_name());
        let tipo = e.file_type().map_err(|e2| e2.to_string())?;
        if tipo.is_dir() {
            n += copiar_pasta(&e.path(), &de)?;
        } else if tipo.is_file() {
            std::fs::copy(e.path(), &de).map_err(|e2| e2.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigView {
    pasta_dados: String,
    pasta_padrao: String,
}

#[tauri::command]
fn config_obter(state: tauri::State<Mutex<AppState>>, app: tauri::AppHandle) -> Result<ConfigView, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    Ok(ConfigView {
        pasta_dados: st.dados_dir.to_string_lossy().into_owned(),
        pasta_padrao: pasta_padrao_dados(&app).to_string_lossy().into_owned(),
    })
}

/// Muda a pasta de dados. Se o destino não tiver um banco da Central, os dados
/// atuais são copiados para lá (migração); se já tiver, a pasta é adotada.
#[tauri::command]
fn config_definir_pasta(
    state: tauri::State<Mutex<AppState>>,
    app: tauri::AppHandle,
    nova: String,
) -> Result<ConfigView, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let nova_pasta = PathBuf::from(&nova);
    if nova_pasta == st.dados_dir {
        let view = ConfigView {
            pasta_dados: st.dados_dir.to_string_lossy().into_owned(),
            pasta_padrao: pasta_padrao_dados(&app).to_string_lossy().into_owned(),
        };
        return Ok(view);
    }
    std::fs::create_dir_all(&nova_pasta).map_err(|e| e.to_string())?;
    if !nova_pasta.join("bancada.sqlite").exists() {
        copiar_pasta(&st.dados_dir, &nova_pasta)?;
    }
    let db = abrir_db(&nova_pasta)?;
    st.db = db;
    st.dados_dir = nova_pasta;
    gravar_config(&st.config_path, &AppConfig { pasta_dados: Some(nova.clone()) })?;
    drop(st);
    config_obter(state, app)
}

/// Exporta um backup completo (banco SQLite + arquivos) para uma subpasta
/// datada dentro da pasta escolhida.
#[tauri::command]
fn backup_exportar(state: tauri::State<Mutex<AppState>>, destino: String) -> Result<String, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let pasta = PathBuf::from(&destino).join(format!("backup-centraldtf-{ts}"));
    let n = copiar_pasta(&st.dados_dir, &pasta)?;
    Ok(format!("{n} arquivo(s) copiados para {}", pasta.to_string_lossy()))
}

/// Importa um backup: a pasta escolhida precisa conter um `bancada.sqlite`.
/// O conteúdo substitui os dados atuais (a pasta atual vira backup automático).
#[tauri::command]
fn backup_importar(state: tauri::State<Mutex<AppState>>, origem: String) -> Result<String, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let origem = PathBuf::from(&origem);
    if !origem.join("bancada.sqlite").exists() {
        return Err("a pasta escolhida não é um backup da Central (falta bancada.sqlite)".into());
    }
    // Segurança: guarda o estado atual antes de sobrescrever.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let salvaguarda = std::env::temp_dir().join(format!("centraldtf-antes-importar-{ts}"));
    copiar_pasta(&st.dados_dir, &salvaguarda)?;
    let n = copiar_pasta(&origem, &st.dados_dir.clone())?;
    st.db = abrir_db(&st.dados_dir.clone())?;
    Ok(format!(
        "{n} arquivo(s) restaurados. Cópia de segurança do estado anterior em {}",
        salvaguarda.to_string_lossy()
    ))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // A config fica num local fixo (dados do app); a pasta de dados em si
            // é configurável e por padrão fica em Documentos\CentralDTF (§3).
            let anchor = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("bancada"));
            let config_path = anchor.join("config.json");
            let cfg = ler_config(&config_path);
            let dir = cfg
                .pasta_dados
                .as_ref()
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .unwrap_or_else(|| pasta_padrao_dados(&app.handle()));
            // Migração única: instalações antigas guardavam os dados na pasta do
            // app; ao estrear a pasta padrão, herda os dados existentes.
            if dir != anchor
                && !dir.join("bancada.sqlite").exists()
                && anchor.join("bancada.sqlite").exists()
            {
                let _ = copiar_pasta(&anchor, &dir);
            }
            let db = abrir_db(&dir).map_err(|e| Box::<dyn std::error::Error>::from(e))?;
            app.manage(Mutex::new(AppState {
                imagens: HashMap::new(),
                proximo_id: 1,
                ultimo: None,
                editor: None,
                moldes: Vec::new(),
                db,
                dados_dir: dir,
                config_path,
            }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            importar_artes,
            encaixar,
            exportar_png,
            exportar_pdf,
            ajustar_layout,
            clientes_list,
            clientes_add,
            logos_list,
            logos_add,
            logos_remover,
            folhas_salvar,
            folhas_list,
            folhas_abrir,
            folhas_excluir,
            abrir_montagem,
            moldes_importar,
            moldes_remover,
            moldes_analisar,
            moldes_encaixar,
            salvar_arquivos,
            catalogo_ler,
            ler_arquivo_b64,
            escrever_arquivo_texto,
            editor_abrir,
            editor_op,
            editor_upscale_ia,
            editor_vetorizar,
            editor_svg,
            editor_desfazer,
            editor_salvar,
            editor_descartar,
            escolher_pasta,
            config_obter,
            config_definir_pasta,
            backup_exportar,
            backup_importar,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar a Bancada");
}
