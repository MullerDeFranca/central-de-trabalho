//! Encaixe por contorno: grade de ocupação em células, com folga dilatada,
//! regra "canto inferior-esquerdo" e busca local. Portado fielmente do
//! Montador DTF (dilate/maskFor/scanRow/findPos/placeMask/buildCluster/
//! bestCluster/packContour).

use crate::modelo::*;
use crate::rng::Rng;
use std::time::{Duration, Instant};

/// Superamostragem da máscara por célula (o Montador usa 3×).
pub const MASK_SUPERSAMPLE: usize = 3;

/// Dilata a grade em `d` células (quadrado), igual ao `dilate` do Montador.
pub fn dilate(g: &[u8], w: usize, h: usize, d: usize) -> Vec<u8> {
    let mut t = vec![0u8; w * h];
    let mut o = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            if g[y * w + x] != 0 {
                let a = x.saturating_sub(d);
                let b = (x + d).min(w - 1);
                for i in a..=b {
                    t[y * w + i] = 1;
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            if t[y * w + x] != 0 {
                let a = y.saturating_sub(d);
                let b = (y + d).min(h - 1);
                for k in a..=b {
                    o[k * w + x] = 1;
                }
            }
        }
    }
    o
}

/// Máscara de uma variante a partir do canal alpha renderizado.
///
/// `alpha` é o bitmap da caixa justa da variante com `MASK_SUPERSAMPLE` px por
/// célula: largura `cw * S` e altura `ch * S`, onde `cw = ceil(v.w / cell)` e
/// `ch = ceil(v.h / cell)`. `d` é a folga em células (metade do espaçamento).
pub fn mask_from_alpha(alpha: &[u8], width: usize, height: usize, d: usize) -> Mask {
    let s = MASK_SUPERSAMPLE;
    let cw = (width / s).max(1);
    let ch = (height / s).max(1);
    let mw = cw + 2 * d;
    let mh = ch + 2 * d;
    let mut grid = vec![0u8; mw * mh];
    for y in 0..height {
        let gy = y / s + d;
        for x in 0..width {
            if alpha[y * width + x] > 10 {
                grid[gy * mw + (x / s + d)] = 1;
            }
        }
    }
    if d > 0 {
        grid = dilate(&grid, mw, mh, d);
    }
    Mask::new(mw, mh, grid)
}

/// Uma arte a encaixar por contorno: especificação + candidatos (um por ângulo,
/// máscaras já com a folga dilatada).
#[derive(Clone)]
pub struct ContourItem {
    pub spec: ItemSpec,
    pub cands: Vec<Cand>,
}

/// Grade de ocupação esparsa por linha (occ/prev criados sob demanda).
struct Grid {
    w: usize,
    h: usize,
    occ: Vec<Option<Vec<u8>>>,
    prev: Vec<Option<Vec<i32>>>,
    full: Vec<u8>,
    first: usize,
    used_rows: usize,
}

fn new_grid(w: usize, h: usize) -> Grid {
    Grid {
        w,
        h,
        occ: vec![None; h],
        prev: vec![None; h],
        full: vec![0; h],
        first: 0,
        used_rows: 0,
    }
}

/// Primeira coluna livre na linha y, pulando para depois de cada colisão.
fn scan_row(g: &Grid, m: &Mask, y: usize) -> i64 {
    let mut x: i64 = 0;
    let x_max = g.w as i64 - m.w as i64;
    while x <= x_max {
        let mut hit: i64 = -1;
        for r in 0..m.h {
            if hit >= 0 {
                break;
            }
            let Some(po) = &g.prev[y + r] else { continue };
            for &(a, b) in &m.runs[r] {
                let q = po[(x + b as i64) as usize] as i64;
                if q >= x + a as i64 {
                    hit = q - a as i64 + 1;
                    break;
                }
            }
        }
        if hit < 0 {
            return x;
        }
        x = hit;
    }
    -1
}

struct Pos {
    cand: usize,
    x: i64,
    y: i64,
    bottom: i64,
}

/// Posição com o menor "fundo" (y + altura), depois mais à esquerda.
fn find_pos(g: &Grid, cands: &[Cand]) -> Option<Pos> {
    let mut best: Option<Pos> = None;
    for (ci, c) in cands.iter().enumerate() {
        let m = &c.m;
        if m.w > g.w || m.h > g.h {
            continue;
        }
        let mut y_lim = (g.h - m.h) as i64;
        if let Some(b) = &best {
            y_lim = y_lim.min(b.bottom - m.h as i64);
        }
        let first_row_full = !m.runs[0].is_empty();
        let mut y = g.first as i64;
        while y <= y_lim {
            if first_row_full && g.full[y as usize] != 0 {
                y += 1;
                continue;
            }
            let x = scan_row(g, m, y as usize);
            if x < 0 {
                y += 1;
                continue;
            }
            let bottom = y + m.h as i64;
            let better = match &best {
                None => true,
                Some(b) => bottom < b.bottom || (bottom == b.bottom && x < b.x),
            };
            if better {
                best = Some(Pos { cand: ci, x, y, bottom });
            }
            break;
        }
    }
    best
}

fn place_mask(g: &mut Grid, m: &Mask, x: i64, y: i64) {
    for r in 0..m.h {
        if m.runs[r].is_empty() {
            continue;
        }
        let yy = (y + r as i64) as usize;
        if g.occ[yy].is_none() {
            g.occ[yy] = Some(vec![0u8; g.w]);
            g.prev[yy] = Some(vec![-1i32; g.w]);
        }
        {
            let occ = g.occ[yy].as_mut().unwrap();
            for &(a, b) in &m.runs[r] {
                for i in (x + a as i64) as usize..=(x + b as i64) as usize {
                    occ[i] = 1;
                }
            }
        }
        let occ = g.occ[yy].as_ref().unwrap();
        let prev = g.prev[yy].as_mut().unwrap();
        let mut last: i32 = -1;
        let mut count = 0usize;
        for i in 0..g.w {
            if occ[i] != 0 {
                last = i as i32;
                count += 1;
            }
            prev[i] = last;
        }
        g.full[yy] = if count == g.w { 1 } else { 0 };
    }
    while g.first < g.h && g.full[g.first] != 0 {
        g.first += 1;
    }
    g.used_rows = g.used_rows.max((y + m.h as i64) as usize);
}

/// Encaixa `count` cópias de uma arte entre si numa faixa de largura `width`;
/// o grupo vira uma única peça (cluster).
fn build_cluster(cands: &[Cand], count: usize, width: usize, hg: usize) -> Cand {
    let mut g = new_grid(width, hg);
    let mut members: Vec<Member> = Vec::new();
    for _ in 0..count {
        let Some(pos) = find_pos(&g, cands) else { break };
        let m = cands[pos.cand].m.clone();
        place_mask(&mut g, &m, pos.x, pos.y);
        members.push(Member { v: cands[pos.cand].members[0].v, dx: pos.x, dy: pos.y });
    }
    let mut mw = 0usize;
    for y in 0..g.used_rows {
        if let Some(prev) = &g.prev[y] {
            mw = mw.max((prev[g.w - 1] + 1) as usize);
        }
    }
    let mh = g.used_rows;
    let mut grid = vec![0u8; mw * mh];
    for y in 0..mh {
        if let Some(occ) = &g.occ[y] {
            grid[y * mw..y * mw + mw].copy_from_slice(&occ[..mw]);
        }
    }
    Cand { m: Mask::new(mw, mh, grid), members }
}

/// Melhor cluster entre ~√n colunas (testa root-1, root, root+1).
fn best_cluster(cands: &[Cand], count: usize, wg: usize, hg: usize) -> Cand {
    let base = cands[0].m.w;
    let min_w = cands.iter().map(|c| c.m.w).min().unwrap_or(1);
    let root = (count as f64).sqrt().ceil() as usize;
    let mut cols_set: Vec<usize> = vec![root.saturating_sub(1).max(1), root, root + 1];
    cols_set.dedup();
    let mut best: Option<Cand> = None;
    for cols in cols_set {
        let width = (cols * base).clamp(min_w, wg);
        let cl = build_cluster(cands, count, width, hg);
        let better = match &best {
            None => true,
            Some(b) => {
                cl.members.len() > b.members.len()
                    || (cl.members.len() == b.members.len() && cl.m.w * cl.m.h < b.m.w * b.m.h)
            }
        };
        if better {
            best = Some(cl);
        }
    }
    best.expect("ao menos um cluster")
}

#[derive(Clone)]
struct Unit {
    item: usize,
    cands: Vec<Cand>,
    area: u64,
}

struct CBin {
    g: Grid,
    placements: Vec<Placement>,
}

/// Encaixe por contorno. `cell` em px (padrão: `contour_cell(s)`).
pub fn pack_contorno(s: &Settings, items: &[ContourItem], budget: Duration, cell: Option<f64>) -> PackResult {
    let dpi = s.dpi;
    let margin = cm_to_px(s.margin_cm, dpi);
    let film_w = cm_to_px(s.width_cm, dpi);
    let usable_w = film_w - 2 * margin;
    let usable_h = cm_to_px(s.max_len_cm, dpi) - 2 * margin;
    let cell = cell.unwrap_or_else(|| contour_cell(s));
    // Cada arte ganha d células de folga; duas folgas separam as artes por >= espaçamento.
    let d = (s.gap_cm / CM_PER_IN * dpi / 2.0 / cell - 1e-9).ceil().max(0.0) as usize;
    let wg = (usable_w as f64 / cell).floor() as usize + 2 * d;
    let hh = (usable_h as f64 / cell).floor() as usize + 2 * d;

    let mut too_big: Vec<usize> = Vec::new();
    let mut units: Vec<Unit> = Vec::new();
    for (idx, it) in items.iter().enumerate() {
        if it.spec.qty < 1 {
            continue;
        }
        let cands: Vec<Cand> = it
            .cands
            .iter()
            .filter(|c| c.m.w <= wg && c.m.h <= hh)
            .cloned()
            .collect();
        if cands.is_empty() {
            too_big.push(idx);
            continue;
        }
        if s.grouped {
            let mut left = it.spec.qty as usize;
            while left > 0 {
                let cl = best_cluster(&cands, left, wg, hh);
                let area = cl.m.area;
                let n = cl.members.len().max(1);
                units.push(Unit { item: idx, cands: vec![cl], area });
                left = left.saturating_sub(n);
            }
        } else {
            let area = cands.iter().map(|c| c.m.area).min().unwrap_or(0);
            for _ in 0..it.spec.qty {
                units.push(Unit { item: idx, cands: cands.clone(), area });
            }
        }
    }

    let pack_units = |seq: &[Unit]| -> (Vec<CBin>, i64) {
        let mut bins: Vec<CBin> = Vec::new();
        for u in seq {
            let mut found: Option<(usize, Pos)> = None;
            for (bi, b) in bins.iter().enumerate() {
                if let Some(p) = find_pos(&b.g, &u.cands) {
                    found = Some((bi, p));
                    break;
                }
            }
            let (bi, pos) = match found {
                Some(x) => x,
                None => {
                    bins.push(CBin { g: new_grid(wg, hh), placements: Vec::new() });
                    let bi = bins.len() - 1;
                    let p = find_pos(&bins[bi].g, &u.cands).expect("unidade cabe em grade vazia");
                    (bi, p)
                }
            };
            let bin = &mut bins[bi];
            let cand = u.cands[pos.cand].clone();
            place_mask(&mut bin.g, &cand.m, pos.x, pos.y);
            for mem in &cand.members {
                let px = margin as f64 + (pos.x + mem.dx) as f64 * cell;
                let py = margin as f64 + (pos.y + mem.dy) as f64 * cell;
                bin.placements
                    .push(mk_placement(u.item, &mem.v, px.round() as i64, py.round() as i64));
            }
        }
        let score = bins.len() as i64 * 1_000_000_000_000
            + bins.iter().map(|b| b.g.used_rows as i64).sum::<i64>();
        (bins, score)
    };

    let deadline = Instant::now() + budget;
    let n = units.len();

    // As 4 ordens iniciais do Montador (área, maior lado, altura, largura).
    let mut orders: Vec<Vec<usize>> = Vec::new();
    {
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by_key(|&k| -(units[k].area as i64));
        orders.push(idx);
    }
    {
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by_key(|&k| {
            let m = &units[k].cands[0].m;
            -(m.w.max(m.h) as i64)
        });
        orders.push(idx);
    }
    {
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by_key(|&k| -(units[k].cands[0].m.h as i64));
        orders.push(idx);
    }
    {
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by_key(|&k| -(units[k].cands[0].m.w as i64));
        orders.push(idx);
    }

    let mut best: Option<(Vec<CBin>, i64)> = None;
    let mut best_seq: Vec<usize> = (0..n).collect();
    for ord in orders {
        let seq: Vec<Unit> = ord.iter().map(|&k| units[k].clone()).collect();
        let r = pack_units(&seq);
        if best.as_ref().is_none_or(|b| r.1 < b.1) {
            best = Some(r);
            best_seq = ord;
        }
        if Instant::now() >= deadline {
            break;
        }
    }

    // Busca local: troca unidades de lugar na sequência do melhor resultado.
    let seed = (n as u32).wrapping_mul(104729).wrapping_add(wg as u32);
    let mut rnd = Rng::new(seed);
    while n > 1 && Instant::now() < deadline {
        let mut seq_idx = best_seq.clone();
        let i = rnd.below(n);
        let j = if rnd.next() < 0.7 {
            let dd = rnd.below(7) as i64 - 3;
            ((i as i64 + dd).clamp(0, n as i64 - 1)) as usize
        } else {
            rnd.below(n)
        };
        if i == j {
            continue;
        }
        seq_idx.swap(i, j);
        let seq: Vec<Unit> = seq_idx.iter().map(|&k| units[k].clone()).collect();
        let r = pack_units(&seq);
        if best.as_ref().is_none_or(|b| r.1 <= b.1) {
            best = Some(r);
            best_seq = seq_idx;
        }
    }

    let sheets = match &best {
        Some((bins, _)) => bins
            .iter()
            .map(|b| to_sheet(b.placements.clone(), film_w, margin))
            .collect(),
        None => Vec::new(),
    };
    PackResult { sheets, too_big, cell }
}
