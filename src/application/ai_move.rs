use crate::engine::{
    build_engine_config, evaluate_move, is_board_coordinate_valid, make_seed, parse_runtime_rules,
    score_move_with_skill_effects, search_with_iterative_deepening, select_move_index,
    EngineConfig, EngineConfigPatch, EngineMove, SearchState, Side,
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
    Checkmate,
}

impl ComputeMoveError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidEngineConfig(_) => "INVALID_ENGINE_CONFIG",
            Self::InvalidPosition(_) => "INVALID_POSITION",
            Self::Checkmate => "CHECKMATE",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidEngineConfig(msg) => msg.clone(),
            Self::InvalidPosition(msg) => (*msg).to_string(),
            Self::Checkmate => "no legal move available (checkmate)".to_string(),
        }
    }
}

pub fn compute_ai_move(input: ComputeMoveCommand) -> Result<ComputeMoveResult, ComputeMoveError> {
    let cfg = build_engine_config(input.config_patch)
        .map_err(|e| ComputeMoveError::InvalidEngineConfig(e.to_string()))?;

    let rules = parse_runtime_rules(&input.board_state)
        .map_err(|_| ComputeMoveError::InvalidPosition("invalid board_state runtime rules"))?;
    let start = Instant::now();
    let seed = cfg
        .random_seed
        .unwrap_or_else(|| make_seed(&input.game_id, input.move_no));
    let mut rng = StdRng::seed_from_u64(seed);
    let mut searched_nodes = 0_u64;
    let mut reached_depth = 1_u32;
    let mut parsed_state: Option<SearchState> = None;
    let normalized_legal: Vec<EngineMove> = input
        .legal_moves
        .into_iter()
        .filter(is_board_coordinate_valid)
        .collect();

    let (normalized_moves, mut scored) = if let Some(sfen) = input.sfen {
        match SearchState::from_sfen(&sfen) {
            Ok(mut state) => {
                state.side_to_move = Side::from_position_side(&input.side_to_move)
                    .ok_or(ComputeMoveError::InvalidPosition("invalid side_to_move"))?;
                state.hydrate_skill_state_from_board_state(&input.board_state);
                parsed_state = Some(state.clone());

                let (search_moves, search_scores, nodes, depth) =
                    search_with_iterative_deepening(&state, &cfg, &rules, start);
                searched_nodes = nodes;
                reached_depth = depth;

                if normalized_legal.is_empty() {
                    (search_moves, search_scores)
                } else {
                    let mut constrained_scores = Vec::new();
                    for (search_idx, score) in search_scores {
                        let search_mv = &search_moves[search_idx];
                        if let Some(legal_idx) = normalized_legal
                            .iter()
                            .position(|legal_mv| move_equals(legal_mv, search_mv))
                        {
                            constrained_scores.push((legal_idx, score));
                        }
                    }

                    if constrained_scores.is_empty() {
                        // SFEN と BFF 由来 legal_moves が噛み合わない場合は安全にフォールバック
                        let fallback =
                            score_moves_heuristically(&normalized_legal, &input.side_to_move, &cfg);
                        (normalized_legal, fallback)
                    } else {
                        (normalized_legal, constrained_scores)
                    }
                }
            }
            Err(_) if normalized_legal.is_empty() => {
                return Err(ComputeMoveError::InvalidPosition(
                    "legal_moves is empty and SFEN parse failed",
                ));
            }
            Err(_) => {
                let fallback =
                    score_moves_heuristically(&normalized_legal, &input.side_to_move, &cfg);
                (normalized_legal, fallback)
            }
        }
    } else if !normalized_legal.is_empty() {
        let fallback = score_moves_heuristically(&normalized_legal, &input.side_to_move, &cfg);
        (normalized_legal, fallback)
    } else {
        return Err(ComputeMoveError::InvalidPosition(
            "legal_moves is empty and sfen is missing",
        ));
    };

    if normalized_moves.is_empty() || scored.is_empty() {
        return Err(ComputeMoveError::Checkmate);
    }

    if let Some(state) = parsed_state.as_ref() {
        for (idx, score) in &mut scored {
            *score += score_move_with_skill_effects(state, &normalized_moves[*idx], &rules, &cfg);
        }
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

fn score_moves_heuristically(
    moves: &[EngineMove],
    side_to_move: &str,
    cfg: &EngineConfig,
) -> Vec<(usize, i32)> {
    moves
        .iter()
        .enumerate()
        .map(|(idx, mv)| (idx, evaluate_move(mv, side_to_move, cfg)))
        .collect()
}

fn move_equals(lhs: &EngineMove, rhs: &EngineMove) -> bool {
    lhs.from_row == rhs.from_row
        && lhs.from_col == rhs.from_col
        && lhs.to_row == rhs.to_row
        && lhs.to_col == rhs.to_col
        && lhs.promote == rhs.promote
        && opt_str_eq(
            lhs.drop_piece_code.as_deref(),
            rhs.drop_piece_code.as_deref(),
        )
        && lhs.piece_code.eq_ignore_ascii_case(&rhs.piece_code)
}

fn opt_str_eq(lhs: Option<&str>, rhs: Option<&str>) -> bool {
    match (lhs, rhs) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        (None, None) => true,
        _ => false,
    }
}
