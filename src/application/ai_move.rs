use crate::engine::{
    build_engine_config, evaluate_move, is_board_coordinate_valid, make_seed, parse_runtime_rules,
    search_with_iterative_deepening, select_move_index, EngineConfig, EngineConfigPatch,
    EngineMove, SearchState, Side,
};
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::time::Instant;

#[derive(Debug)]
pub struct ComputeMoveCommand {
    pub game_id: String,
    pub move_no: u32,
    pub side_to_move: String,
    pub sfen: Option<String>,
    pub board_state: serde_json::Value,
    pub legal_moves: Vec<EngineMove>,
    pub config_patch: EngineConfigPatch,
}

#[derive(Debug)]
pub struct ComputeMoveResult {
    pub selected_move: EngineMove,
    pub meta: ComputeMoveMeta,
}

#[derive(Debug)]
pub struct ComputeMoveMeta {
    pub think_ms: u64,
    pub searched_nodes: u64,
    pub search_depth: u32,
    pub eval_cp: i32,
    pub candidate_count: usize,
    pub config_applied: EngineConfig,
}

#[derive(Debug)]
pub enum ComputeMoveError {
    InvalidEngineConfig(String),
    InvalidPosition(&'static str),
}

impl ComputeMoveError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidEngineConfig(_) => "INVALID_ENGINE_CONFIG",
            Self::InvalidPosition(_) => "INVALID_POSITION",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidEngineConfig(msg) => msg.clone(),
            Self::InvalidPosition(msg) => (*msg).to_string(),
        }
    }
}

pub fn compute_ai_move(input: ComputeMoveCommand) -> Result<ComputeMoveResult, ComputeMoveError> {
    let cfg = build_engine_config(input.config_patch)
        .map_err(|e| ComputeMoveError::InvalidEngineConfig(e.to_string()))?;

    let rules = parse_runtime_rules(&input.board_state);
    let start = Instant::now();
    let seed = cfg
        .random_seed
        .unwrap_or_else(|| make_seed(&input.game_id, input.move_no));
    let mut rng = StdRng::seed_from_u64(seed);
    let mut searched_nodes = 0_u64;
    let mut reached_depth = 1_u32;

    let (normalized_moves, mut scored) = if !input.legal_moves.is_empty() {
        let moves: Vec<EngineMove> = input
            .legal_moves
            .into_iter()
            .filter(is_board_coordinate_valid)
            .collect();
        let scores: Vec<(usize, i32)> = moves
            .iter()
            .enumerate()
            .map(|(idx, mv)| (idx, evaluate_move(mv, &input.side_to_move, &cfg)))
            .collect();
        (moves, scores)
    } else if let Some(sfen) = input.sfen {
        match SearchState::from_sfen(&sfen) {
            Ok(mut state) => {
                state.side_to_move = if input.side_to_move == "player" {
                    Side::Black
                } else {
                    Side::White
                };
                let (moves, scores, nodes, depth) =
                    search_with_iterative_deepening(&state, &cfg, &rules, start);
                searched_nodes = nodes;
                reached_depth = depth;
                (moves, scores)
            }
            Err(_) => {
                return Err(ComputeMoveError::InvalidPosition(
                    "legal_moves is empty and SFEN parse failed",
                ));
            }
        }
    } else {
        return Err(ComputeMoveError::InvalidPosition(
            "legal_moves is empty and sfen is missing",
        ));
    };

    if normalized_moves.is_empty() || scored.is_empty() {
        return Err(ComputeMoveError::InvalidPosition("no legal move available"));
    }

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    let best_score = scored[0].1;
    let selected_idx = select_move_index(&scored, best_score, &cfg, &mut rng);
    let selected_move = normalized_moves[selected_idx].clone();

    let think_ms = start.elapsed().as_millis() as u64;
    if searched_nodes == 0 {
        searched_nodes = scored.len().min(cfg.max_nodes as usize) as u64;
    }

    Ok(ComputeMoveResult {
        selected_move,
        meta: ComputeMoveMeta {
            think_ms,
            searched_nodes,
            search_depth: reached_depth.min(cfg.max_depth),
            eval_cp: best_score,
            candidate_count: scored.len(),
            config_applied: cfg,
        },
    })
}
