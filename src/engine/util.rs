use crate::engine::config::EngineConfig;
use rand::rngs::StdRng;
use rand::Rng;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn select_move_index(
    scored: &[(usize, i32)],
    best_score: i32,
    cfg: &EngineConfig,
    rng: &mut StdRng,
) -> usize {
    let top_k = usize::min(cfg.random_topk as usize, scored.len());
    let top = &scored[..top_k];

    let use_blunder = cfg.blunder_rate > 0.0 && rng.gen_bool(cfg.blunder_rate);
    if use_blunder {
        let allowed = top
            .iter()
            .copied()
            .filter(|(_, score)| best_score - score <= cfg.blunder_max_loss_cp as i32)
            .collect::<Vec<_>>();

        if !allowed.is_empty() {
            let choice = rng.gen_range(0..allowed.len());
            return allowed[choice].0;
        }
    }

    if cfg.temperature <= 0.0 || top.len() == 1 {
        return top[0].0;
    }

    let mut weights = Vec::with_capacity(top.len());
    let mut total = 0.0;

    for (_, score) in top.iter().copied() {
        let delta = (score - best_score) as f64;
        let w = (delta / cfg.temperature).exp().max(1e-6);
        total += w;
        weights.push(w);
    }

    let mut ticket = rng.gen_range(0.0..total);
    for (i, (idx, _)) in top.iter().copied().enumerate() {
        ticket -= weights[i];
        if ticket <= 0.0 {
            return idx;
        }
    }

    top[0].0
}

pub fn make_seed(game_id: &str, move_no: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    game_id.hash(&mut hasher);
    move_no.hash(&mut hasher);
    hasher.finish()
}
