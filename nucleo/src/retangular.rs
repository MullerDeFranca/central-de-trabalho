//! Encaixe retangular: MaxRects com regra "canto inferior-esquerdo", 4 ordens
//! iniciais e busca local por trocas até estourar o tempo. Portado fielmente
//! do Montador DTF (placeInBin/packSeq/improveSeq/groupShapes/packRect).

use crate::modelo::*;
use crate::rng::Rng;
use std::time::{Duration, Instant};

/// Uma arte a encaixar: especificação + variantes de giro (caixas justas).
#[derive(Clone)]
pub struct PackItem {
    pub spec: ItemSpec,
    pub variants: Vec<Variant>,
}

/// Monta as variantes de uma arte só pela matemática da caixa (sem raster).
pub fn item_variants(spec: &ItemSpec, s: &Settings) -> Vec<Variant> {
    let iw = cm_to_px(spec.w_cm, s.dpi);
    let ih = cm_to_px(spec.h_cm, s.dpi);
    angles_for(spec.rotate, s)
        .into_iter()
        .map(|a| variant_box(iw, ih, a))
        .collect()
}

/// Peça interna: arte solta (com variantes) ou bloco de cópias agrupadas.
#[derive(Clone)]
struct Piece {
    item: usize,
    variants: Vec<Variant>,
    /// Bloco agrupado: deslocamentos de cada cópia dentro do bloco (px).
    cells: Option<Vec<(i64, i64)>>,
    /// Variante da cópia individual (para expandir o bloco em placements).
    cell_variant: Option<Variant>,
    sort_w: i64,
    sort_h: i64,
}

#[derive(Clone, Copy)]
struct Rect {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

struct Bin {
    free: Vec<Rect>,
    /// (índice da peça na sequência, variante escolhida, x, y)
    placements: Vec<(usize, Variant, i64, i64)>,
    used_h: i64,
}

fn new_bin(w: i64, h: i64) -> Bin {
    Bin {
        free: vec![Rect { x: 0, y: 0, w, h }],
        placements: Vec::new(),
        used_h: 0,
    }
}

fn contains(a: &Rect, b: &Rect) -> bool {
    b.x >= a.x && b.y >= a.y && b.x + b.w <= a.x + a.w && b.y + b.h <= a.y + a.h
}

fn place_in_bin(bin: &mut Bin, piece_idx: usize, piece: &Piece, gap: i64) -> bool {
    let mut best: Option<(i64, i64, i64, i64, Variant, i64)> = None; // x,y,w,h,v,top
    for v in &piece.variants {
        let w = v.w + gap;
        let h = v.h + gap;
        for f in &bin.free {
            if w > f.w || h > f.h {
                continue;
            }
            let top = f.y + h;
            let better = match &best {
                None => true,
                Some(b) => top < b.5 || (top == b.5 && f.x < b.0),
            };
            if better {
                best = Some((f.x, f.y, w, h, *v, top));
            }
        }
    }
    let Some((bx, by, bw, bh, bv, _)) = best else {
        return false;
    };

    let mut out: Vec<Rect> = Vec::with_capacity(bin.free.len() * 2);
    for f in &bin.free {
        if bx >= f.x + f.w || bx + bw <= f.x || by >= f.y + f.h || by + bh <= f.y {
            out.push(*f);
            continue;
        }
        if bx > f.x {
            out.push(Rect { x: f.x, y: f.y, w: bx - f.x, h: f.h });
        }
        if bx + bw < f.x + f.w {
            out.push(Rect { x: bx + bw, y: f.y, w: f.x + f.w - bx - bw, h: f.h });
        }
        if by > f.y {
            out.push(Rect { x: f.x, y: f.y, w: f.w, h: by - f.y });
        }
        if by + bh < f.y + f.h {
            out.push(Rect { x: f.x, y: by + bh, w: f.w, h: f.y + f.h - by - bh });
        }
    }
    // Remove retângulos contidos em outro (o desempate do JS mantém o primeiro).
    let mut keep = Vec::with_capacity(out.len());
    'outer: for (i, a) in out.iter().enumerate() {
        for (j, b) in out.iter().enumerate() {
            if j != i && contains(b, a) && (!contains(a, b) || j < i) {
                continue 'outer;
            }
        }
        keep.push(*a);
    }
    bin.free = keep;

    bin.placements.push((piece_idx, bv, bx, by));
    bin.used_h = bin.used_h.max(by + bh - gap);
    true
}

struct Packed {
    bins: Vec<Bin>,
    seq: Vec<Piece>,
    score: i64,
}

fn pack_seq(seq: Vec<Piece>, bin_w: i64, bin_h: i64, gap: i64) -> Packed {
    let mut bins: Vec<Bin> = Vec::new();
    for (i, p) in seq.iter().enumerate() {
        let mut placed = false;
        for b in bins.iter_mut() {
            if place_in_bin(b, i, p, gap) {
                placed = true;
                break;
            }
        }
        if !placed {
            let mut b = new_bin(bin_w, bin_h);
            place_in_bin(&mut b, i, p, gap);
            bins.push(b);
        }
    }
    let score = bins.len() as i64 * 1_000_000_000_000 + bins.iter().map(|b| b.used_h).sum::<i64>();
    Packed { bins, seq, score }
}

fn best_of_orders(pieces: &[Piece], bin_w: i64, bin_h: i64, gap: i64) -> Packed {
    let mut best: Option<Packed> = None;
    let mut consider = |r: Packed| {
        if best.as_ref().is_none_or(|b| r.score < b.score) {
            best = Some(r);
        }
    };
    // As 4 ordenações do Montador.
    let mut v: Vec<Piece>;
    v = pieces.to_vec();
    v.sort_by_key(|p| -(p.sort_w * p.sort_h));
    consider(pack_seq(v, bin_w, bin_h, gap));
    v = pieces.to_vec();
    v.sort_by_key(|p| -p.sort_w.max(p.sort_h));
    consider(pack_seq(v, bin_w, bin_h, gap));
    v = pieces.to_vec();
    v.sort_by(|a, b| b.sort_h.cmp(&a.sort_h).then(b.sort_w.cmp(&a.sort_w)));
    consider(pack_seq(v, bin_w, bin_h, gap));
    v = pieces.to_vec();
    v.sort_by(|a, b| b.sort_w.cmp(&a.sort_w).then(b.sort_h.cmp(&a.sort_h)));
    consider(pack_seq(v, bin_w, bin_h, gap));
    best.expect("ao menos uma ordenação")
}

fn improve_seq(mut best: Packed, bin_w: i64, bin_h: i64, gap: i64, deadline: Instant) -> Packed {
    let n = best.seq.len();
    if n < 2 {
        return best;
    }
    let seed = (n as u32).wrapping_mul(7919).wrapping_add(bin_w as u32);
    let mut rnd = Rng::new(seed);
    while Instant::now() < deadline {
        let mut seq = best.seq.clone();
        let i = rnd.below(n);
        let j = if rnd.next() < 0.7 {
            let d = rnd.below(7) as i64 - 3;
            ((i as i64 + d).clamp(0, n as i64 - 1)) as usize
        } else {
            rnd.below(n)
        };
        if i == j {
            continue;
        }
        seq.swap(i, j);
        let r = pack_seq(seq, bin_w, bin_h, gap);
        if r.score <= best.score {
            best = r;
        }
    }
    best
}

struct Shape {
    blocks: Vec<Piece>,
    waste: i64,
    squareness: i64,
}

/// Formatos possíveis para o bloco de cópias de uma arte (colunas × linhas por ângulo).
fn group_shapes(item: usize, qty: u32, variants: &[Variant], gap: i64, bin_w: i64, bin_h: i64) -> Vec<Shape> {
    let n = qty as i64;
    let mut shapes = Vec::new();
    for cv in variants {
        let (cw, ch) = (cv.w, cv.h);
        let max_cols = bin_w / (cw + gap);
        let max_rows = bin_h / (ch + gap);
        if max_cols < 1 || max_rows < 1 {
            continue;
        }
        for cols in 1..=n.min(max_cols) {
            let mut blocks = Vec::new();
            let mut left = n;
            let mut waste = 0i64;
            while left > 0 {
                let k = left.min(cols * max_rows);
                let bc = cols.min(k);
                let br = (k + bc - 1) / bc;
                let cells: Vec<(i64, i64)> = (0..k)
                    .map(|i| ((i % bc) * (cw + gap), (i / bc) * (ch + gap)))
                    .collect();
                let bw = bc * cw + (bc - 1) * gap;
                let bh = br * ch + (br - 1) * gap;
                let pseudo = Variant { angle: cv.angle, iw: cv.iw, ih: cv.ih, w: bw, h: bh, cx: bw as f64 / 2.0, cy: bh as f64 / 2.0 };
                blocks.push(Piece {
                    item,
                    variants: vec![pseudo],
                    cells: Some(cells),
                    cell_variant: Some(*cv),
                    sort_w: bw,
                    sort_h: bh,
                });
                waste += (bc * br - k) * cw * ch;
                left -= k;
            }
            shapes.push(Shape {
                squareness: (blocks[0].sort_w - blocks[0].sort_h).abs(),
                blocks,
                waste,
            });
        }
    }
    shapes.sort_by(|a, b| a.waste.cmp(&b.waste).then(a.squareness.cmp(&b.squareness)));
    shapes.truncate(12);
    shapes
}

fn expand(piece: &Piece, v: &Variant, x: i64, y: i64, margin: i64) -> Vec<Placement> {
    match (&piece.cells, &piece.cell_variant) {
        (Some(cells), Some(cv)) => cells
            .iter()
            .map(|(dx, dy)| mk_placement(piece.item, cv, x + dx + margin, y + dy + margin))
            .collect(),
        _ => vec![mk_placement(piece.item, v, x + margin, y + margin)],
    }
}

/// Encaixe retangular. `items` já traz as variantes por ângulo (ver `item_variants`).
pub fn pack_rect(s: &Settings, items: &[PackItem], budget: Duration) -> PackResult {
    let dpi = s.dpi;
    let gap = cm_to_px(s.gap_cm, dpi);
    let margin = cm_to_px(s.margin_cm, dpi);
    let film_w = cm_to_px(s.width_cm, dpi);
    let bin_w = film_w - 2 * margin + gap;
    let bin_h = cm_to_px(s.max_len_cm, dpi) - 2 * margin + gap;
    let deadline = Instant::now() + budget;
    let fits = |v: &Variant| v.w + gap <= bin_w && v.h + gap <= bin_h;

    let mut too_big: Vec<usize> = Vec::new();
    let mut best: Option<Packed> = None;

    if s.grouped {
        let mut groups: Vec<Vec<Shape>> = Vec::new();
        for (idx, it) in items.iter().enumerate() {
            if it.spec.qty < 1 {
                continue;
            }
            let vs: Vec<Variant> = it.variants.iter().copied().filter(|v| fits(v)).collect();
            let shapes = if vs.is_empty() {
                Vec::new()
            } else {
                group_shapes(idx, it.spec.qty, &vs, gap, bin_w, bin_h)
            };
            if shapes.is_empty() {
                too_big.push(idx);
            } else {
                groups.push(shapes);
            }
        }
        if !groups.is_empty() {
            let mut choice = vec![0usize; groups.len()];
            let evaluate = |choice: &[usize]| -> Packed {
                let pieces: Vec<Piece> = groups
                    .iter()
                    .enumerate()
                    .flat_map(|(i, g)| g[choice[i]].blocks.iter().cloned())
                    .collect();
                best_of_orders(&pieces, bin_w, bin_h, gap)
            };
            best = Some(evaluate(&choice));
            let mut improved = true;
            while improved && Instant::now() < deadline {
                improved = false;
                for gi in 0..groups.len() {
                    if Instant::now() >= deadline {
                        break;
                    }
                    for ci in 0..groups[gi].len() {
                        if ci == choice[gi] {
                            continue;
                        }
                        let prev = choice[gi];
                        choice[gi] = ci;
                        let r = evaluate(&choice);
                        if r.score < best.as_ref().unwrap().score {
                            best = Some(r);
                            improved = true;
                        } else {
                            choice[gi] = prev;
                        }
                    }
                }
            }
            best = Some(improve_seq(best.unwrap(), bin_w, bin_h, gap, deadline));
        }
    } else {
        let mut pieces: Vec<Piece> = Vec::new();
        for (idx, it) in items.iter().enumerate() {
            if it.spec.qty < 1 {
                continue;
            }
            let vs: Vec<Variant> = it.variants.iter().copied().filter(|v| fits(v)).collect();
            if vs.is_empty() {
                too_big.push(idx);
                continue;
            }
            for _ in 0..it.spec.qty {
                pieces.push(Piece {
                    item: idx,
                    sort_w: vs[0].w,
                    sort_h: vs[0].h,
                    variants: vs.clone(),
                    cells: None,
                    cell_variant: None,
                });
            }
        }
        if !pieces.is_empty() {
            best = Some(improve_seq(
                best_of_orders(&pieces, bin_w, bin_h, gap),
                bin_w,
                bin_h,
                gap,
                deadline,
            ));
        }
    }

    let sheets = match &best {
        Some(b) => b
            .bins
            .iter()
            .map(|bin| {
                let placements: Vec<Placement> = bin
                    .placements
                    .iter()
                    .flat_map(|(pi, v, x, y)| expand(&b.seq[*pi], v, *x, *y, margin))
                    .collect();
                to_sheet(placements, film_w, margin)
            })
            .collect(),
        None => Vec::new(),
    };
    PackResult { sheets, too_big, cell: contour_cell(s) }
}
