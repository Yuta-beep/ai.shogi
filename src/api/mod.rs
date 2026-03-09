mod dto;
mod error;
mod handlers;

use axum::{routing::get, routing::post, Router};

pub fn router() -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/v1/ai/move", post(handlers::post_ai_move))
}

#[cfg(test)]
mod tests;
