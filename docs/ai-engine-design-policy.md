# Shogi AI Engine 設計方針

最終更新: 2026-03-09

## 1. スコープ

- このリポジトリ (`shogi-ai`) は Rust で AI エンジン本体を実装する場所とする。
- `backend` は BFF として、HTTP 入出力、認証、DB 連携、エンジン呼び出しを担当する。
- AI エンジンは対局ロジックのうち「局面評価・候補手探索・最終手選択」に責務を限定する。

## 2. システム境界

- クライアント: `frontend` (HTTP リクエスト送信)
- API/BFF: `backend` (Next.js Route Handler)
- 推論実行: `shogi-ai` (Rust)
- データ永続化: Supabase (master/public + 今後 game 系を追加)

基本フロー:

1. フロントエンドが BFF API を呼ぶ
2. BFF が必要な局面/ユーザー情報を DB から取得する
3. BFF が Rust エンジンへ推論リクエストを送る
4. Rust エンジンが最善手と推論メタデータを返す
5. BFF が結果を保存してフロントへ返却する

## 3. エンジン設計原則

- エンジンはステートレスに保つ（局面はリクエストで受け取る）。
- 同一入力 + 同一パラメータで再現可能な出力を優先する。
- 乱択を使う場合は seed を受け取れる形にして追跡可能にする。
- 合法手生成と詰み回避は難易度に関係なく常に保証する。

## 4. 強さ調整方針

難易度プリセット (`easy/normal/hard`) は定義しない。  
強さは独立パラメータで直接制御する。

### 4.1 EngineConfig 定義（厳密）

`engine_config` は次の JSON オブジェクトとする。

```json
{
  "max_depth": 3,
  "max_nodes": 20000,
  "time_limit_ms": 300,
  "quiescence_enabled": true,
  "eval_material_weight": 1.0,
  "eval_position_weight": 0.35,
  "eval_king_safety_weight": 0.25,
  "eval_mobility_weight": 0.2,
  "blunder_rate": 0.0,
  "blunder_max_loss_cp": 0,
  "random_topk": 1,
  "temperature": 0.0,
  "always_legal_move": true,
  "mate_avoidance": true,
  "max_repeat_draw_bias": 0.0,
  "random_seed": null
}
```

### 4.2 フィールド仕様（必須/任意、範囲、デフォルト）

- `max_depth`
  - 型: integer
  - 必須: 任意
  - 範囲: `1..=12`
  - デフォルト: `3`
- `max_nodes`
  - 型: integer
  - 必須: 任意
  - 範囲: `100..=5_000_000`
  - デフォルト: `20_000`
- `time_limit_ms`
  - 型: integer
  - 必須: 任意
  - 範囲: `10..=60_000`
  - デフォルト: `300`
- `quiescence_enabled`
  - 型: boolean
  - 必須: 任意
  - デフォルト: `true`
- `eval_material_weight`
  - 型: number
  - 必須: 任意
  - 範囲: `0.0..=10.0`
  - デフォルト: `1.0`
- `eval_position_weight`
  - 型: number
  - 必須: 任意
  - 範囲: `0.0..=10.0`
  - デフォルト: `0.35`
- `eval_king_safety_weight`
  - 型: number
  - 必須: 任意
  - 範囲: `0.0..=10.0`
  - デフォルト: `0.25`
- `eval_mobility_weight`
  - 型: number
  - 必須: 任意
  - 範囲: `0.0..=10.0`
  - デフォルト: `0.2`
- `blunder_rate`
  - 型: number
  - 必須: 任意
  - 範囲: `0.0..=1.0`
  - デフォルト: `0.0`
- `blunder_max_loss_cp`
  - 型: integer
  - 必須: 任意
  - 範囲: `0..=3000`
  - デフォルト: `0`
- `random_topk`
  - 型: integer
  - 必須: 任意
  - 範囲: `1..=20`
  - デフォルト: `1`
- `temperature`
  - 型: number
  - 必須: 任意
  - 範囲: `0.0..=2.0`
  - デフォルト: `0.0`
- `always_legal_move`
  - 型: boolean
  - 必須: 任意
  - デフォルト: `true`（`false` は受け付けない）
- `mate_avoidance`
  - 型: boolean
  - 必須: 任意
  - デフォルト: `true`（`false` は受け付けない）
- `max_repeat_draw_bias`
  - 型: number
  - 必須: 任意
  - 範囲: `-1.0..=1.0`
  - デフォルト: `0.0`
- `random_seed`
  - 型: integer | null
  - 必須: 任意
  - 範囲: `0..=9_223_372_036_854_775_807`（i64）
  - デフォルト: `null`

補足:

- `always_legal_move` と `mate_avoidance` は安全要件として常時 true。API で false が渡された場合は 400 を返す。
- `blunder_rate = 0` または `random_topk = 1` かつ `temperature = 0` の場合、決定論的な最善手選択になる（同一 seed/同一入力前提）。
- 探索停止条件は `time_limit_ms` または `max_nodes` の早い方で打ち切る。

### 4.3 バリデーション方針

- 不正型、不正範囲、禁止値（安全ガード false）は 400 (`INVALID_ENGINE_CONFIG`) を返す。
- 任意項目が欠けた場合はデフォルト補完する。
- 補完後の `engine_config` をそのまま `game.ai_inference_logs.engine_config` に保存する。

### 4.4 運用ルール

- API では `difficulty` 文字列を受けない。
- `engine_config` を入力として受ける。
- ログ/保存時はプリセット ID ではなく実際の数値セットを保存する。

## 5. DB 連携前提（現状確認）

確認済み migration:

- `backend/supabase/migrations/20260305052035_init_master_piece_stage.sql`
- `backend/supabase/migrations/20260307114000_skill_structured_schema.sql`
- `backend/supabase/migrations/20260307122000_extend_stage_tables.sql`
- `backend/supabase/migrations/20260308235900_create_players_table.sql`
- `backend/supabase/migrations/20260309000001_make_display_name_nullable.sql`

既存スキーマから利用可能な主データ:

- `master.m_piece`, `master.m_move_pattern`, `master.m_move_pattern_vector`
- `master.m_skill`, `master.m_skill_effect`
- `master.m_stage`, `master.m_stage_piece`, `master.m_stage_initial_placement`, `master.m_stage_reward`
- `public.players`

注意:

- `backend/docs/db_schema_players.md` と現行 migration に差分があるため、実装時は migration を正として扱う。

## 6. Phase1 完了項目

1. BFF とエンジン間のリクエスト/レスポンス契約を固定（`docs/phase1-contract.md`）
2. `game` 系テーブル（games/positions/moves/inference_logs）の DDL を作成・適用
3. Rust の最小実装（入力検証 + 基本合法性フィルタ + 1手選択 + メタ出力）を作成

## 7. 次ステップ（Phase2）

1. 盤面表現を SFEN 中心に統一し、完全な合法手生成を Rust 側で実装
2. 探索アルゴリズムを 1-ply から反復深化 + alpha-beta へ拡張
3. BFF で `game.games / positions / moves / ai_inference_logs` の保存処理を本実装
4. 評価関数の重みチューニング用にオフライン対戦バッチを追加
