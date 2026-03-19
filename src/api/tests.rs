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

#[tokio::test]
async fn ai_move_returns_checkmate_when_no_legal_moves_in_sfen() {
    let app = router();

    // Enemy king at (0,0), player golds at (0,1) and (1,0) and (1,1).
    // All king escape squares are occupied or covered — genuine checkmate.
    // SFEN: kG7/GG7/9/9/9/9/9/9/8K  w (enemy to move)
    let payload = json!({
        "game_id": "test-checkmate",
        "move_no": 1,
        "position": {
            "side_to_move": "enemy",
            "turn_number": 1,
            "move_count": 0,
            "sfen": "kG7/GG7/9/9/9/9/9/9/8K w - 1",
            "board_state": {},
            "hands": { "player": {}, "enemy": {} },
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

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["is_checkmate"], true);
}

#[tokio::test]
async fn apply_move_returns_canonical_next_position() {
    let app = router();

    let payload = json!({
        "position": {
            "side_to_move": "player",
            "turn_number": 1,
            "move_count": 0,
            "sfen": "4k4/9/9/9/4P4/9/9/9/4K4 b - 1",
            "state_hash": null,
            "board_state": {},
            "hands": { "player": {}, "enemy": {} },
            "legal_moves": []
        },
        "selected_move": {
            "from_row": 4,
            "from_col": 4,
            "to_row": 3,
            "to_col": 4,
            "piece_code": "FU",
            "promote": false,
            "drop_piece_code": null,
            "captured_piece_code": null,
            "notation": null
        }
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/positions/apply")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["position"]["side_to_move"], "enemy");
    assert_eq!(parsed["position"]["turn_number"], 2);
    assert_eq!(parsed["position"]["move_count"], 1);
    assert_eq!(
        parsed["position"]["sfen"],
        json!("4k4/9/9/4P4/9/9/9/9/4K4 w - 2")
    );
    assert_eq!(
        parsed["position"]["board_state"]["skill_state"],
        json!({
            "piece_statuses": [],
            "board_hazards": [],
            "movement_modifiers": [],
            "piece_defenses": [],
            "turn_start_rules": []
        })
    );
}

#[tokio::test]
async fn legal_moves_returns_skill_aware_moves_from_hydrated_state() {
    let app = router();

    let payload = json!({
        "position": {
            "side_to_move": "player",
            "turn_number": 1,
            "move_count": 0,
            "sfen": "4k4/9/9/9/4B4/9/9/9/4K4 b - 1",
            "state_hash": null,
            "board_state": {
                "skill_state": {
                    "movement_modifiers": [
                        {
                            "row": 4,
                            "col": 4,
                            "side": "player",
                            "movement_rule": "orthogonal_step_only",
                            "remaining_turns": 2
                        }
                    ]
                }
            },
            "hands": { "player": {}, "enemy": {} },
            "legal_moves": []
        }
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/positions/legal-moves")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let moves = parsed["legal_moves"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let bishop_moves = moves
        .into_iter()
        .filter(|mv| mv["piece_code"] == "KA")
        .collect::<Vec<_>>();
    assert_eq!(
        bishop_moves,
        vec![
            json!({
                "from_row": 4,
                "from_col": 4,
                "to_row": 3,
                "to_col": 4,
                "piece_code": "KA",
                "promote": false,
                "drop_piece_code": null,
                "captured_piece_code": null,
                "notation": null
            }),
            json!({
                "from_row": 4,
                "from_col": 4,
                "to_row": 5,
                "to_col": 4,
                "piece_code": "KA",
                "promote": false,
                "drop_piece_code": null,
                "captured_piece_code": null,
                "notation": null
            }),
            json!({
                "from_row": 4,
                "from_col": 4,
                "to_row": 4,
                "to_col": 3,
                "piece_code": "KA",
                "promote": false,
                "drop_piece_code": null,
                "captured_piece_code": null,
                "notation": null
            }),
            json!({
                "from_row": 4,
                "from_col": 4,
                "to_row": 4,
                "to_col": 5,
                "piece_code": "KA",
                "promote": false,
                "drop_piece_code": null,
                "captured_piece_code": null,
                "notation": null
            }),
        ]
    );
}
