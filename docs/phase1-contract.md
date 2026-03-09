# Phase1 Contract (BFF <-> Rust Engine)

## Endpoint

- Method: `POST`
- Path: `/v1/ai/move`
- Content-Type: `application/json`

## Request

```json
{
  "game_id": "uuid-string",
  "move_no": 12,
  "position": {
    "side_to_move": "enemy",
    "turn_number": 6,
    "move_count": 11,
    "board_state": {},
    "hands": {},
    "legal_moves": [
      {
        "from_row": 6,
        "from_col": 4,
        "to_row": 5,
        "to_col": 4,
        "piece_code": "FU",
        "promote": false,
        "drop_piece_code": null,
        "captured_piece_code": null,
        "notation": "7f7e"
      }
    ]
  },
  "engine_config": {
    "max_depth": 3,
    "max_nodes": 20000,
    "time_limit_ms": 300
  }
}
```

## Response

```json
{
  "selected_move": {
    "from_row": 6,
    "from_col": 4,
    "to_row": 5,
    "to_col": 4,
    "piece_code": "FU",
    "promote": false,
    "drop_piece_code": null,
    "captured_piece_code": null,
    "notation": "7f7e"
  },
  "meta": {
    "engine_version": "0.1.0",
    "think_ms": 3,
    "searched_nodes": 1,
    "search_depth": 1,
    "eval_cp": 0,
    "candidate_count": 1,
    "config_applied": {}
  }
}
```

## Error

- `400 INVALID_ENGINE_CONFIG`
- `400 INVALID_POSITION`
- `500 ENGINE_INTERNAL`
