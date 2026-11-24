//! Repetição máxima ("fill"): maior quantidade das artes marcadas que cabe numa
//! folha de até L cm, por busca binária sobre o encaixe real. Portado do
//! fillPack do Montador DTF.

use crate::contorno::{pack_contorno, ContourItem};
use crate::modelo::*;
use crate::retangular::{pack_rect, PackItem};
use std::collections::HashMap;
use std::time::Duration;

pub struct FillOutcome {
    pub result: PackResult,
    /// Quantidades finais das artes "fill", na ordem em que aparecem na lista.
    pub quantities: Vec<u32>,
    /// As artes de quantidade fixa já passam do comprimento escolhido.
    pub fixed_too_long: bool,
    /// Comprimento final da folha (cm).
    pub len_cm: f64,
}

/// Núcleo genérico: `eval(qtd, budget)` encaixa as artes com as quantidades
/// dadas (somente as artes fill mudam; as fixas já estão embutidas no fecho).
#[allow(clippy::too_many_arguments)]
fn fill_core(
    s: &Settings,
    ratios: &[u32],
    areas: &[f64],
    fixed_area: f64,
    len_cm: f64,
    factor: f64,
    top_off: bool,
    try_budget: Duration,
    full_budget: Duration,
    mut eval: impl FnMut(&[u32], Duration) -> PackResult,
) -> FillOutcome {
    let lpx = cm_to_px(len_cm, s.dpi);
    let margin = cm_to_px(s.margin_cm, s.dpi);
    let usable = (cm_to_px(s.width_cm, s.dpi) - 2 * margin).max(1) as f64
        * (lpx - 2 * margin).max(1) as f64;
    let n = ratios.len();

    let fits = |res: &PackResult| {
        res.too_big.is_empty()
            && res.sheets.len() <= 1
            && (res.sheets.is_empty() || res.sheets[0].length_px <= lpx)
    };

    let mut eval_q = |q: &[u32], budget: Duration, results: &mut HashMap<String, PackResult>| -> bool {
        let res = eval(q, budget);
        let ok = fits(&res);
        if ok {
            results.insert(
                q.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","),
                res,
            );
        }
        ok
    };

    let zeros = vec![0u32; n];
    let mut local = HashMap::new();
    let r0 = eval_q(&zeros, try_budget, &mut local);
    let mut q = zeros.clone();
    let fixed_too_long = !r0;

    if !fixed_too_long {
        let per: f64 = ratios.iter().zip(areas).map(|(r, a)| *r as f64 * a).sum();
        if per > 0.0 {
            let mut lo: u64 = 0;
            let mut hi: u64 = (((usable * factor - fixed_area) / per).floor().max(0.0) as u64) + 1;
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                let qt: Vec<u32> = ratios.iter().map(|r| r * mid as u32).collect();
                if eval_q(&qt, try_budget, &mut local) {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            q = ratios.iter().map(|r| r * lo as u32).collect();
        }
        // Sem proporção: completa os vãos, primeiro com as logos maiores.
        if top_off {
            let order = {
                let mut o: Vec<usize> = (0..n).collect();
                o.sort_by(|&a, &b| areas[b].partial_cmp(&areas[a]).unwrap());
                o
            };
            for i in order {
                let used: f64 = fixed_area
                    + q.iter().zip(areas).map(|(qq, a)| *qq as f64 * a).sum::<f64>();
                let mut elo: u64 = 0;
                let mut ehi: u64 =
                    (((usable * factor - used) / areas[i]).floor().max(0.0) as u64) + 1;
                while elo < ehi {
                    let mid = (elo + ehi).div_ceil(2);
                    let mut q2 = q.clone();
                    q2[i] += mid as u32;
                    if eval_q(&q2, try_budget, &mut local) {
                        elo = mid;
                    } else {
                        ehi = mid - 1;
                    }
                }
                q[i] += elo as u32;
            }
        }
    }

    // Encaixe final com o tempo normal; fica com o melhor entre ele e o da busca.
    let mut res = eval(&q, full_budget);
    let key = q.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    if let Some(found) = local.remove(&key) {
        let prefer_found = !fits(&res)
            || match (res.sheets.first(), found.sheets.first()) {
                (Some(a), Some(b)) => b.length_px < a.length_px,
                _ => false,
            };
        if prefer_found {
            res = found;
        }
    }

    let len_cm = res
        .sheets
        .first()
        .map(|sh| px_to_cm(sh.length_px as f64, s.dpi))
        .unwrap_or(0.0);
    FillOutcome { result: res, quantities: q, fixed_too_long, len_cm }
}

/// No Montador, o modo "com proporção" é uma escolha da interface; quem chama
/// passa `top_off = true` no modo sem proporção (completar vãos) e `false`
/// no modo balanceado.

fn area_of(spec: &ItemSpec, s: &Settings, gap: i64) -> f64 {
    let iw = cm_to_px(spec.w_cm, s.dpi) + gap;
    let ih = cm_to_px(spec.h_cm, s.dpi) + gap;
    (iw * ih) as f64
}

/// Repetição máxima com encaixe retangular. `balanced` = respeitar a proporção
/// (ratio) entre as artes fill; sem proporção, completa os vãos com mais cópias.
pub fn repeticao_maxima_rect(
    s: &Settings,
    items: &[PackItem],
    len_cm: f64,
    balanced: bool,
) -> FillOutcome {
    let gap = cm_to_px(s.gap_cm, s.dpi);
    let fills: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| it.spec.fill && it.spec.ratio > 0)
        .map(|(i, _)| i)
        .collect();
    let ratios: Vec<u32> = fills
        .iter()
        .map(|&i| if balanced { items[i].spec.ratio.max(1) } else { 1 })
        .collect();
    let areas: Vec<f64> = fills.iter().map(|&i| area_of(&items[i].spec, s, gap)).collect();
    let fixed_area: f64 = items
        .iter()
        .enumerate()
        .filter(|(i, _)| !fills.contains(i))
        .map(|(_, it)| it.spec.qty as f64 * area_of(&it.spec, s, gap))
        .sum();

    let eval = |q: &[u32], budget: Duration| -> PackResult {
        let mut its: Vec<PackItem> = items.to_vec();
        for (k, &i) in fills.iter().enumerate() {
            its[i].spec.qty = q[k];
        }
        pack_rect(s, &its, budget)
    };
    fill_core(
        s,
        &ratios,
        &areas,
        fixed_area,
        len_cm,
        1.05,
        !balanced,
        Duration::from_millis(30),
        Duration::from_millis(300),
        eval,
    )
}

/// Repetição máxima com encaixe por contorno.
pub fn repeticao_maxima_contorno(
    s: &Settings,
    items: &[ContourItem],
    len_cm: f64,
    balanced: bool,
    cell: Option<f64>,
) -> FillOutcome {
    let gap = cm_to_px(s.gap_cm, s.dpi);
    let fills: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| it.spec.fill && it.spec.ratio > 0)
        .map(|(i, _)| i)
        .collect();
    let ratios: Vec<u32> = fills
        .iter()
        .map(|&i| if balanced { items[i].spec.ratio.max(1) } else { 1 })
        .collect();
    let areas: Vec<f64> = fills.iter().map(|&i| area_of(&items[i].spec, s, gap)).collect();
    let fixed_area: f64 = items
        .iter()
        .enumerate()
        .filter(|(i, _)| !fills.contains(i))
        .map(|(_, it)| it.spec.qty as f64 * area_of(&it.spec, s, gap))
        .sum();

    let eval = |q: &[u32], budget: Duration| -> PackResult {
        let mut its: Vec<ContourItem> = items.to_vec();
        for (k, &i) in fills.iter().enumerate() {
            its[i].spec.qty = q[k];
        }
        pack_contorno(s, &its, budget, cell)
    };
    fill_core(
        s,
        &ratios,
        &areas,
        fixed_area,
        len_cm,
        2.0,
        !balanced,
        Duration::from_millis(120),
        Duration::from_millis(1500),
        eval,
    )
}
