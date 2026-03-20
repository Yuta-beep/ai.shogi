use crate::api::dto::{CanonicalPositionOutput, PositionInput};
use crate::engine::rules::parse_runtime_rules;
use crate::engine::search::{apply_move, generate_legal_moves};
use crate::engine::skill_executor::simulate_move_with_skills;
use crate::engine::types::{piece_code, EngineMove, GenMove, RuntimeRules, SearchState, Side};

#[derive(Debug)]
pub struct ApplyMoveCommand {
    pub position: PositionInput,
    pub selected_move: EngineMove,
}

#[derive(Debug)]
pub struct ApplyMoveResult {
    pub position: CanonicalPositionOutput,
}

#[derive(Debug)]
pub enum ApplyMoveError {
    InvalidPosition(&'static str),
    IllegalMove,
}

impl ApplyMoveError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPosition(_) => "INVALID_POSITION",
            Self::IllegalMove => "ILLEGAL_MOVE",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidPosition(message) => (*message).to_string(),
            Self::IllegalMove => "selected move is not legal for current position".to_string(),
        }
    }
}

pub fn apply_canonical_move(input: ApplyMoveCommand) -> Result<ApplyMoveResult, ApplyMoveError> {
    let rules = parse_runtime_rules(&input.position.board_state)
        .map_err(|_| ApplyMoveError::InvalidPosition("invalid board_state runtime rules"))?;
    let sfen = input
        .position
        .sfen
        .as_deref()
        .ok_or(ApplyMoveError::InvalidPosition("sfen is required"))?;
    let mut state = SearchState::from_sfen(sfen)
        .map_err(|_| ApplyMoveError::InvalidPosition("invalid sfen"))?;
    state.side_to_move = Side::from_position_side(&input.position.side_to_move)
        .ok_or(ApplyMoveError::InvalidPosition("invalid side_to_move"))?;
    state.hydrate_skill_state_from_board_state(&input.position.board_state);

    let legal_moves = generate_legal_moves(&state, &rules, true);
    let Some(matched_move) = legal_moves
        .iter()
        .find(|candidate| generated_move_matches(candidate, &input.selected_move))
        .cloned()
    else {
        return Err(ApplyMoveError::IllegalMove);
    };

    let next_state = simulate_or_apply(&state, &matched_move, &input.selected_move, &rules);
    let next_turn_number = input.position.turn_number.saturating_add(1);
    let next_move_count = input.position.move_count.saturating_add(1);
    let next_sfen = next_state.to_sfen(next_turn_number);
    let next_hands = next_state.hands_to_json();
    let next_skill_state = next_state.skill_state_to_json();
    let next_board_state = merge_board_state(&input.position.board_state, next_skill_state);

    Ok(ApplyMoveResult {
        position: CanonicalPositionOutput {
            side_to_move: next_state.side_to_move.as_position_side().to_string(),
            turn_number: next_turn_number,
            move_count: next_move_count,
            sfen: Some(next_sfen),
            state_hash: None,
            board_state: next_board_state,
            hands: next_hands,
        },
    })
}

fn simulate_or_apply(
    state: &SearchState,
    matched_move: &GenMove,
    selected_move: &EngineMove,
    rules: &RuntimeRules,
) -> SearchState {
    simulate_move_with_skills(state, selected_move, rules)
        .map(|simulated| simulated.state)
        .unwrap_or_else(|| apply_move(state, matched_move))
}

fn merge_board_state(
    board_state: &serde_json::Value,
    skill_state: serde_json::Value,
) -> serde_json::Value {
    let mut next = board_state.as_object().cloned().unwrap_or_default();
    // SFEN が正規の盤面ソースなので、視覚用の配置キャッシュは局面更新後に捨てる。
    // これを残すとフロントで移動前の駒が重複表示される。
    next.remove("pieces");
    next.remove("placements");
    next.remove("boardPieces");
    next.remove("board_pieces");
    next.insert("skill_state".to_string(), skill_state);
    serde_json::Value::Object(next)
}

#[cfg(test)]
mod tests {
    use super::{apply_canonical_move, merge_board_state, ApplyMoveCommand};
    use crate::api::dto::PositionInput;
    use crate::engine::types::EngineMove;
    use serde_json::json;

    #[test]
    fn merge_board_state_drops_visual_piece_caches() {
        let board_state = json!({
            "pieces": [{ "row": 6, "col": 4 }],
            "placements": [{ "row": 6, "col": 4 }],
            "boardPieces": [{ "row": 6, "col": 4 }],
            "board_pieces": [{ "row": 6, "col": 4 }],
            "custom_move_vectors": { "FU": [{ "dr": -1, "dc": 0 }] },
            "skill_state": { "before": true }
        });

        let merged = merge_board_state(&board_state, json!({ "after": true }));

        assert!(merged.get("pieces").is_none());
        assert!(merged.get("placements").is_none());
        assert!(merged.get("boardPieces").is_none());
        assert!(merged.get("board_pieces").is_none());
        assert_eq!(merged.get("custom_move_vectors"), Some(&json!({ "FU": [{ "dr": -1, "dc": 0 }] })));
        assert_eq!(merged.get("skill_state"), Some(&json!({ "after": true })));
    }

    #[test]
    fn apply_canonical_move_returns_sfen_position_without_visual_piece_caches() {
        let result = apply_canonical_move(ApplyMoveCommand {
            position: PositionInput {
                side_to_move: "player".to_string(),
                turn_number: 1,
                move_count: 0,
                sfen: Some("4k4/9/9/9/9/9/4P4/9/4K4 b - 1".to_string()),
                state_hash: None,
                board_state: json!({
                    "pieces": [{ "row": 6, "col": 4, "pieceCode": "FU" }],
                    "placements": [{ "row": 6, "col": 4, "pieceCode": "FU" }],
                    "boardPieces": [{ "row": 6, "col": 4, "pieceCode": "FU" }],
                    "board_pieces": [{ "row": 6, "col": 4, "pieceCode": "FU" }],
                    "custom_move_vectors": { "FU": [{ "dr": -1, "dc": 0 }] }
                }),
                hands: json!({ "player": {}, "enemy": {} }),
                legal_moves: vec![],
            },
            selected_move: EngineMove {
                from_row: Some(6),
                from_col: Some(4),
                to_row: 5,
                to_col: 4,
                piece_code: "FU".to_string(),
                promote: false,
                drop_piece_code: None,
                captured_piece_code: None,
                notation: None,
            },
        })
        .expect("move must apply");

        assert_eq!(result.position.side_to_move, "enemy");
        assert_eq!(result.position.turn_number, 2);
        assert_eq!(result.position.move_count, 1);
        assert_eq!(result.position.sfen, Some("4k4/9/9/9/9/4P4/9/9/4K4 w - 2".to_string()));
        assert!(result.position.board_state.get("pieces").is_none());
        assert!(result.position.board_state.get("placements").is_none());
        assert!(result.position.board_state.get("boardPieces").is_none());
        assert!(result.position.board_state.get("board_pieces").is_none());
        assert_eq!(
            result.position.board_state.get("custom_move_vectors"),
            Some(&json!({ "FU": [{ "dr": -1, "dc": 0 }] }))
        );
        assert_eq!(result.position.hands, json!({ "player": {}, "enemy": {} }));
    }
}

fn generated_move_matches(candidate: &GenMove, selected_move: &EngineMove) -> bool {
    let from_matches = match (
        candidate.from,
        selected_move.from_row,
        selected_move.from_col,
    ) {
        (Some((row, col)), Some(from_row), Some(from_col)) => {
            row == from_row as usize && col == from_col as usize
        }
        (None, None, None) => true,
        _ => false,
    };
    if !from_matches {
        return false;
    }

    candidate.to.0 == selected_move.to_row as usize
        && candidate.to.1 == selected_move.to_col as usize
        && candidate.promote == selected_move.promote
        && piece_code(&candidate.piece.kind).eq_ignore_ascii_case(&selected_move.piece_code)
        && candidate
            .drop
            .as_ref()
            .map(|kind| piece_code(kind))
            .map(|code| {
                code.eq_ignore_ascii_case(selected_move.drop_piece_code.as_deref().unwrap_or(""))
            })
            .unwrap_or(selected_move.drop_piece_code.is_none())
}
