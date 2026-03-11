# shogi-ai

Rust ベースの将棋 AI エンジンです。BFF から HTTP で呼ばれる手選択 API を提供します。

## アーキテクチャ

```
API層        リクエスト受信・DTO変換・エラー整形
Application層 手選択フロー制御 (compute_ai_move)
Engine層     設定検証・合法手生成・探索・評価・最終選択
```

探索アルゴリズム: 反復深化 + negamax (alpha-beta)

## Run

```bash
cargo run
```

デフォルト待受: `0.0.0.0:8080`

環境変数:

| 変数名 | 例 | 説明 |
|---|---|---|
| `AI_ENGINE_BIND` | `127.0.0.1:8080` | 待受アドレス |
| `RUST_LOG` | `debug` | ログレベル |

## API

### `GET /health`

ヘルスチェック。`200 OK` を返します。

### `POST /v1/ai/move`

AI の手を1手返します。

**リクエスト例:**

```json
{
  "game_id": "uuid-string",
  "move_no": 12,
  "position": {
    "side_to_move": "enemy",
    "turn_number": 6,
    "move_count": 11,
    "sfen": "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
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

**レスポンス例:**

```json
{
  "selected_move": { "from_row": 6, "from_col": 4, "to_row": 5, "to_col": 4, "piece_code": "FU", "promote": false },
  "meta": {
    "engine_version": "0.1.0",
    "think_ms": 12,
    "searched_nodes": 4200,
    "search_depth": 3,
    "eval_cp": 50,
    "candidate_count": 30,
    "config_applied": { "max_depth": 3, "max_nodes": 20000, "time_limit_ms": 300, "..." : "..." }
  }
}
```

**エラー:**

| ステータス | コード | 説明 |
|---|---|---|
| 400 | `INVALID_ENGINE_CONFIG` | エンジン設定値が範囲外 |
| 400 | `INVALID_POSITION` | 合法手なし / SFEN 不正 |
| 500 | `ENGINE_INTERNAL` | 内部エラー |

## engine_config パラメータ

| パラメータ | デフォルト | 範囲 | 説明 |
|---|---|---|---|
| `max_depth` | 3 | 1–12 | 最大探索深さ |
| `max_nodes` | 20000 | 100–5000000 | ノード数上限 |
| `time_limit_ms` | 300 | 10–60000 | 思考時間上限 (ms) |
| `quiescence_enabled` | true | — | 静止探索 (現時点で未接続) |
| `eval_material_weight` | 1.0 | 0–10 | 駒得評価の重み |
| `eval_position_weight` | 0.35 | 0–10 | 位置評価の重み |
| `eval_king_safety_weight` | 0.25 | 0–10 | 王安全評価の重み |
| `eval_mobility_weight` | 0.2 | 0–10 | 機動力評価の重み |
| `blunder_rate` | 0.0 | 0–1 | ミス確率 (難易度調整用) |
| `blunder_max_loss_cp` | 0 | 0–3000 | ミス時の最大損失 (cp) |
| `random_topk` | 1 | 1–20 | 上位 k 手からランダム選択 |
| `temperature` | 0.0 | 0–2 | ソフトマックス温度 |
| `always_legal_move` | true | — | 常に合法手を返す (変更不可) |
| `mate_avoidance` | true | — | 詰み回避 (変更不可) |
| `max_repeat_draw_bias` | 0.0 | -1–1 | 千日手回避バイアス (現時点で未使用) |
| `random_seed` | null | — | 乱数シード (再現性用) |

`sfen` を渡すと反復深化探索、省略時は 1 手ヒューリスティック評価にフォールバックします。

## ドキュメント

- `docs/phase1-contract.md` — API 契約詳細
- `docs/ai-algorithm-overview-ja.md` — 手選択アルゴリズム概要
- `docs/ai-engine-design-policy.md` — 設計方針
