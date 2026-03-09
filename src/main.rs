use axum::{http::StatusCode, response::IntoResponse, routing::get, routing::post, Json, Router};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::time::Instant;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
struct EngineMoveRequest {
    game_id: String,
    move_no: u32,
    position: PositionInput,
    #[serde(default)]
    engine_config: EngineConfigInput,
}

#[derive(Debug, Deserialize, Clone)]
struct PositionInput {
    side_to_move: String,
    turn_number: u32,
    move_count: u32,
    #[serde(default)]
    sfen: Option<String>,
    #[serde(default)]
    state_hash: Option<String>,
    #[serde(default)]
    board_state: serde_json::Value,
    #[serde(default)]
    hands: serde_json::Value,
    legal_moves: Vec<MoveInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MoveInput {
    from_row: Option<i32>,
    from_col: Option<i32>,
    to_row: i32,
    to_col: i32,
    piece_code: String,
    #[serde(default)]
    promote: bool,
    #[serde(default)]
    drop_piece_code: Option<String>,
    #[serde(default)]
    captured_piece_code: Option<String>,
    #[serde(default)]
    notation: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct EngineConfigInput {
    max_depth: Option<u32>,
    max_nodes: Option<u32>,
    time_limit_ms: Option<u32>,
    quiescence_enabled: Option<bool>,
    eval_material_weight: Option<f64>,
    eval_position_weight: Option<f64>,
    eval_king_safety_weight: Option<f64>,
    eval_mobility_weight: Option<f64>,
    blunder_rate: Option<f64>,
    blunder_max_loss_cp: Option<u32>,
    random_topk: Option<u32>,
    temperature: Option<f64>,
    always_legal_move: Option<bool>,
    mate_avoidance: Option<bool>,
    max_repeat_draw_bias: Option<f64>,
    random_seed: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct EngineConfig {
    max_depth: u32,
    max_nodes: u32,
    time_limit_ms: u32,
    quiescence_enabled: bool,
    eval_material_weight: f64,
    eval_position_weight: f64,
    eval_king_safety_weight: f64,
    eval_mobility_weight: f64,
    blunder_rate: f64,
    blunder_max_loss_cp: u32,
    random_topk: u32,
    temperature: f64,
    always_legal_move: bool,
    mate_avoidance: bool,
    max_repeat_draw_bias: f64,
    random_seed: Option<u64>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_nodes: 20_000,
            time_limit_ms: 300,
            quiescence_enabled: true,
            eval_material_weight: 1.0,
            eval_position_weight: 0.35,
            eval_king_safety_weight: 0.25,
            eval_mobility_weight: 0.2,
            blunder_rate: 0.0,
            blunder_max_loss_cp: 0,
            random_topk: 1,
            temperature: 0.0,
            always_legal_move: true,
            mate_avoidance: true,
            max_repeat_draw_bias: 0.0,
            random_seed: None,
        }
    }
}

#[derive(Debug, Serialize)]
struct EngineMoveResponse {
    selected_move: MoveInput,
    meta: EngineMeta,
}

#[derive(Debug, Serialize)]
struct EngineMeta {
    engine_version: &'static str,
    think_ms: u64,
    searched_nodes: u64,
    search_depth: u32,
    eval_cp: i32,
    candidate_count: usize,
    config_applied: EngineConfig,
}

#[derive(Debug, Serialize)]
struct ApiError {
    code: &'static str,
    message: String,
}

#[derive(Debug, Error)]
enum ConfigError {
    #[error("{0}")]
    Invalid(String),
}

#[tokio::main]
async fn main() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,ai_request=info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .init();

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/ai/move", post(post_ai_move));

    let bind = std::env::var("AI_ENGINE_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let addr: SocketAddr = bind.parse().expect("invalid AI_ENGINE_BIND");

    info!("shogi-ai listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");
    axum::serve(listener, app).await.expect("server error");
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "ok": true }))
}

async fn post_ai_move(Json(payload): Json<EngineMoveRequest>) -> impl IntoResponse {
    info!(
        target: "ai_request",
        game_id = %payload.game_id,
        move_no = payload.move_no,
        side_to_move = %payload.position.side_to_move,
        has_legal_moves = !payload.position.legal_moves.is_empty(),
        has_sfen = payload.position.sfen.is_some(),
        "received /v1/ai/move"
    );

    let cfg = match build_engine_config(payload.engine_config) {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!(
                target: "ai_request",
                game_id = %payload.game_id,
                move_no = payload.move_no,
                error = %e,
                "invalid engine config"
            );
            return err(StatusCode::BAD_REQUEST, "INVALID_ENGINE_CONFIG", e.to_string());
        }
    };
    let rules = parse_runtime_rules(&payload.position.board_state);

    let start = Instant::now();
    let position_ctx = payload.position.clone();
    let seed = cfg.random_seed.unwrap_or_else(|| make_seed(&payload.game_id, payload.move_no));
    let mut rng = StdRng::seed_from_u64(seed);
    let mut searched_nodes = 0_u64;
    let mut reached_depth = 1_u32;

    let (normalized_moves, mut scored) = if !payload.position.legal_moves.is_empty() {
        let moves: Vec<MoveInput> = payload
            .position
            .legal_moves
            .into_iter()
            .filter(is_board_coordinate_valid)
            .collect();
        let scores: Vec<(usize, i32)> = moves
            .iter()
            .enumerate()
            .map(|(idx, mv)| (idx, evaluate_move(mv, &position_ctx, &cfg)))
            .collect();
        (moves, scores)
    } else if let Some(sfen) = payload.position.sfen.clone() {
        match SearchState::from_sfen(&sfen) {
            Ok(mut state) => {
                state.side_to_move = if payload.position.side_to_move == "player" {
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
                warn!(
                    target: "ai_request",
                    game_id = %payload.game_id,
                    move_no = payload.move_no,
                    "invalid position: legal_moves empty and SFEN parse failed"
                );
                return err(
                    StatusCode::BAD_REQUEST,
                    "INVALID_POSITION",
                    "legal_moves is empty and SFEN parse failed",
                );
            }
        }
    } else {
        warn!(
            target: "ai_request",
            game_id = %payload.game_id,
            move_no = payload.move_no,
            "invalid position: legal_moves empty and sfen missing"
        );
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_POSITION",
            "legal_moves is empty and sfen is missing",
        );
    };

    if normalized_moves.is_empty() || scored.is_empty() {
        warn!(
            target: "ai_request",
            game_id = %payload.game_id,
            move_no = payload.move_no,
            "no legal moves available"
        );
        return err(StatusCode::BAD_REQUEST, "INVALID_POSITION", "no legal move available");
    }

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    let best_score = scored[0].1;
    let selected_idx = select_move_index(&scored, best_score, &cfg, &mut rng);
    let selected_move = normalized_moves[selected_idx].clone();

    let think_ms = start.elapsed().as_millis() as u64;
    if searched_nodes == 0 {
        searched_nodes = scored.len().min(cfg.max_nodes as usize) as u64;
    }

    info!(
        target: "ai_request",
        game_id = %payload.game_id,
        move_no = payload.move_no,
        selected_piece = %selected_move.piece_code,
        to_row = selected_move.to_row,
        to_col = selected_move.to_col,
        eval_cp = best_score,
        depth = reached_depth.min(cfg.max_depth),
        nodes = searched_nodes,
        think_ms = think_ms,
        "completed /v1/ai/move"
    );

    (
        StatusCode::OK,
        Json(EngineMoveResponse {
            selected_move,
            meta: EngineMeta {
                engine_version: env!("CARGO_PKG_VERSION"),
                think_ms,
                searched_nodes,
                search_depth: reached_depth.min(cfg.max_depth),
                eval_cp: best_score,
                candidate_count: scored.len(),
                config_applied: cfg,
            },
        }),
    )
        .into_response()
}

fn select_move_index(
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

fn evaluate_move(mv: &MoveInput, position: &PositionInput, cfg: &EngineConfig) -> i32 {
    let capture_value = mv
        .captured_piece_code
        .as_deref()
        .map(piece_capture_cp)
        .unwrap_or(0) as f64;

    let promote_bonus = if mv.promote { 60.0 } else { 0.0 };

    let center_dist = ((mv.to_row - 4).abs() + (mv.to_col - 4).abs()) as f64;
    let center_bonus = 8.0 - center_dist;

    let side_bias = if position.side_to_move == "enemy" { 1.0 } else { -1.0 };

    let positional = (promote_bonus + center_bonus * 3.0) * cfg.eval_position_weight;
    let material = capture_value * cfg.eval_material_weight;
    let mobility = 5.0 * cfg.eval_mobility_weight;
    let king_safety = 2.0 * cfg.eval_king_safety_weight;

    let _unused_for_phase1 = (
        cfg.quiescence_enabled,
        cfg.max_repeat_draw_bias,
        position.turn_number,
        position.move_count,
        &position.sfen,
        &position.state_hash,
        &position.board_state,
        &position.hands,
    );

    ((material + positional + mobility + king_safety) * side_bias) as i32
}

fn piece_capture_cp(piece_code: &str) -> i32 {
    match piece_code {
        "OU" => 10_000,
        "HI" | "KA" => 900,
        "KI" => 600,
        "GI" => 500,
        "KE" | "KY" => 350,
        "FU" => 100,
        _ => 150,
    }
}

fn is_board_coordinate_valid(mv: &MoveInput) -> bool {
    let to_ok = (0..=8).contains(&mv.to_row) && (0..=8).contains(&mv.to_col);
    if !to_ok {
        return false;
    }

    match (mv.from_row, mv.from_col, mv.drop_piece_code.as_ref()) {
        (Some(r), Some(c), None) => (0..=8).contains(&r) && (0..=8).contains(&c),
        (None, None, Some(_)) => true,
        _ => false,
    }
}

fn make_seed(game_id: &str, move_no: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    game_id.hash(&mut hasher);
    move_no.hash(&mut hasher);
    hasher.finish()
}

fn build_engine_config(input: EngineConfigInput) -> Result<EngineConfig, ConfigError> {
    let mut cfg = EngineConfig::default();

    if let Some(v) = input.max_depth {
        ensure_range_u32("max_depth", v, 1, 12)?;
        cfg.max_depth = v;
    }
    if let Some(v) = input.max_nodes {
        ensure_range_u32("max_nodes", v, 100, 5_000_000)?;
        cfg.max_nodes = v;
    }
    if let Some(v) = input.time_limit_ms {
        ensure_range_u32("time_limit_ms", v, 10, 60_000)?;
        cfg.time_limit_ms = v;
    }
    if let Some(v) = input.quiescence_enabled {
        cfg.quiescence_enabled = v;
    }
    if let Some(v) = input.eval_material_weight {
        ensure_range_f64("eval_material_weight", v, 0.0, 10.0)?;
        cfg.eval_material_weight = v;
    }
    if let Some(v) = input.eval_position_weight {
        ensure_range_f64("eval_position_weight", v, 0.0, 10.0)?;
        cfg.eval_position_weight = v;
    }
    if let Some(v) = input.eval_king_safety_weight {
        ensure_range_f64("eval_king_safety_weight", v, 0.0, 10.0)?;
        cfg.eval_king_safety_weight = v;
    }
    if let Some(v) = input.eval_mobility_weight {
        ensure_range_f64("eval_mobility_weight", v, 0.0, 10.0)?;
        cfg.eval_mobility_weight = v;
    }
    if let Some(v) = input.blunder_rate {
        ensure_range_f64("blunder_rate", v, 0.0, 1.0)?;
        cfg.blunder_rate = v;
    }
    if let Some(v) = input.blunder_max_loss_cp {
        ensure_range_u32("blunder_max_loss_cp", v, 0, 3000)?;
        cfg.blunder_max_loss_cp = v;
    }
    if let Some(v) = input.random_topk {
        ensure_range_u32("random_topk", v, 1, 20)?;
        cfg.random_topk = v;
    }
    if let Some(v) = input.temperature {
        ensure_range_f64("temperature", v, 0.0, 2.0)?;
        cfg.temperature = v;
    }
    if let Some(v) = input.max_repeat_draw_bias {
        ensure_range_f64("max_repeat_draw_bias", v, -1.0, 1.0)?;
        cfg.max_repeat_draw_bias = v;
    }

    cfg.random_seed = input.random_seed;

    if let Some(v) = input.always_legal_move {
        if !v {
            return Err(ConfigError::Invalid(
                "always_legal_move must be true".to_string(),
            ));
        }
    }
    if let Some(v) = input.mate_avoidance {
        if !v {
            return Err(ConfigError::Invalid("mate_avoidance must be true".to_string()));
        }
    }

    Ok(cfg)
}

fn ensure_range_u32(name: &str, value: u32, min: u32, max: u32) -> Result<(), ConfigError> {
    if value < min || value > max {
        return Err(ConfigError::Invalid(format!(
            "{} must be in {}..={} (got {})",
            name, min, max, value
        )));
    }
    Ok(())
}

fn ensure_range_f64(name: &str, value: f64, min: f64, max: f64) -> Result<(), ConfigError> {
    if !value.is_finite() || value < min || value > max {
        return Err(ConfigError::Invalid(format!(
            "{} must be in {}..={} (got {})",
            name, min, max, value
        )));
    }
    Ok(())
}

fn err(status: StatusCode, code: &'static str, message: impl Into<String>) -> axum::response::Response {
    (
        status,
        Json(ApiError {
            code,
            message: message.into(),
        }),
    )
        .into_response()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Black,
    White,
}

impl Side {
    fn opposite(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PieceKind {
    Pawn,
    Lance,
    Knight,
    Silver,
    Gold,
    Bishop,
    Rook,
    King,
}

#[derive(Debug, Clone, Copy)]
struct Piece {
    side: Side,
    kind: PieceKind,
    promoted: bool,
}

#[derive(Debug, Clone)]
struct SearchState {
    board: [Option<Piece>; 81],
    side_to_move: Side,
    hands: [[u8; 7]; 2],
}

#[derive(Debug, Clone)]
struct GenMove {
    from: Option<(usize, usize)>,
    to: (usize, usize),
    piece: Piece,
    promote: bool,
    capture: Option<Piece>,
    drop: Option<PieceKind>,
}

#[derive(Debug, Clone, Copy)]
struct VectorRule {
    dr: i32,
    dc: i32,
    slide: bool,
}

#[derive(Debug, Clone, Default)]
struct RuntimeRules {
    extra_vectors_by_piece: HashMap<String, Vec<VectorRule>>,
    eval_bonus_by_piece: HashMap<String, i32>,
}

impl SearchState {
    fn from_sfen(sfen: &str) -> Result<Self, String> {
        let mut parts = sfen.split_whitespace();
        let board_part = parts.next().ok_or("missing board")?;
        let side_part = parts.next().ok_or("missing side")?;
        let hands_part = parts.next().unwrap_or("-");

        let mut board: [Option<Piece>; 81] = [None; 81];
        for (row, rank) in board_part.split('/').enumerate() {
            if row >= 9 {
                return Err("too many ranks".to_string());
            }
            let mut col = 0usize;
            let mut chars = rank.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch.is_ascii_digit() {
                    col += ch.to_digit(10).ok_or("invalid digit")? as usize;
                    continue;
                }
                let promoted = if ch == '+' {
                    true
                } else {
                    false
                };
                let pch = if promoted {
                    chars.next().ok_or("invalid promoted piece")?
                } else {
                    ch
                };
                if col >= 9 {
                    return Err("column overflow".to_string());
                }
                let side = if pch.is_ascii_uppercase() {
                    Side::Black
                } else {
                    Side::White
                };
                let piece = Piece {
                    side,
                    kind: piece_kind_from_char(pch).ok_or("invalid piece")?,
                    promoted,
                };
                board[row * 9 + col] = Some(piece);
                col += 1;
            }
            if col != 9 {
                return Err("rank width mismatch".to_string());
            }
        }
        let side_to_move = if side_part == "b" {
            Side::Black
        } else {
            Side::White
        };
        let hands = parse_sfen_hands(hands_part)?;
        Ok(Self {
            board,
            side_to_move,
            hands,
        })
    }
}

fn search_with_iterative_deepening(
    state: &SearchState,
    cfg: &EngineConfig,
    rules: &RuntimeRules,
    start: Instant,
) -> (Vec<MoveInput>, Vec<(usize, i32)>, u64, u32) {
    let mut nodes = 0u64;
    let root = generate_legal_moves(state, rules, true);
    let root_inputs: Vec<MoveInput> = root.iter().map(to_move_input).collect();
    if root.is_empty() {
        return (Vec::new(), Vec::new(), 0, 1);
    }

    let mut last_scores: Vec<i32> = root
        .iter()
        .map(|mv| {
            let next = apply_move(state, mv);
            -evaluate_state(&next, cfg, rules)
        })
        .collect();
    let mut reached_depth = 1;

    for depth in 2..=cfg.max_depth {
        if nodes >= cfg.max_nodes as u64 || start.elapsed().as_millis() as u32 >= cfg.time_limit_ms {
            break;
        }
        let mut depth_scores = Vec::with_capacity(root.len());
        for mv in &root {
            if nodes >= cfg.max_nodes as u64 || start.elapsed().as_millis() as u32 >= cfg.time_limit_ms {
                break;
            }
            let next = apply_move(state, mv);
            let score = -negamax(
                &next,
                depth - 1,
                -30000,
                30000,
                cfg,
                rules,
                start,
                &mut nodes,
            );
            depth_scores.push(score);
        }
        if depth_scores.len() == root.len() {
            last_scores = depth_scores;
            reached_depth = depth;
        } else {
            break;
        }
    }

    let scored = last_scores
        .into_iter()
        .enumerate()
        .map(|(idx, score)| (idx, score))
        .collect();
    (root_inputs, scored, nodes.max(root.len() as u64), reached_depth)
}

fn negamax(
    state: &SearchState,
    depth: u32,
    mut alpha: i32,
    beta: i32,
    cfg: &EngineConfig,
    rules: &RuntimeRules,
    start: Instant,
    nodes: &mut u64,
) -> i32 {
    if depth == 0 || *nodes >= cfg.max_nodes as u64 || start.elapsed().as_millis() as u32 >= cfg.time_limit_ms {
        return evaluate_state(state, cfg, rules);
    }
    *nodes += 1;

    let moves = generate_legal_moves(state, rules, true);
    if moves.is_empty() {
        return -29000 + depth as i32;
    }

    let mut best = -30000;
    for mv in &moves {
        let next = apply_move(state, mv);
        let score = -negamax(&next, depth - 1, -beta, -alpha, cfg, rules, start, nodes);
        if score > best {
            best = score;
        }
        if score > alpha {
            alpha = score;
        }
        if alpha >= beta {
            break;
        }
    }
    best
}

fn evaluate_state(state: &SearchState, cfg: &EngineConfig, rules: &RuntimeRules) -> i32 {
    let mut material = 0.0;
    let mut mobility = 0.0;
    let mut center = 0.0;

    for row in 0..9 {
        for col in 0..9 {
            if let Some(p) = state.board[row * 9 + col] {
                let v = piece_base_value(p.kind) as f64 + if p.promoted { 80.0 } else { 0.0 };
                let s = if p.side == state.side_to_move { 1.0 } else { -1.0 };
                material += v * s;
                let bonus = rules
                    .eval_bonus_by_piece
                    .get(piece_code(p.kind))
                    .copied()
                    .unwrap_or(0) as f64;
                material += bonus * s;
                center += (8.0 - ((row as i32 - 4).abs() + (col as i32 - 4).abs()) as f64) * s;
            }
        }
    }
    mobility += generate_legal_moves(state, rules, false).len() as f64;

    (material * cfg.eval_material_weight
        + center * cfg.eval_position_weight
        + mobility * cfg.eval_mobility_weight) as i32
}

fn generate_legal_moves(state: &SearchState, rules: &RuntimeRules, enforce_uchifuzume: bool) -> Vec<GenMove> {
    let pseudo = generate_pseudo_moves(state, rules);
    let mut legal = Vec::new();
    for mv in pseudo {
        let next = apply_move(state, &mv);
        if is_in_check(&next, state.side_to_move, rules) {
            continue;
        }
        if enforce_uchifuzume && is_uchi_fuzume(state, &mv, rules) {
            continue;
        }
        legal.push(mv);
    }
    legal
}

fn generate_pseudo_moves(state: &SearchState, rules: &RuntimeRules) -> Vec<GenMove> {
    let mut moves = Vec::new();
    for row in 0..9 {
        for col in 0..9 {
            let idx = row * 9 + col;
            let Some(piece) = state.board[idx] else { continue };
            if piece.side != state.side_to_move {
                continue;
            }
            gen_piece_moves(state, row, col, piece, rules, &mut moves);
        }
    }
    gen_drop_moves(state, &mut moves);
    moves
}

fn gen_piece_moves(
    state: &SearchState,
    row: usize,
    col: usize,
    piece: Piece,
    rules: &RuntimeRules,
    out: &mut Vec<GenMove>,
) {
    let fwd = if piece.side == Side::Black { -1 } else { 1 };
    let gold_dirs = [(fwd, 0), (fwd, -1), (fwd, 1), (0, -1), (0, 1), (-fwd, 0)];
    let king_dirs = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)];
    let silver_dirs = [(fwd, -1), (fwd, 0), (fwd, 1), (-fwd, -1), (-fwd, 1)];

    let mut push_step = |dr: i32, dc: i32, slide: bool| {
        let mut r = row as i32 + dr;
        let mut c = col as i32 + dc;
        while (0..=8).contains(&r) && (0..=8).contains(&c) {
            let tidx = r as usize * 9 + c as usize;
            if let Some(tp) = state.board[tidx] {
                if tp.side == piece.side {
                    break;
                }
                push_promote_variants(out, make_gen_move((row, col), (r as usize, c as usize), piece, Some(tp)));
                break;
            }
            push_promote_variants(out, make_gen_move((row, col), (r as usize, c as usize), piece, None));
            if !slide {
                break;
            }
            r += dr;
            c += dc;
        }
    };

    if piece.promoted
        && matches!(piece.kind, PieceKind::Pawn | PieceKind::Lance | PieceKind::Knight | PieceKind::Silver)
    {
        for (dr, dc) in gold_dirs {
            push_step(dr, dc, false);
        }
        return;
    }

    match piece.kind {
        PieceKind::Pawn => push_step(fwd, 0, false),
        PieceKind::Lance => push_step(fwd, 0, true),
        PieceKind::Knight => {
            push_step(fwd * 2, -1, false);
            push_step(fwd * 2, 1, false);
        }
        PieceKind::Silver => {
            for (dr, dc) in silver_dirs {
                push_step(dr, dc, false);
            }
        }
        PieceKind::Gold => {
            for (dr, dc) in gold_dirs {
                push_step(dr, dc, false);
            }
        }
        PieceKind::Bishop => {
            for (dr, dc) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
                push_step(dr, dc, true);
            }
            if piece.promoted {
                for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    push_step(dr, dc, false);
                }
            }
        }
        PieceKind::Rook => {
            for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                push_step(dr, dc, true);
            }
            if piece.promoted {
                for (dr, dc) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
                    push_step(dr, dc, false);
                }
            }
        }
        PieceKind::King => {
            for (dr, dc) in king_dirs {
                push_step(dr, dc, false);
            }
        }
    }

    if let Some(extra) = rules.extra_vectors_by_piece.get(piece_code(piece.kind)) {
        for v in extra {
            push_step(v.dr, v.dc, v.slide);
        }
    }
}

fn make_gen_move(from: (usize, usize), to: (usize, usize), piece: Piece, capture: Option<Piece>) -> GenMove {
    let can_promote = piece_promotable(piece.kind)
        && !piece.promoted
        && (is_promotion_zone(piece.side, from.0) || is_promotion_zone(piece.side, to.0));
    GenMove {
        from: Some(from),
        to,
        piece,
        promote: can_promote && must_promote(piece, to.0),
        capture,
        drop: None,
    }
}

fn apply_move(state: &SearchState, mv: &GenMove) -> SearchState {
    let mut next = state.clone();
    if let Some((fr, fc)) = mv.from {
        let from_idx = fr * 9 + fc;
        let to_idx = mv.to.0 * 9 + mv.to.1;
        let mut piece = next.board[from_idx].expect("piece must exist");
        next.board[from_idx] = None;
        if let Some(cap) = next.board[to_idx] {
            if cap.kind != PieceKind::King {
                if let Some(hidx) = hand_index(cap.kind) {
                    next.hands[side_index(piece.side)][hidx] =
                        next.hands[side_index(piece.side)][hidx].saturating_add(1);
                }
            }
        }
        if mv.promote {
            piece.promoted = true;
        }
        next.board[to_idx] = Some(piece);
    } else if let Some(kind) = mv.drop {
        let to_idx = mv.to.0 * 9 + mv.to.1;
        next.board[to_idx] = Some(Piece {
            side: state.side_to_move,
            kind,
            promoted: false,
        });
        if let Some(hidx) = hand_index(kind) {
            next.hands[side_index(state.side_to_move)][hidx] =
                next.hands[side_index(state.side_to_move)][hidx].saturating_sub(1);
        }
    }
    next.side_to_move = state.side_to_move.opposite();
    next
}

fn to_move_input(mv: &GenMove) -> MoveInput {
    MoveInput {
        from_row: mv.from.map(|(r, _)| r as i32),
        from_col: mv.from.map(|(_, c)| c as i32),
        to_row: mv.to.0 as i32,
        to_col: mv.to.1 as i32,
        piece_code: piece_code(mv.piece.kind).to_string(),
        promote: mv.promote,
        drop_piece_code: mv.drop.map(|k| piece_code(k).to_string()),
        captured_piece_code: mv.capture.map(|p| piece_code(p.kind).to_string()),
        notation: None,
    }
}

fn push_promote_variants(out: &mut Vec<GenMove>, base: GenMove) {
    let mut pushed = false;
    if base.from.is_some() && piece_promotable(base.piece.kind) && !base.piece.promoted {
        if let Some((fr, _)) = base.from {
            if is_promotion_zone(base.piece.side, fr) || is_promotion_zone(base.piece.side, base.to.0) {
                let mut m = base.clone();
                m.promote = true;
                out.push(m);
                pushed = true;
                if must_promote(base.piece, base.to.0) {
                    return;
                }
            }
        }
    }
    if !pushed || !must_promote(base.piece, base.to.0) {
        let mut m = base;
        m.promote = false;
        out.push(m);
    }
}

fn parse_sfen_hands(hands: &str) -> Result<[[u8; 7]; 2], String> {
    let mut out = [[0u8; 7]; 2];
    if hands == "-" {
        return Ok(out);
    }
    let mut cnt = 0u32;
    for ch in hands.chars() {
        if ch.is_ascii_digit() {
            cnt = cnt * 10 + ch.to_digit(10).ok_or("invalid hand digit")?;
            continue;
        }
        let n = if cnt == 0 { 1 } else { cnt };
        cnt = 0;
        let side = if ch.is_ascii_uppercase() {
            Side::Black
        } else {
            Side::White
        };
        let kind = piece_kind_from_char(ch).ok_or("invalid hand piece")?;
        if kind == PieceKind::King {
            return Err("king cannot be in hand".to_string());
        }
        let idx = hand_index(kind).ok_or("invalid hand kind")?;
        out[side_index(side)][idx] = out[side_index(side)][idx].saturating_add(n as u8);
    }
    Ok(out)
}

fn gen_drop_moves(state: &SearchState, out: &mut Vec<GenMove>) {
    let h = &state.hands[side_index(state.side_to_move)];
    for kind in [
        PieceKind::Pawn,
        PieceKind::Lance,
        PieceKind::Knight,
        PieceKind::Silver,
        PieceKind::Gold,
        PieceKind::Bishop,
        PieceKind::Rook,
    ] {
        let Some(hidx) = hand_index(kind) else { continue };
        if h[hidx] == 0 {
            continue;
        }
        for row in 0..9 {
            for col in 0..9 {
                let idx = row * 9 + col;
                if state.board[idx].is_some() {
                    continue;
                }
                if !drop_allowed(state, kind, row, col) {
                    continue;
                }
                out.push(GenMove {
                    from: None,
                    to: (row, col),
                    piece: Piece {
                        side: state.side_to_move,
                        kind,
                        promoted: false,
                    },
                    promote: false,
                    capture: None,
                    drop: Some(kind),
                });
            }
        }
    }
}

fn drop_allowed(state: &SearchState, kind: PieceKind, row: usize, col: usize) -> bool {
    match (state.side_to_move, kind) {
        (Side::Black, PieceKind::Pawn | PieceKind::Lance) if row == 0 => return false,
        (Side::White, PieceKind::Pawn | PieceKind::Lance) if row == 8 => return false,
        (Side::Black, PieceKind::Knight) if row <= 1 => return false,
        (Side::White, PieceKind::Knight) if row >= 7 => return false,
        _ => {}
    }
    if kind == PieceKind::Pawn && has_pawn_on_file(state, state.side_to_move, col) {
        return false;
    }
    true
}

fn has_pawn_on_file(state: &SearchState, side: Side, col: usize) -> bool {
    for row in 0..9 {
        if let Some(p) = state.board[row * 9 + col] {
            if p.side == side && p.kind == PieceKind::Pawn && !p.promoted {
                return true;
            }
        }
    }
    false
}

fn is_in_check(state: &SearchState, side: Side, rules: &RuntimeRules) -> bool {
    let king_pos = state
        .board
        .iter()
        .enumerate()
        .find_map(|(idx, p)| match p {
            Some(pc) if pc.side == side && pc.kind == PieceKind::King => Some((idx / 9, idx % 9)),
            _ => None,
        });
    let Some((kr, kc)) = king_pos else {
        return false;
    };
    attacks_square(state, side.opposite(), kr, kc, rules)
}

fn attacks_square(state: &SearchState, attacker: Side, tr: usize, tc: usize, rules: &RuntimeRules) -> bool {
    for row in 0..9 {
        for col in 0..9 {
            let Some(piece) = state.board[row * 9 + col] else { continue };
            if piece.side != attacker {
                continue;
            }
            let mut list = Vec::new();
            gen_piece_moves(state, row, col, piece, rules, &mut list);
            if list.into_iter().any(|m| m.to == (tr, tc)) {
                return true;
            }
        }
    }
    false
}

fn is_uchi_fuzume(state: &SearchState, mv: &GenMove, rules: &RuntimeRules) -> bool {
    if mv.drop != Some(PieceKind::Pawn) {
        return false;
    }
    let next = apply_move(state, mv);
    if !is_in_check(&next, next.side_to_move, rules) {
        return false;
    }
    let replies = generate_legal_moves(&next, rules, false);
    replies.is_empty()
}

fn side_index(side: Side) -> usize {
    match side {
        Side::Black => 0,
        Side::White => 1,
    }
}

fn hand_index(kind: PieceKind) -> Option<usize> {
    match kind {
        PieceKind::Pawn => Some(0),
        PieceKind::Lance => Some(1),
        PieceKind::Knight => Some(2),
        PieceKind::Silver => Some(3),
        PieceKind::Gold => Some(4),
        PieceKind::Bishop => Some(5),
        PieceKind::Rook => Some(6),
        PieceKind::King => None,
    }
}

fn parse_runtime_rules(board_state: &serde_json::Value) -> RuntimeRules {
    let mut rules = RuntimeRules::default();

    if let Some(m) = board_state.get("eval_bonus_by_piece").and_then(|v| v.as_object()) {
        for (k, v) in m {
            if let Some(cp) = v.as_i64() {
                rules.eval_bonus_by_piece.insert(k.to_ascii_uppercase(), cp as i32);
            }
        }
    }

    if let Some(m) = board_state
        .get("custom_move_vectors")
        .and_then(|v| v.as_object())
    {
        for (piece, arr) in m {
            let mut vecs = Vec::new();
            if let Some(items) = arr.as_array() {
                for item in items {
                    let dr = item.get("dr").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
                    let dc = item.get("dc").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
                    let slide = item
                        .get("slide")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false);
                    if dr != 0 || dc != 0 {
                        vecs.push(VectorRule { dr, dc, slide });
                    }
                }
            }
            if !vecs.is_empty() {
                rules
                    .extra_vectors_by_piece
                    .insert(piece.to_ascii_uppercase(), vecs);
            }
        }
    }

    rules
}

fn piece_kind_from_char(ch: char) -> Option<PieceKind> {
    match ch.to_ascii_uppercase() {
        'P' => Some(PieceKind::Pawn),
        'L' => Some(PieceKind::Lance),
        'N' => Some(PieceKind::Knight),
        'S' => Some(PieceKind::Silver),
        'G' => Some(PieceKind::Gold),
        'B' => Some(PieceKind::Bishop),
        'R' => Some(PieceKind::Rook),
        'K' => Some(PieceKind::King),
        _ => None,
    }
}

fn piece_code(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::Pawn => "FU",
        PieceKind::Lance => "KY",
        PieceKind::Knight => "KE",
        PieceKind::Silver => "GI",
        PieceKind::Gold => "KI",
        PieceKind::Bishop => "KA",
        PieceKind::Rook => "HI",
        PieceKind::King => "OU",
    }
}

fn piece_base_value(kind: PieceKind) -> i32 {
    match kind {
        PieceKind::Pawn => 100,
        PieceKind::Lance => 300,
        PieceKind::Knight => 320,
        PieceKind::Silver => 500,
        PieceKind::Gold => 600,
        PieceKind::Bishop => 900,
        PieceKind::Rook => 1000,
        PieceKind::King => 10000,
    }
}

fn piece_promotable(kind: PieceKind) -> bool {
    !matches!(kind, PieceKind::Gold | PieceKind::King)
}

fn is_promotion_zone(side: Side, row: usize) -> bool {
    match side {
        Side::Black => row <= 2,
        Side::White => row >= 6,
    }
}

fn must_promote(piece: Piece, to_row: usize) -> bool {
    match (piece.side, piece.kind) {
        (Side::Black, PieceKind::Pawn | PieceKind::Lance) => to_row == 0,
        (Side::White, PieceKind::Pawn | PieceKind::Lance) => to_row == 8,
        (Side::Black, PieceKind::Knight) => to_row <= 1,
        (Side::White, PieceKind::Knight) => to_row >= 7,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_move() -> MoveInput {
        MoveInput {
            from_row: Some(6),
            from_col: Some(4),
            to_row: 5,
            to_col: 4,
            piece_code: "FU".to_string(),
            promote: false,
            drop_piece_code: None,
            captured_piece_code: None,
            notation: Some("7f7e".to_string()),
        }
    }

    #[test]
    fn engine_config_defaults_are_applied() {
        let cfg = build_engine_config(EngineConfigInput::default()).expect("default config should be valid");
        assert_eq!(cfg.max_depth, 3);
        assert_eq!(cfg.max_nodes, 20_000);
        assert_eq!(cfg.time_limit_ms, 300);
        assert!(cfg.always_legal_move);
        assert!(cfg.mate_avoidance);
    }

    #[test]
    fn engine_config_rejects_invalid_values() {
        let cfg = EngineConfigInput {
            max_depth: Some(0),
            ..EngineConfigInput::default()
        };
        assert!(build_engine_config(cfg).is_err());
    }

    #[test]
    fn engine_config_rejects_false_safety_flags() {
        let cfg = EngineConfigInput {
            always_legal_move: Some(false),
            ..EngineConfigInput::default()
        };
        assert!(build_engine_config(cfg).is_err());
    }

    #[test]
    fn coordinate_validation_accepts_normal_move() {
        assert!(is_board_coordinate_valid(&sample_move()));
    }

    #[test]
    fn coordinate_validation_rejects_inconsistent_drop() {
        let mut mv = sample_move();
        mv.drop_piece_code = Some("FU".to_string());
        assert!(!is_board_coordinate_valid(&mv));
    }

    #[test]
    fn select_move_is_deterministic_when_no_randomness() {
        let cfg = EngineConfig {
            random_topk: 3,
            blunder_rate: 0.0,
            temperature: 0.0,
            ..EngineConfig::default()
        };
        let scored = vec![(2, 100), (1, 90), (0, 80)];
        let mut rng = StdRng::seed_from_u64(42);
        let idx = select_move_index(&scored, 100, &cfg, &mut rng);
        assert_eq!(idx, 2);
    }

    #[test]
    fn seed_is_stable_for_same_input() {
        let a = make_seed("game-1", 12);
        let b = make_seed("game-1", 12);
        assert_eq!(a, b);
    }

    #[test]
    fn parse_sfen_basic_position() {
        let sfen = "4k4/9/9/9/9/9/9/9/4K4 b - 1";
        let state = SearchState::from_sfen(sfen).expect("must parse");
        let kings = state
            .board
            .iter()
            .filter(|p| p.map(|x| x.kind == PieceKind::King).unwrap_or(false))
            .count();
        assert_eq!(kings, 2);
    }

    #[test]
    fn generate_moves_from_sfen_produces_candidates() {
        let sfen = "4k4/9/9/9/9/9/9/4P4/4K4 b - 1";
        let state = SearchState::from_sfen(sfen).expect("must parse");
        let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);
        assert!(!moves.is_empty());
    }

    #[test]
    fn nifu_drop_is_filtered() {
        let sfen = "4k4/9/9/9/9/9/4P4/9/4K4 b P 1";
        let state = SearchState::from_sfen(sfen).expect("must parse");
        let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);
        let has_same_file_pawn_drop = moves.iter().any(|m| m.drop == Some(PieceKind::Pawn) && m.to.1 == 4);
        assert!(!has_same_file_pawn_drop);
    }

    #[test]
    fn legal_moves_filter_out_self_check() {
        let sfen = "4k4/9/9/9/9/9/9/4r4/4K4 b - 1";
        let state = SearchState::from_sfen(sfen).expect("must parse");
        let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);
        assert!(moves.iter().all(|m| {
            let next = apply_move(&state, m);
            !is_in_check(&next, Side::Black, &RuntimeRules::default())
        }));
    }

    #[test]
    fn custom_vectors_are_loaded() {
        let v: serde_json::Value = serde_json::json!({
            "custom_move_vectors": {
                "FU": [
                    { "dr": -1, "dc": -1, "slide": false }
                ]
            },
            "eval_bonus_by_piece": {
                "FU": 10
            }
        });
        let rules = parse_runtime_rules(&v);
        assert_eq!(rules.extra_vectors_by_piece.get("FU").map(|x| x.len()), Some(1));
        assert_eq!(rules.eval_bonus_by_piece.get("FU").copied(), Some(10));
    }
}
