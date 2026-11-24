//! Modelo de dados do núcleo: configurações da folha, artes, variantes de giro,
//! máscaras de contorno e o resultado do encaixe.
//!
//! Portado com fidelidade do Montador DTF (montador-dtf/index.html), mantendo
//! as mesmas unidades (tudo em px na resolução do filme) e o mesmo formato de
//! backup (`app: "montador-dtf"`) para o app novo abrir as folhas antigas.

use serde::{Deserialize, Serialize};

pub const CM_PER_IN: f64 = 2.54;
pub const MAX_WIDTH_CM: f64 = 200.0;
pub const MAX_LEN_CM: f64 = 3000.0; // campo vazio = sem limite (na prática, 30 m)
pub const MAX_LEN_LIMIT: f64 = 3000.0;
pub const EXPORT_WARN_CM: f64 = 200.0;

#[inline]
pub fn cm_to_px(cm: f64, dpi: f64) -> i64 {
    (cm / CM_PER_IN * dpi).round() as i64
}
#[inline]
pub fn px_to_cm(px: f64, dpi: f64) -> f64 {
    px / dpi * CM_PER_IN
}
#[inline]
pub fn clamp(v: f64, a: f64, b: f64) -> f64 {
    v.min(b).max(a)
}

/// Configurações da folha/filme (o `readSettings()` do Montador).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub width_cm: f64,
    pub margin_cm: f64,
    pub gap_cm: f64,
    pub max_len_cm: f64,
    pub dpi: f64,
    pub contour: bool,
    pub grouped: bool,
    pub mirror: bool,
    pub angle_step: f64,
    pub cost_m: f64,
    pub price_m: f64,
    #[serde(default)]
    pub force_rotate: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            width_cm: 58.0,
            margin_cm: 0.5,
            gap_cm: 0.3,
            max_len_cm: MAX_LEN_CM,
            dpi: 300.0,
            contour: false,
            grouped: false,
            mirror: false,
            angle_step: 90.0,
            cost_m: 0.0,
            price_m: 0.0,
            force_rotate: false,
        }
    }
}

impl Settings {
    /// Aplica os mesmos limites do app (readSettings).
    pub fn sanitized(mut self) -> Self {
        self.width_cm = clamp(self.width_cm, 1.0, MAX_WIDTH_CM);
        self.margin_cm = self.margin_cm.max(0.0);
        self.gap_cm = self.gap_cm.max(0.0);
        self.max_len_cm = clamp(self.max_len_cm.round(), 10.0, MAX_LEN_LIMIT);
        self.dpi = clamp(self.dpi.round(), 72.0, 1200.0);
        if self.angle_step <= 0.0 {
            self.angle_step = 90.0;
        }
        self
    }
}

/// Uma arte na lista (a "logo" do Montador). As imagens em si ficam fora do
/// núcleo puro: quem chama entrega as variantes/máscaras já calculadas.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSpec {
    pub name: String,
    pub w_cm: f64,
    pub h_cm: f64,
    #[serde(default = "um")]
    pub qty: u32,
    #[serde(default = "sim")]
    pub rotate: bool,
    #[serde(default)]
    pub fill: bool,
    #[serde(default = "um")]
    pub ratio: u32,
}

fn um() -> u32 {
    1
}
fn sim() -> bool {
    true
}

/// Variação de giro de uma arte: caixa justa da arte girada, em px de saída.
/// cx/cy = centro da arte dentro da caixa.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Variant {
    pub angle: f64,
    /// tamanho da arte sem giro (px na resolução do filme)
    pub iw: i64,
    pub ih: i64,
    /// caixa justa girada
    pub w: i64,
    pub h: i64,
    pub cx: f64,
    pub cy: f64,
}

/// Máscara da arte em células (encaixe por contorno): grade de ocupação com
/// os "runs" (trechos ocupados por linha) pré-calculados.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mask {
    pub w: usize,
    pub h: usize,
    #[serde(with = "serde_bytes_grid")]
    pub grid: Vec<u8>,
    #[serde(skip)]
    pub runs: Vec<Vec<(i32, i32)>>,
    #[serde(skip)]
    pub area: u64,
}

mod serde_bytes_grid {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        // Grade binária: 0/1 por célula. Vai como string de "0"/"1" (compacta no JSON).
        s.serialize_str(unsafe { std::str::from_utf8_unchecked(v) })
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        Ok(s.into_bytes())
    }
}

impl Mask {
    pub fn new(w: usize, h: usize, grid: Vec<u8>) -> Self {
        let mut m = Mask { w, h, grid, runs: Vec::new(), area: 0 };
        m.compute_runs();
        m
    }
    pub fn compute_runs(&mut self) {
        self.runs.clear();
        self.area = 0;
        for y in 0..self.h {
            let mut row = Vec::new();
            let mut x = 0usize;
            while x < self.w {
                if self.grid[y * self.w + x] == 0 {
                    x += 1;
                    continue;
                }
                let a = x;
                while x + 1 < self.w && self.grid[y * self.w + x + 1] != 0 {
                    x += 1;
                }
                row.push((a as i32, x as i32));
                self.area += (x - a + 1) as u64;
                x += 1;
            }
            self.runs.push(row);
        }
    }
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> bool {
        self.grid[y * self.w + x] != 0
    }
}

/// Candidato de encaixe por contorno: uma máscara + os membros (para blocos
/// agrupados, vários membros com deslocamento em células).
#[derive(Clone)]
pub struct Cand {
    pub m: Mask,
    pub members: Vec<Member>,
}

#[derive(Clone, Copy)]
pub struct Member {
    pub v: Variant,
    pub dx: i64,
    pub dy: i64,
}

/// Peça posicionada na folha (px na resolução do filme).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Placement {
    pub item: usize,
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub angle: f64,
    pub iw: i64,
    pub ih: i64,
    pub cx: f64,
    pub cy: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    pub width_px: i64,
    pub length_px: i64,
    pub placements: Vec<Placement>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackResult {
    pub sheets: Vec<Sheet>,
    /// Índices das artes que não cabem no filme.
    pub too_big: Vec<usize>,
    /// Célula do encaixe por contorno (px), útil para checar sobreposição.
    pub cell: f64,
}

/// Tamanho da célula do encaixe por contorno (1 a 5 mm, conforme a largura do filme).
pub fn contour_cell(s: &Settings) -> f64 {
    let margin = cm_to_px(s.margin_cm, s.dpi);
    let usable_w = cm_to_px(s.width_cm, s.dpi) - 2 * margin;
    let cell_mm = clamp(px_to_cm(usable_w as f64, s.dpi) * 10.0 / 350.0, 1.0, 5.0);
    cell_mm / 25.4 * s.dpi
}

pub fn mk_placement(item: usize, v: &Variant, x: i64, y: i64) -> Placement {
    Placement {
        item,
        x,
        y,
        w: v.w,
        h: v.h,
        angle: v.angle,
        iw: v.iw,
        ih: v.ih,
        cx: v.cx,
        cy: v.cy,
    }
}

pub fn to_sheet(placements: Vec<Placement>, film_w: i64, margin: i64) -> Sheet {
    let mut bottom = 0i64;
    for p in &placements {
        bottom = bottom.max(p.y + p.h);
    }
    Sheet { width_px: film_w, length_px: bottom + margin, placements }
}

/// Ângulos a testar para uma arte ([0] quando ela não pode girar).
/// Em retângulo, 180°+ dá a mesma caixa que o ângulo oposto; no contorno a forma muda.
pub fn angles_for(rotate: bool, s: &Settings) -> Vec<f64> {
    if !rotate && !s.force_rotate {
        return vec![0.0];
    }
    let limit = if s.contour { 360.0 } else { 180.0 };
    let mut out = Vec::new();
    let mut a = 0.0;
    while a < limit - 1e-9 {
        out.push(a);
        a += s.angle_step;
    }
    out
}

/// Caixa justa de um retângulo iw×ih girado (sem raster — só a matemática da caixa).
/// Para ângulos que não são múltiplos de 90°, a caixa da forma real depende dos
/// pixels (o app afina com alphaBox); aqui vai a caixa do retângulo girado.
pub fn variant_box(iw: i64, ih: i64, angle: f64) -> Variant {
    let a = angle.rem_euclid(360.0);
    if (a % 90.0).abs() < 1e-9 {
        let straight = (a % 180.0).abs() < 1e-9;
        let (w, h) = if straight { (iw, ih) } else { (ih, iw) };
        return Variant { angle: a, iw, ih, w, h, cx: w as f64 / 2.0, cy: h as f64 / 2.0 };
    }
    let rad = a.to_radians();
    let (tw, th) = (iw as f64, ih as f64);
    let w = (tw * rad.cos().abs() + th * rad.sin().abs()).ceil() as i64 + 2;
    let h = (tw * rad.sin().abs() + th * rad.cos().abs()).ceil() as i64 + 2;
    Variant { angle: a, iw, ih, w, h, cx: w as f64 / 2.0, cy: h as f64 / 2.0 }
}
