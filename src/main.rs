use axum::{http::StatusCode, response::IntoResponse, routing::get, routing::post, Json, Router};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::time::Instant;
use thiserror::Error;
use tracing::info;

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
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
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
    if payload.position.legal_moves.is_empty() {
        return err(StatusCode::BAD_REQUEST, "INVALID_POSITION", "legal_moves must not be empty");
    }

    let cfg = match build_engine_config(payload.engine_config) {
        Ok(cfg) => cfg,
        Err(e) => return err(StatusCode::BAD_REQUEST, "INVALID_ENGINE_CONFIG", e.to_string()),
    };

    let start = Instant::now();
    let position_ctx = payload.position.clone();

    let normalized_moves: Vec<MoveInput> = payload
        .position
        .legal_moves
        .into_iter()
        .filter(is_board_coordinate_valid)
        .collect();

    if normalized_moves.is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_POSITION",
            "no valid move after basic legality filter",
        );
    }

    let seed = cfg.random_seed.unwrap_or_else(|| make_seed(&payload.game_id, payload.move_no));
    let mut rng = StdRng::seed_from_u64(seed);

    let mut scored: Vec<(usize, i32)> = normalized_moves
        .iter()
        .enumerate()
        .map(|(idx, mv)| (idx, evaluate_move(mv, &position_ctx, &cfg)))
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    let best_score = scored[0].1;

    let selected_idx = select_move_index(&scored, best_score, &cfg, &mut rng);
    let selected_move = normalized_moves[selected_idx].clone();

    let think_ms = start.elapsed().as_millis() as u64;
    let searched_nodes = scored.len().min(cfg.max_nodes as usize) as u64;

    (
        StatusCode::OK,
        Json(EngineMoveResponse {
            selected_move,
            meta: EngineMeta {
                engine_version: env!("CARGO_PKG_VERSION"),
                think_ms,
                searched_nodes,
                search_depth: 1.min(cfg.max_depth),
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
}
