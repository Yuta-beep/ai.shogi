use crate::engine::config::EngineConfig;
use crate::engine::constants::{
    CENTER_DIST_MAX, HEURISTIC_CENTER_WEIGHT, HEURISTIC_KING_SAFETY_BASE_CP,
    HEURISTIC_MOBILITY_BASE_CP, HEURISTIC_PROMOTE_BONUS_CP,
};
use crate::engine::types::EngineMove;

pub fn evaluate_move(mv: &EngineMove, side_to_move: &str, cfg: &EngineConfig) -> i32 {
    let capture_value = mv
        .captured_piece_code
        .as_deref()
        .map(piece_capture_cp)
        .unwrap_or(0) as f64;

    let promote_bonus = if mv.promote { HEURISTIC_PROMOTE_BONUS_CP } else { 0.0 };

    let center_dist = ((mv.to_row - 4).abs() + (mv.to_col - 4).abs()) as f64;
    let center_bonus = CENTER_DIST_MAX - center_dist;

    let side_bias = if side_to_move == "enemy" { 1.0 } else { -1.0 };

    let positional = (promote_bonus + center_bonus * HEURISTIC_CENTER_WEIGHT) * cfg.eval_position_weight;
    let material = capture_value * cfg.eval_material_weight;
    let mobility = HEURISTIC_MOBILITY_BASE_CP * cfg.eval_mobility_weight;
    let king_safety = HEURISTIC_KING_SAFETY_BASE_CP * cfg.eval_king_safety_weight;

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

pub fn is_board_coordinate_valid(mv: &EngineMove) -> bool {
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
