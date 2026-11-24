//! PRNG determinístico (mulberry32), igual ao do Montador: mesma semente,
//! mesma sequência de trocas na busca local.

pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed)
    }
    /// Devolve f64 em [0, 1).
    pub fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6D2B79F5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61))) ^ t;
        ((t ^ (t >> 14)) as u32) as f64 / 4294967296.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() * n as f64).floor() as usize
    }
}
