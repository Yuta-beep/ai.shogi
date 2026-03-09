use super::router;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let app = router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed, json!({ "ok": true }));
}

#[tokio::test]
async fn ai_move_rejects_empty_position_without_sfen() {
    let app = router();

    let payload = json!({
        "game_id": "test-game",
        "move_no": 1,
        "position": {
            "side_to_move": "player",
            "turn_number": 1,
            "move_count": 0,
            "board_state": {},
            "hands": {},
            "legal_moves": []
        },
        "engine_config": {}
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ai/move")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["code"], "INVALID_POSITION");
}
