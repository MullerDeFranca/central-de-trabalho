//! Encaixe de moldes de sublimação — port fiel do `encaixe-de-moldes`.
//!
//! Cada página de PDF chega já renderizada (RGBA, `ppm` pixels por mm) e o que
//! tem tinta vira máscara. As partes de cada molde são os componentes conectados
//! (frente, costas, mangas…). O encaixe usa uma grade de células de `grid_mm`
//! com máscara dilatada pelo gap entre peças.

pub const PT_MM: f64 = 25.4 / 72.0;

/* ------------------------------ análise ------------------------------ */

#[derive(Clone, Copy, Debug)]
pub struct CfgMolde {
    pub grid_mm: f64,
    pub split: bool,
    pub white: bool,
}

/// Uma parte de molde pronta para o encaixe.
#[derive(Clone, Debug)]
pub struct Parte {
    pub wmm: f64,
    pub hmm: f64,
    pub mask_w: u32,
    pub mask_h: u32,
    pub mask: Vec<u8>,
    /// Células ocupadas (após fillHoles) — usado na ordenação e no teste de espaço.
    pub cells: u32,
    pub area_mm2: f64,
    /// Miniatura só com os pixels desta parte (RGBA, resolução da análise).
    pub thumb_rgba: Vec<u8>,
    pub thumb_w: u32,
    pub thumb_h: u32,
    /// Recorte da página original em pt: [left, right, top, bottom].
    pub crop: [f64; 4],
    /// Retângulos de recorte (pt, origem no canto inferior esquerdo do recorte):
    /// [x, y, w, h]. Vazio = a caixa toda é da peça.
    pub clip: Vec<[f64; 4]>,
}

fn fill_holes(mk: &mut [u8], w: usize, h: usize) {
    let mut seen = vec![0u8; w * h];
    let mut st: Vec<usize> = Vec::new();
    let mut push = |x: usize, y: usize, seen: &mut Vec<u8>, st: &mut Vec<usize>| {
        let k = y * w + x;
        if mk[k] == 0 && seen[k] == 0 {
            seen[k] = 1;
            st.push(k);
        }
    };
    for x in 0..w {
        push(x, 0, &mut seen, &mut st);
        push(x, h - 1, &mut seen, &mut st);
    }
    for y in 0..h {
        push(0, y, &mut seen, &mut st);
        push(w - 1, y, &mut seen, &mut st);
    }
    while let Some(k) = st.pop() {
        let x = k % w;
        let y = k / w;
        if x > 0 {
            push(x - 1, y, &mut seen, &mut st);
        }
        if x + 1 < w {
            push(x + 1, y, &mut seen, &mut st);
        }
        if y > 0 {
            push(x, y - 1, &mut seen, &mut st);
        }
        if y + 1 < h {
            push(x, y + 1, &mut seen, &mut st);
        }
    }
    for k in 0..w * h {
        if mk[k] == 0 && seen[k] == 0 {
            mk[k] = 1;
        }
    }
}

/// Analisa um molde renderizado: separa as partes e gera máscaras de encaixe,
/// miniaturas, recortes (pt) e clip paths quando outra peça invade a caixa.
pub fn analisar(rgba: &[u8], w: usize, h: usize, ppm: f64, view: [f64; 4], cfg: &CfgMolde) -> Vec<Parte> {
    let n = w * h;
    // Pixels com tinta.
    let mut px = vec![0u8; n];
    for i in 0..n {
        let j = i * 4;
        if rgba[j + 3] > 20 {
            if cfg.white && rgba[j] > 245 && rgba[j + 1] > 245 && rgba[j + 2] > 245 {
                continue;
            }
            px[i] = 1;
        }
    }

    // 1) Componentes conectados (8 vizinhos).
    let mut lab = vec![0i32; n];
    let mut comps: Vec<(i64, i64, i64, i64, u64)> = vec![(0, 0, 0, 0, 0)]; // 1-based
    let mut st: Vec<usize> = Vec::new();
    for i in 0..n {
        if px[i] == 0 || lab[i] != 0 {
            continue;
        }
        let id = comps.len() as i32;
        let (mut x0, mut y0, mut x1, mut y1, mut cnt) = (i64::MAX, i64::MAX, -1i64, -1i64, 0u64);
        lab[i] = id;
        st.push(i);
        while let Some(k) = st.pop() {
            let x = (k % w) as i64;
            let y = (k / w) as i64;
            cnt += 1;
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x + dx;
                    let ny = y + dy;
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let q = (ny as usize) * w + nx as usize;
                    if px[q] == 1 && lab[q] == 0 {
                        lab[q] = id;
                        st.push(q);
                    }
                }
            }
        }
        comps.push((x0, y0, x1, y1, cnt));
    }

    // 2) Agrupa: cada componente vira uma peça; pontinhos < 4 cm² colados a uma
    //    peça (até 15 mm) são considerados parte dela.
    #[derive(Clone)]
    struct Grupo {
        labs: Vec<i32>,
        x0: i64,
        y0: i64,
        x1: i64,
        y1: i64,
    }
    fn add(g: &mut Grupo, id: i32, c: &(i64, i64, i64, i64, u64)) {
        g.labs.push(id);
        g.x0 = g.x0.min(c.0);
        g.y0 = g.y0.min(c.1);
        g.x1 = g.x1.max(c.2);
        g.y1 = g.y1.max(c.3);
    }
    let list: Vec<(i32, &(i64, i64, i64, i64, u64))> = comps
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, c)| c.4 >= 4)
        .map(|(i, c)| (i as i32, c))
        .collect();
    let mut groups: Vec<Grupo> = Vec::new();
    if !cfg.split {
        if !list.is_empty() {
            let mut g = Grupo { labs: vec![], x0: i64::MAX, y0: i64::MAX, x1: -1, y1: -1 };
            for (id, c) in &list {
                add(&mut g, *id, c);
            }
            groups.push(g);
        }
    } else {
        let tiny = (4.0 * 100.0 * ppm * ppm) as u64;
        let near = 15.0 * ppm;
        let bigs: Vec<_> = list.iter().filter(|(_, c)| c.4 >= tiny).collect();
        let smalls: Vec<_> = list.iter().filter(|(_, c)| c.4 < tiny).collect();
        for (id, c) in &bigs {
            let mut g = Grupo { labs: vec![], x0: i64::MAX, y0: i64::MAX, x1: -1, y1: -1 };
            add(&mut g, *id, c);
            groups.push(g);
        }
        for (id, c) in &smalls {
            let mut best: Option<usize> = None;
            let mut bd = f64::INFINITY;
            for (gi, g) in groups.iter().enumerate() {
                let dx = (g.x0 - c.2).max(c.0 - g.x1).max(0) as f64;
                let dy = (g.y0 - c.3).max(c.1 - g.y1).max(0) as f64;
                let d = (dx * dx + dy * dy).sqrt();
                if d < bd {
                    bd = d;
                    best = Some(gi);
                }
            }
            match best {
                Some(gi) if bd <= near => add(&mut groups[gi], *id, c),
                _ => {
                    let mut g = Grupo { labs: vec![], x0: i64::MAX, y0: i64::MAX, x1: -1, y1: -1 };
                    add(&mut g, *id, c);
                    groups.push(g);
                }
            }
        }
    }

    let cell_px = cfg.grid_mm * ppm;
    let s = ppm * PT_MM; // px por pt
    // Mapa direto rótulo -> grupo (1-based), como o Int32Array do original.
    let mut grp = vec![0i32; comps.len()];
    for (gi, g) in groups.iter().enumerate() {
        for l in &g.labs {
            grp[*l as usize] = (gi + 1) as i32;
        }
    }
    let mut parts = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        let gid = (gi + 1) as i32;
        let (bx0, by0) = (g.x0 as usize, g.y0 as usize);
        let wpx = (g.x1 - g.x0 + 1) as usize;
        let hpx = (g.y1 - g.y0 + 1) as usize;

        // Pixels desta peça dentro da caixa; detecta invasão de outra peça.
        let mut own = vec![0u8; wpx * hpx];
        let mut other = false;
        let mut area = 0u64;
        for y in 0..hpx {
            let ro = (y + by0) * w + bx0;
            for x in 0..wpx {
                let l = lab[ro + x];
                if l == 0 {
                    continue;
                }
                let gg = grp[l as usize];
                if gg == gid {
                    own[y * wpx + x] = 1;
                    area += 1;
                } else if gg != 0 {
                    other = true;
                }
            }
        }

        // 3) Se outra peça invade a caixa, máscara de recorte: cada pixel vazio
        //    fica com a peça mais próxima; vira retângulos por linha (pt).
        let mut clip: Vec<[f64; 4]> = Vec::new();
        if other {
            let mut owner = vec![0i32; wpx * hpx];
            let mut q: Vec<usize> = Vec::new();
            for y in 0..hpx {
                let ro = (y + by0) * w + bx0;
                for x in 0..wpx {
                    let l = lab[ro + x];
                    if l != 0 {
                        let g2 = grp[l as usize];
                        if g2 != 0 {
                            owner[y * wpx + x] = g2;
                            q.push(y * wpx + x);
                        }
                    }
                }
            }
            let mut qh = 0;
            while qh < q.len() {
                let k = q[qh];
                qh += 1;
                let x = k % wpx;
                let y = k / wpx;
                let o = owner[k];
                if x > 0 && owner[k - 1] == 0 {
                    owner[k - 1] = o;
                    q.push(k - 1);
                }
                if x + 1 < wpx && owner[k + 1] == 0 {
                    owner[k + 1] = o;
                    q.push(k + 1);
                }
                if y > 0 && owner[k - wpx] == 0 {
                    owner[k - wpx] = o;
                    q.push(k - wpx);
                }
                if y + 1 < hpx && owner[k + wpx] == 0 {
                    owner[k + wpx] = o;
                    q.push(k + wpx);
                }
            }
            // Retângulos por linha, juntando linhas iguais.
            let runs_of = |y: usize| -> Vec<(usize, usize)> {
                let mut r = Vec::new();
                let mut x = 0;
                while x < wpx {
                    if owner[y * wpx + x] == gid {
                        let a = x;
                        while x < wpx && owner[y * wpx + x] == gid {
                            x += 1;
                        }
                        r.push((a, x));
                    } else {
                        x += 1;
                    }
                }
                r
            };
            let mut prev: Option<Vec<(usize, usize)>> = None;
            let mut prev_start = 0usize;
            for y in 0..=hpx {
                let cur = if y < hpx { Some(runs_of(y)) } else { None };
                if cur != prev {
                    if let Some(p) = &prev {
                        for (a, b) in p {
                            clip.push([
                                *a as f64 / s,
                                (hpx - y) as f64 / s,
                                (*b - *a) as f64 / s,
                                (y - prev_start) as f64 / s,
                            ]);
                        }
                    }
                    prev = cur;
                    prev_start = y;
                }
            }
        }

        // Máscara em células (para o encaixe).
        let cw = (wpx as f64 / cell_px).ceil() as usize;
        let ch = (hpx as f64 / cell_px).ceil() as usize;
        let mut mk = vec![0u8; cw * ch];
        for y in 0..hpx {
            let my = (y as f64 / cell_px) as usize * cw;
            for x in 0..wpx {
                if own[y * wpx + x] == 1 {
                    mk[my + (x as f64 / cell_px) as usize] = 1;
                }
            }
        }
        fill_holes(&mut mk, cw, ch);
        let cells: u32 = mk.iter().map(|v| *v as u32).sum();

        // Miniatura só com os pixels desta peça.
        let mut thumb = vec![0u8; wpx * hpx * 4];
        for y in 0..hpx {
            let so = ((y + by0) * w + bx0) * 4;
            let to = y * wpx * 4;
            for x in 0..wpx {
                if own[y * wpx + x] == 0 {
                    continue;
                }
                let a = so + x * 4;
                let b = to + x * 4;
                thumb[b..b + 4].copy_from_slice(&rgba[a..a + 4]);
            }
        }

        let crop = [
            view[0] + bx0 as f64 / s,
            view[0] + (bx0 + wpx) as f64 / s,
            view[3] - by0 as f64 / s,
            view[3] - (by0 + hpx) as f64 / s,
        ];
        parts.push(Parte {
            wmm: wpx as f64 / ppm,
            hmm: hpx as f64 / ppm,
            mask_w: cw as u32,
            mask_h: ch as u32,
            mask: mk,
            cells,
            area_mm2: area as f64 / (ppm * ppm),
            thumb_rgba: thumb,
            thumb_w: wpx as u32,
            thumb_h: hpx as u32,
            crop,
            clip,
        });
    }
    parts
}

/* --------------------------- formas giradas --------------------------- */

fn rot_mask(m: &(u32, u32, &[u8]), deg: u16) -> (u32, u32, Vec<u8>) {
    let (w, h, d) = (m.0 as usize, m.1 as usize, m.2);
    if deg == 0 {
        return (w as u32, h as u32, d.to_vec());
    }
    if deg == 180 {
        let mut o = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                o[(h - 1 - y) * w + (w - 1 - x)] = d[y * w + x];
            }
        }
        return (w as u32, h as u32, o);
    }
    let (nw, nh) = (h, w);
    let mut o = vec![0u8; w * h];
    if deg == 90 {
        for y in 0..h {
            for x in 0..w {
                o[(w - 1 - x) * nw + y] = d[y * w + x];
            }
        }
    } else {
        // 270
        for y in 0..h {
            for x in 0..w {
                o[x * nw + (h - 1 - y)] = d[y * w + x];
            }
        }
    }
    (nw as u32, nh as u32, o)
}

/// Spans por linha: [(início, fim inclusivo), ...].
fn row_spans(d: &[u8], w: usize, h: usize) -> Vec<Vec<(i32, i32)>> {
    let mut rows = Vec::with_capacity(h);
    for y in 0..h {
        let mut sp = Vec::new();
        let mut x = 0;
        while x < w {
            if d[y * w + x] == 1 {
                let a = x;
                while x < w && d[y * w + x] == 1 {
                    x += 1;
                }
                sp.push((a as i32, (x - 1) as i32));
            } else {
                x += 1;
            }
        }
        rows.push(sp);
    }
    rows
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub w: i32,
    pub h: i32,
    pub k: i32,
    pub dil: Vec<Vec<(i32, i32)>>,
    pub own: Vec<Vec<(i32, i32)>>,
    pub own_cells: u32,
    pub off_x: f64,
    pub off_y: f64,
    pub rw: f64,
    pub rh: f64,
    pub deg: u16,
}

/// Forma de uma parte girada, com máscara dilatada pelo gap (em células).
pub fn get_shape(part: &Parte, deg: u16, grid_mm: f64, gap_cells: i32) -> Shape {
    let (mw, mh, md) = rot_mask(&(part.mask_w, part.mask_h, &part.mask), deg);
    let (mw, mh) = (mw as usize, mh as usize);
    let k = gap_cells;
    let (dw, dh) = (mw + 2 * k as usize, mh + 2 * k as usize);
    let mut dd = vec![0u8; dw * dh];
    let mut offs: Vec<(i32, i32)> = Vec::new();
    for dy in -k..=k {
        for dx in -k..=k {
            if dx * dx + dy * dy <= k * k + k {
                offs.push((dx, dy));
            }
        }
    }
    for y in 0..mh {
        for x in 0..mw {
            if md[y * mw + x] == 0 {
                continue;
            }
            let edge = x == 0
                || y == 0
                || x == mw - 1
                || y == mh - 1
                || md[y * mw + x - 1] == 0
                || md[y * mw + x + 1] == 0
                || md[(y - 1) * mw + x] == 0
                || md[(y + 1) * mw + x] == 0;
            if !edge {
                dd[(y + k as usize) * dw + x + k as usize] = 1;
                continue;
            }
            for (dx, dy) in &offs {
                dd[((y as i32 + k + dy) as usize) * dw + (x as i32 + k + dx) as usize] = 1;
            }
        }
    }
    let dil = row_spans(&dd, dw, dh); // coordenadas relativas a (-k,-k)
    let own = row_spans(&md, mw, mh);
    let own_cells = own
        .iter()
        .flat_map(|r| r.iter())
        .map(|(a, b)| (b - a + 1) as u32)
        .sum();
    // Deslocamento da arte dentro da grade girada (sobra da última célula).
    let pad_x = part.mask_w as f64 * grid_mm - part.wmm;
    let pad_y = part.mask_h as f64 * grid_mm - part.hmm;
    let (mut off_x, mut off_y) = (0.0, 0.0);
    match deg {
        90 => off_y = pad_x,
        180 => {
            off_x = pad_x;
            off_y = pad_y;
        }
        270 => off_x = pad_y,
        _ => {}
    }
    let (rw, rh) = if deg == 90 || deg == 270 {
        (part.hmm, part.wmm)
    } else {
        (part.wmm, part.hmm)
    };
    Shape {
        w: mw as i32,
        h: mh as i32,
        k,
        dil,
        own,
        own_cells,
        off_x,
        off_y,
        rw,
        rh,
        deg,
    }
}

/* ------------------------------ encaixe ------------------------------ */

struct Bin {
    w: usize,
    l: usize,
    occ: Vec<u8>,
    pre: Vec<i32>,
    free: i64,
    max_y: i32,
}

impl Bin {
    fn new(w: usize, l: usize) -> Self {
        Bin { w, l, occ: vec![0; w * l], pre: vec![0; (w + 1) * l], free: (w * l) as i64, max_y: 0 }
    }

    /// -1 se cabe; senão o próximo x a tentar.
    fn fits(&self, sh: &Shape, x: i32, y: i32) -> i32 {
        let (w, l) = (self.w as i32, self.l as i32);
        let k = sh.k;
        for r in 0..sh.dil.len() as i32 {
            let yy = y + r - k;
            if yy < 0 || yy >= l {
                continue;
            }
            let sp = &sh.dil[r as usize];
            let base = (yy as usize) * (self.w + 1);
            for &(a, b) in sp {
                let (a, b) = (a - k, b - k);
                let xa = (x + a).max(0);
                let xb = (x + b).min(w - 1);
                if xa > xb {
                    continue;
                }
                if self.pre[base + xb as usize + 1] - self.pre[base + xa as usize] > 0 {
                    let mut c = xb;
                    let ro = (yy as usize) * self.w;
                    while self.occ[ro + c as usize] == 0 {
                        c -= 1;
                    }
                    return c - a + 1;
                }
            }
        }
        -1
    }

    fn find(&self, sh: &Shape) -> Option<(i32, i32)> {
        if sh.w as usize > self.w || sh.h as usize > self.l {
            return None;
        }
        for y in 0..=(self.l as i32 - sh.h) {
            let mut x = 0;
            while x <= self.w as i32 - sh.w {
                let n = self.fits(sh, x, y);
                if n < 0 {
                    return Some((x, y));
                }
                x = n;
            }
        }
        None
    }

    fn place(&mut self, sh: &Shape, x: i32, y: i32) {
        for r in 0..sh.own.len() as i32 {
            let yy = y + r;
            let sp = &sh.own[r as usize];
            if sp.is_empty() {
                continue;
            }
            for &(a, b) in sp {
                for xx in (x + a)..=(x + b) {
                    let idx = (yy as usize) * self.w + xx as usize;
                    if self.occ[idx] == 0 {
                        self.occ[idx] = 1;
                        self.free -= 1;
                    }
                }
            }
            // Reconstrói o prefixo da linha afetada.
            let base = (yy as usize) * (self.w + 1);
            let ro = (yy as usize) * self.w;
            let mut s = 0i32;
            self.pre[base] = 0;
            for xx in 0..self.w {
                s += self.occ[ro + xx] as i32;
                self.pre[base + xx + 1] = s;
            }
        }
        if y + sh.h > self.max_y {
            self.max_y = y + sh.h;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Giro {
    Nao,
    G180,
    Todos,
}

impl Giro {
    pub fn rots(self) -> &'static [u16] {
        match self {
            Giro::Nao => &[0],
            Giro::G180 => &[0, 180],
            Giro::Todos => &[0, 90, 180, 270],
        }
    }
}

#[derive(Clone, Debug)]
pub struct MoldeQtd {
    /// Índice global do molde (na lista do app).
    pub molde: usize,
    pub qtd: u32,
}

#[derive(Clone, Debug)]
pub struct PedidoNest {
    pub giro: Giro,
    pub moldes: Vec<MoldeQtd>,
}

#[derive(Clone, Copy, Debug)]
pub struct NestCfg {
    pub w_mm: f64,
    pub gap_mm: f64,
    pub max_len_mm: f64,
    pub grid_mm: f64,
    pub together: bool,
}

#[derive(Clone, Debug)]
pub struct PlacementMolde {
    pub pedido: usize,
    pub molde: usize,
    pub parte: usize,
    pub x: i32,
    pub y: i32,
    pub deg: u16,
    pub off_x: f64,
    pub off_y: f64,
    pub rw: f64,
    pub rh: f64,
}

#[derive(Clone, Debug)]
pub struct BinOut {
    pub len_mm: i64,
    pub area_mm2: f64,
    pub items: Vec<PlacementMolde>,
}

#[derive(Clone, Debug)]
pub struct NestOut {
    pub bins: Vec<BinOut>,
    pub erros: Vec<String>,
    pub total: usize,
}

/// Encaixa os moldes de todos os pedidos no rolo, em partes de até max_len.
/// `partes` são as partes analisadas por molde (índice global).
pub fn encaixar(cfg: &NestCfg, partes: &[Vec<Parte>], pedidos: &[PedidoNest], nomes: &[String]) -> NestOut {
    let g = cfg.grid_mm;
    let gap_cells = (cfg.gap_mm / g - 1e-9).ceil() as i32;
    let wc = (cfg.w_mm / g).floor() as usize;
    let lc = (cfg.max_len_mm / g).floor() as usize;

    struct It {
        pi: usize,
        molde: usize,
        parte: usize,
        rots: &'static [u16],
        area: u32,
    }
    let mut items: Vec<It> = Vec::new();
    let mut erros: Vec<String> = Vec::new();
    for (pi, p) in pedidos.iter().enumerate() {
        for m in &p.moldes {
            if m.qtd == 0 {
                continue;
            }
            let pts = match partes.get(m.molde) {
                Some(v) => v,
                None => continue,
            };
            if pts.is_empty() {
                let nome = nomes.get(m.molde).cloned().unwrap_or_default();
                let msg = format!("“{}” não tem nada para imprimir.", nome);
                if !erros.contains(&msg) {
                    erros.push(msg);
                }
                continue;
            }
            for _ in 0..m.qtd {
                for (ix, pt) in pts.iter().enumerate() {
                    items.push(It { pi, molde: m.molde, parte: ix, rots: p.giro.rots(), area: pt.cells });
                }
            }
        }
    }
    let total = items.len();
    if items.is_empty() {
        if erros.is_empty() {
            erros.push("Coloque a quantidade de pelo menos um molde.".into());
        }
        return NestOut { bins: vec![], erros, total };
    }
    if cfg.together {
        items.sort_by(|a, b| a.pi.cmp(&b.pi).then(b.area.cmp(&a.area)));
    } else {
        items.sort_by(|a, b| b.area.cmp(&a.area).then(a.pi.cmp(&b.pi)));
    }

    let mut bins: Vec<Bin> = Vec::new();
    let mut bin_items: Vec<Vec<PlacementMolde>> = Vec::new();
    let mut first_bin: Vec<Option<usize>> = vec![None; pedidos.len()];
    let mut shape_cache: std::collections::HashMap<(usize, usize, u16), std::rc::Rc<Shape>> =
        std::collections::HashMap::new();

    for it in &items {
        let shapes: Vec<std::rc::Rc<Shape>> = it
            .rots
            .iter()
            .map(|&d| {
                let key = (it.molde, it.parte, d);
                shape_cache
                    .entry(key)
                    .or_insert_with(|| std::rc::Rc::new(get_shape(&partes[it.molde][it.parte], d, g, gap_cells)))
                    .clone()
            })
            .collect();
        let cells_need = shapes[0].own_cells as i64;

        let start_bin = if cfg.together { first_bin[it.pi].unwrap_or(0) } else { 0 };
        let mut placed = false;
        let mut bi = start_bin;
        while bi <= bins.len() && !placed {
            if bi == bins.len() {
                bins.push(Bin::new(wc, lc));
                bin_items.push(Vec::new());
            }
            let bin = &bins[bi];
            if bin.free < cells_need {
                bi += 1;
                continue;
            }
            let mut best: Option<(i32, i32, std::rc::Rc<Shape>, i64)> = None;
            for sh in &shapes {
                if let Some((x, y)) = bin.find(sh) {
                    let sc = ((y + sh.h) as i64) * wc as i64 + x as i64;
                    if best.as_ref().map_or(true, |b| sc < b.3) {
                        best = Some((x, y, sh.clone(), sc));
                    }
                }
            }
            if let Some((x, y, sh, _)) = best {
                bins[bi].place(&sh, x, y);
                bin_items[bi].push(PlacementMolde {
                    pedido: it.pi,
                    molde: it.molde,
                    parte: it.parte,
                    x,
                    y,
                    deg: sh.deg,
                    off_x: sh.off_x,
                    off_y: sh.off_y,
                    rw: sh.rw,
                    rh: sh.rh,
                });
                placed = true;
                if first_bin[it.pi].is_none() {
                    first_bin[it.pi] = Some(bi);
                }
            } else if bins[bi].max_y == 0 && bin_items[bi].is_empty() {
                // Não coube nem numa parte vazia.
                bins.pop();
                bin_items.pop();
                break;
            }
            bi += 1;
        }
        if !placed {
            let nome = nomes.get(it.molde).cloned().unwrap_or_default();
            let msg = format!("“{}” é maior que o rolo ou que o comprimento máximo da parte.", nome);
            if !erros.contains(&msg) {
                erros.push(msg);
            }
        }
    }

    let bins_out = bins
        .iter()
        .zip(bin_items)
        .filter(|(_, its)| !its.is_empty())
        .map(|(_, its)| {
            let mut len = 0.0f64;
            let mut area = 0.0f64;
            for it in &its {
                len = len.max(it.y as f64 * g + it.off_y + it.rh);
                area += partes[it.molde][it.parte].area_mm2;
            }
            BinOut { len_mm: len.ceil() as i64, area_mm2: area, items: its }
        })
        .collect();
    NestOut { bins: bins_out, erros, total }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Molde 100×50 mm com duas partes separadas (dois quadrados de 20 mm).
    fn molde_duas_partes() -> (Vec<u8>, usize, usize, f64) {
        let ppm = 2.0;
        let (w, h) = (200usize, 100usize);
        let mut rgba = vec![0u8; w * h * 4];
        let mut paint = |x0: usize, y0: usize, x1: usize, y1: usize| {
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * w + x) * 4;
                    rgba[i] = 200;
                    rgba[i + 1] = 40;
                    rgba[i + 2] = 40;
                    rgba[i + 3] = 255;
                }
            }
        };
        paint(10, 10, 50, 50); // 20×20 mm
        paint(100, 40, 140, 80);
        (rgba, w, h, ppm)
    }

    #[test]
    fn separa_duas_partes() {
        let (rgba, w, h, ppm) = molde_duas_partes();
        let cfg = CfgMolde { grid_mm: 2.0, split: true, white: false };
        let partes = analisar(&rgba, w, h, ppm, [0.0, 0.0, w as f64 / ppm / PT_MM, h as f64 / ppm / PT_MM], &cfg);
        assert_eq!(partes.len(), 2, "duas manchas separadas = duas partes");
        for p in &partes {
            assert!((p.wmm - 20.0).abs() < 1.5, "largura ~20 mm, veio {}", p.wmm);
            assert!(p.cells > 0);
        }
        // Sem split: uma parte só.
        let cfg2 = CfgMolde { split: false, ..cfg };
        let partes2 = analisar(&rgba, w, h, ppm, [0.0, 0.0, w as f64 / ppm / PT_MM, h as f64 / ppm / PT_MM], &cfg2);
        assert_eq!(partes2.len(), 1);
    }

    #[test]
    fn encaixe_respeita_gap_e_rolo() {
        let (rgba, w, h, ppm) = molde_duas_partes();
        let cfg = CfgMolde { grid_mm: 2.0, split: true, white: false };
        let partes = vec![analisar(&rgba, w, h, ppm, [0.0, 0.0, w as f64 / ppm / PT_MM, h as f64 / ppm / PT_MM], &cfg)];
        let nest = NestCfg { w_mm: 300.0, gap_mm: 5.0, max_len_mm: 500.0, grid_mm: 2.0, together: true };
        let pedidos = vec![PedidoNest { giro: Giro::Todos, moldes: vec![MoldeQtd { molde: 0, qtd: 5 }] }];
        let out = encaixar(&nest, &partes, &pedidos, &["teste".into()]);
        assert_eq!(out.total, 10);
        let colocadas: usize = out.bins.iter().map(|b| b.items.len()).sum();
        assert_eq!(colocadas, 10, "todas as peças devem caber");
        assert!(out.erros.is_empty());
        // Comprimento não pode estourar o máximo.
        for b in &out.bins {
            assert!(b.len_mm <= 500 + 5);
        }
    }

    #[test]
    fn peca_grande_demais_vai_para_erros() {
        let (rgba, w, h, ppm) = molde_duas_partes();
        let cfg = CfgMolde { grid_mm: 2.0, split: false, white: false };
        let partes = vec![analisar(&rgba, w, h, ppm, [0.0, 0.0, w as f64 / ppm / PT_MM, h as f64 / ppm / PT_MM], &cfg)];
        // Rolo de 30 mm: a parte única (~65×20 mm) não cabe na largura.
        let nest = NestCfg { w_mm: 30.0, gap_mm: 5.0, max_len_mm: 500.0, grid_mm: 2.0, together: true };
        let pedidos = vec![PedidoNest { giro: Giro::Nao, moldes: vec![MoldeQtd { molde: 0, qtd: 1 }] }];
        let out = encaixar(&nest, &partes, &pedidos, &["teste".into()]);
        assert!(out.bins.is_empty());
        assert_eq!(out.erros.len(), 1);
    }
}
