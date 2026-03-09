use crate::api::dto::{
    EngineConfigOutput, EngineMeta, EngineMoveRequest, EngineMoveResponse, MoveInput,
};
use crate::api::error::err;
use crate::application::ai_move::{compute_ai_move, ComputeMoveCommand};
use axum::{http::StatusCode, response::IntoResponse, Json};
use tracing::{info, warn};

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "ok": true }))
}

pub async fn post_ai_move(Json(payload): Json<EngineMoveRequest>) -> impl IntoResponse {
    info!(
        target: "ai_request",
        game_id = %payload.game_id,
        move_no = payload.move_no,
        side_to_move = %payload.position.side_to_move,
        turn_number = payload.position.turn_number,
        move_count = payload.position.move_count,
        has_legal_moves = !payload.position.legal_moves.is_empty(),
        has_sfen = payload.position.sfen.is_some(),
        has_state_hash = payload.position.state_hash.is_some(),
        has_hands = !payload.position.hands.is_null(),
        "received /v1/ai/move"
    );

    let cmd = ComputeMoveCommand {
        game_id: payload.game_id,
        move_no: payload.move_no,
        side_to_move: payload.position.side_to_move,
        sfen: payload.position.sfen,
        board_state: payload.position.board_state,
        legal_moves: payload
            .position
            .legal_moves
            .into_iter()
            .map(Into::into)
            .collect(),
        config_patch: payload.engine_config.into(),
    };

    let result = match compute_ai_move(cmd) {
        Ok(result) => result,
        Err(e) => {
            warn!(
                target: "ai_request",
                code = e.code(),
                message = %e.message(),
                "failed /v1/ai/move"
            );

            let status = if e.code() == "INVALID_ENGINE_CONFIG" {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::BAD_REQUEST
            };
            return err(status, e.code(), e.message());
        }
    };

    info!(
        target: "ai_request",
        selected_piece = %result.selected_move.piece_code,
        to_row = result.selected_move.to_row,
        to_col = result.selected_move.to_col,
        eval_cp = result.meta.eval_cp,
        depth = result.meta.search_depth,
        nodes = result.meta.searched_nodes,
        think_ms = result.meta.think_ms,
        "completed /v1/ai/move"
    );

    (
        StatusCode::OK,
        Json(EngineMoveResponse {
            selected_move: MoveInput::from(result.selected_move),
            meta: EngineMeta {
                engine_version: env!("CARGO_PKG_VERSION"),
                think_ms: result.meta.think_ms,
                searched_nodes: result.meta.searched_nodes,
                search_depth: result.meta.search_depth,
                eval_cp: result.meta.eval_cp,
                candidate_count: result.meta.candidate_count,
                config_applied: EngineConfigOutput::from(result.meta.config_applied),
            },
        }),
    )
        .into_response()
}
