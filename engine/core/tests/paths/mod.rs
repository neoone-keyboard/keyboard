//! Synthetic swipe paths for the swipe tests (`swipe_eval`, `swipe_mix`): a finger's path for
//! a word, each key hit off-centre, corners cut, a little shake, from a fixed seed.

use mlengine_core::swipe::KeyMap;

/// xorshift64*, so the paths are the same on every run.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        ((self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    pub fn gauss(&mut self) -> f32 {
        let (u, v) = (self.next().max(1e-7), self.next());
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }
}

/// A seed from a name (FNV-1a): the same on every run, never zero.
pub fn seed(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    }) | 1
}

/// How rough a path is.
#[derive(Debug, Clone, Copy)]
pub struct Noise {
    /// How far each key's centre is missed, in key widths (Gaussian).
    pub aim: f32,
    /// Sideways jitter of each point, in key widths (Gaussian).
    pub shake: f32,
    /// How much of `MAX_CUT` a corner may be cut by (1: anything up to it).
    pub cut: f32,
}

/// The swipe eval's paths.
#[allow(dead_code)]
pub const NOISE: Noise = Noise {
    aim: 0.22,
    shake: 0.04,
    cut: 1.0,
};

/// A finger's path for `word` on keys one unit wide: each key's centre moved by `aim` key
/// widths (Gaussian), corners pulled up to `MAX_CUT` toward their neighbours, then points
/// every quarter key with `shake` of sideways jitter.
pub fn noisy_path(keys: &KeyMap, word: &str, rng: &mut Rng, noise: Noise) -> Vec<(f32, f32)> {
    /// The farthest a corner is cut, in key widths.
    const MAX_CUT: f32 = 0.4;
    let Noise { aim, shake, cut } = noise;
    let mut targets: Vec<(f32, f32)> = Vec::new();
    for b in word.bytes() {
        let Some(c) = keys.centre(b) else { continue };
        if targets.last().is_some_and(|&l| l == c) {
            continue;
        }
        targets.push(c);
    }
    let mut aimed: Vec<(f32, f32)> = targets
        .iter()
        .map(|&(x, y)| (x + aim * rng.gauss(), y + aim * rng.gauss()))
        .collect();
    for i in 1..aimed.len().saturating_sub(1) {
        let (a, b, c) = (aimed[i - 1], aimed[i], aimed[i + 1]);
        let mid = ((a.0 + c.0) / 2.0, (a.1 + c.1) / 2.0);
        // A finger rounds a corner but still reaches near the key: the bend marks the letter.
        let (dx, dy) = (mid.0 - b.0, mid.1 - b.1);
        let reach = (dx * dx + dy * dy).sqrt();
        let pull = (cut * MAX_CUT * rng.next()).min(reach) / reach.max(1e-6);
        aimed[i] = (b.0 + dx * pull, b.1 + dy * pull);
    }
    let mut path = Vec::new();
    for w in aimed.windows(2) {
        let (a, b) = (w[0], w[1]);
        let length = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let steps = (length * 4.0).ceil().max(1.0) as usize;
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            path.push((
                a.0 + (b.0 - a.0) * t + shake * rng.gauss(),
                a.1 + (b.1 - a.1) * t + shake * rng.gauss(),
            ));
        }
    }
    path.extend(aimed.last());
    path
}
