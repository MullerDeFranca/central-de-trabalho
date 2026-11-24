//! Núcleo da Bancada: encaixe de artes em filme DTF (retangular e por contorno),
//! repetição máxima, cores e modelo de dados.
//!
//! Portado com fidelidade do Montador DTF (montador-dtf/index.html): mesmas
//! unidades (px na resolução do filme), mesmas sementes de PRNG e mesma ordem
//! de decisão, para o app novo reproduzir os resultados do antigo.

pub mod contorno;
pub mod cores;
pub mod editor;
pub mod modelo;
pub mod moldes;
pub mod repeticao;
pub mod retangular;
pub mod rng;
pub mod vetor;

pub use contorno::{mask_from_alpha, pack_contorno, ContourItem, MASK_SUPERSAMPLE};
pub use cores::{blend_colors, color_gap, from_oklab, hex_to_rgb, rgb_hex, to_oklab};
pub use modelo::*;
pub use repeticao::{repeticao_maxima_contorno, repeticao_maxima_rect, FillOutcome};
pub use retangular::{pack_rect, PackItem};
