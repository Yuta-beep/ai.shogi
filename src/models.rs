use crate::engine::config::EngineConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct EngineMoveRequest {
    pub game_id: String,
    pub move_no: u32,
    pub position: PositionInput,
    #[serde(default)]
    pub engine_config: EngineConfigInput,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PositionInput {
    pub side_to_move: String,
    pub turn_number: u32,
    pub move_count: u32,
    #[serde(default)]
    pub sfen: Option<String>,
    #[serde(default)]
    pub state_hash: Option<String>,
    #[serde(default)]
    pub board_state: serde_json::Value,
    #[serde(default)]
    pub hands: serde_json::Value,
    pub legal_moves: Vec<MoveInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveInput {
    pub from_row: Option<i32>,
    pub from_col: Option<i32>,
    pub to_row: i32,
    pub to_col: i32,
    pub piece_code: String,
    #[serde(default)]
    pub promote: bool,
    #[serde(default)]
    pub drop_piece_code: Option<String>,
    #[serde(default)]
    pub captured_piece_code: Option<String>,
    #[serde(default)]
    pub notation: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct EngineConfigInput {
    pub max_depth: Option<u32>,
    pub max_nodes: Option<u32>,
    pub time_limit_ms: Option<u32>,
    pub quiescence_enabled: Option<bool>,
    pub eval_material_weight: Option<f64>,
    pub eval_position_weight: Option<f64>,
    pub eval_king_safety_weight: Option<f64>,
    pub eval_mobility_weight: Option<f64>,
    pub blunder_rate: Option<f64>,
    pub blunder_max_loss_cp: Option<u32>,
    pub random_topk: Option<u32>,
    pub temperature: Option<f64>,
    pub always_legal_move: Option<bool>,
    pub mate_avoidance: Option<bool>,
    pub max_repeat_draw_bias: Option<f64>,
    pub random_seed: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct EngineMoveResponse {
    pub selected_move: MoveInput,
    pub meta: EngineMeta,
}

#[derive(Debug, Serialize)]
pub struct EngineMeta {
    pub engine_version: &'static str,
    pub think_ms: u64,
    pub searched_nodes: u64,
    pub search_depth: u32,
    pub eval_cp: i32,
    pub candidate_count: usize,
    pub config_applied: EngineConfig,
}
