use crate::api::dto::{MoveInput, PositionInput};
use crate::engine::rules::parse_runtime_rules;
use crate::engine::search::generate_legal_moves;
use crate::engine::types::{piece_code, GenMove, SearchState, Side};

#[derive(Debug)]
pub struct LegalMovesCommand {
    pub position: PositionInput,
}

#[derive(Debug)]
pub struct LegalMovesResult {
    pub legal_moves: Vec<MoveInput>,
}

#[derive(Debug)]
pub enum LegalMovesError {
    InvalidPosition(&'static str),
}

impl LegalMovesError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPosition(_) => "INVALID_POSITION",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidPosition(message) => (*message).to_string(),
        }
    }
}

pub fn generate_canonical_legal_moves(
    input: LegalMovesCommand,
) -> Result<LegalMovesResult, LegalMovesError> {
    let rules = parse_runtime_rules(&input.position.board_state)
        .map_err(|_| LegalMovesError::InvalidPosition("invalid board_state runtime rules"))?;
    let sfen = input
        .position
        .sfen
        .as_deref()
        .ok_or(LegalMovesError::InvalidPosition("sfen is required"))?;
    let mut state = SearchState::from_sfen(sfen)
        .map_err(|_| LegalMovesError::InvalidPosition("invalid sfen"))?;
    state.side_to_move = Side::from_position_side(&input.position.side_to_move)
        .ok_or(LegalMovesError::InvalidPosition("invalid side_to_move"))?;
    state.hydrate_skill_state_from_board_state(&input.position.board_state);

    Ok(LegalMovesResult {
        legal_moves: generate_legal_moves(&state, &rules, true)
            .iter()
            .map(to_move_input)
            .collect(),
    })
}

fn to_move_input(mv: &GenMove) -> MoveInput {
    MoveInput {
        from_row: mv.from.map(|(row, _)| row as i32),
        from_col: mv.from.map(|(_, col)| col as i32),
        to_row: mv.to.0 as i32,
        to_col: mv.to.1 as i32,
        piece_code: piece_code(&mv.piece.kind).to_string(),
        promote: mv.promote,
        drop_piece_code: mv.drop.as_ref().map(|kind| piece_code(kind).to_string()),
        captured_piece_code: mv.capture.map(|piece| piece_code(&piece.kind).to_string()),
        notation: None,
    }
}
