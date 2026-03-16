pub(crate) mod dto;
mod error;
mod handlers;

use axum::{routing::get, routing::post, Router};

pub fn router() -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/v1/ai/move", post(handlers::post_ai_move))
        .route(
            "/v1/positions/legal-moves",
            post(handlers::post_legal_moves),
        )
        .route("/v1/positions/apply", post(handlers::post_apply_move))
}

#[cfg(test)]
mod tests;
