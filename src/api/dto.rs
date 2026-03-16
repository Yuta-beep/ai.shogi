use crate::engine::{EngineConfig, EngineConfigPatch, EngineMove};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct EngineMoveRequest {
    pub game_id: String,
    pub move_no: u32,
    pub position: PositionInput,
    #[serde(default)]
    pub engine_config: EngineConfigInput,
}

#[derive(Debug, Deserialize)]
pub struct EngineApplyMoveRequest {
    pub position: PositionInput,
    pub selected_move: MoveInput,
}

#[derive(Debug, Deserialize)]
pub struct EngineLegalMovesRequest {
    pub position: PositionInput,
}

#[derive(Debug, Deserialize)]
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
pub struct EngineApplyMoveResponse {
    pub position: CanonicalPositionOutput,
}

#[derive(Debug, Serialize)]
pub struct EngineLegalMovesResponse {
    pub legal_moves: Vec<MoveInput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CanonicalPositionOutput {
    pub side_to_move: String,
    pub turn_number: u32,
    pub move_count: u32,
    pub sfen: Option<String>,
    pub state_hash: Option<String>,
    pub board_state: serde_json::Value,
    pub hands: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct EngineMeta {
    pub engine_version: &'static str,
    pub think_ms: u64,
    pub searched_nodes: u64,
    pub search_depth: u32,
    pub eval_cp: i32,
    pub candidate_count: usize,
    pub config_applied: EngineConfigOutput,
}

#[derive(Debug, Serialize)]
pub struct EngineConfigOutput {
    pub max_depth: u32,
    pub max_nodes: u32,
    pub time_limit_ms: u32,
    pub quiescence_enabled: bool,
    pub eval_material_weight: f64,
    pub eval_position_weight: f64,
    pub eval_king_safety_weight: f64,
    pub eval_mobility_weight: f64,
    pub blunder_rate: f64,
    pub blunder_max_loss_cp: u32,
    pub random_topk: u32,
    pub temperature: f64,
    pub always_legal_move: bool,
    pub mate_avoidance: bool,
    pub max_repeat_draw_bias: f64,
    pub random_seed: Option<u64>,
}

impl From<MoveInput> for EngineMove {
    fn from(value: MoveInput) -> Self {
        Self {
            from_row: value.from_row,
            from_col: value.from_col,
            to_row: value.to_row,
            to_col: value.to_col,
            piece_code: value.piece_code,
            promote: value.promote,
            drop_piece_code: value.drop_piece_code,
            captured_piece_code: value.captured_piece_code,
            notation: value.notation,
        }
    }
}

impl From<EngineMove> for MoveInput {
    fn from(value: EngineMove) -> Self {
        Self {
            from_row: value.from_row,
            from_col: value.from_col,
            to_row: value.to_row,
            to_col: value.to_col,
            piece_code: value.piece_code,
            promote: value.promote,
            drop_piece_code: value.drop_piece_code,
            captured_piece_code: value.captured_piece_code,
            notation: value.notation,
        }
    }
}

impl From<EngineConfigInput> for EngineConfigPatch {
    fn from(value: EngineConfigInput) -> Self {
        Self {
            max_depth: value.max_depth,
            max_nodes: value.max_nodes,
            time_limit_ms: value.time_limit_ms,
            quiescence_enabled: value.quiescence_enabled,
            eval_material_weight: value.eval_material_weight,
            eval_position_weight: value.eval_position_weight,
            eval_king_safety_weight: value.eval_king_safety_weight,
            eval_mobility_weight: value.eval_mobility_weight,
            blunder_rate: value.blunder_rate,
            blunder_max_loss_cp: value.blunder_max_loss_cp,
            random_topk: value.random_topk,
            temperature: value.temperature,
            always_legal_move: value.always_legal_move,
            mate_avoidance: value.mate_avoidance,
            max_repeat_draw_bias: value.max_repeat_draw_bias,
            random_seed: value.random_seed,
        }
    }
}

impl From<EngineConfig> for EngineConfigOutput {
    fn from(value: EngineConfig) -> Self {
        Self {
            max_depth: value.max_depth,
            max_nodes: value.max_nodes,
            time_limit_ms: value.time_limit_ms,
            quiescence_enabled: value.quiescence_enabled,
            eval_material_weight: value.eval_material_weight,
            eval_position_weight: value.eval_position_weight,
            eval_king_safety_weight: value.eval_king_safety_weight,
            eval_mobility_weight: value.eval_mobility_weight,
            blunder_rate: value.blunder_rate,
            blunder_max_loss_cp: value.blunder_max_loss_cp,
            random_topk: value.random_topk,
            temperature: value.temperature,
            always_legal_move: value.always_legal_move,
            mate_avoidance: value.mate_avoidance,
            max_repeat_draw_bias: value.max_repeat_draw_bias,
            random_seed: value.random_seed,
        }
    }
}
