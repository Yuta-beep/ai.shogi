# Shogi AI アルゴリズム概要

最終更新: 2026-03-20

## 1. 目的

このドキュメントは `shogi-ai` の手選択ロジックを詳細に説明します。
実装の入口は `POST /v1/ai/move` で、最終的に `selected_move` を 1 手返します。

---

## 2. レイヤ構成

```
┌────────────────────────────────────────────────┐
│ API層  (src/api/)                               │
│  handlers.rs  リクエスト受信・DTO変換・エラー整形  │
└────────────────────┬───────────────────────────┘
                     │
┌────────────────────▼───────────────────────────┐
│ Application層  (src/application/)               │
│  ai_move.rs   手選択フロー制御                   │
└────────────────────┬───────────────────────────┘
                     │
┌────────────────────▼───────────────────────────┐
│ Engine層  (src/engine/)                         │
│  constants.rs   定数一覧                         │
│  config.rs      設定検証・デフォルト補完           │
│  search.rs      合法手生成・探索・局面評価          │
│  heuristic.rs   1手ヒューリスティック評価           │
│  skill_executor.rs  スキル効果シミュレーション      │
│  util.rs        最終手選択（乱択）                 │
│  types.rs       局面・駒・手の型定義               │
│  rules.rs       board_state → RuntimeRules 変換  │
└────────────────────────────────────────────────┘
```

---

## 3. 手選択フロー全体

`compute_ai_move` (src/application/ai_move.rs) の処理順序：

```
1. engine_config をデフォルト補完 + バリデーション
2. board_state から RuntimeRules を構築
   (extra_vectors_by_piece / eval_bonus_by_piece / スキル定義等)
3. legal_moves を座標検証して正規化
4. sfen が存在する場合:
   a. SFEN を SearchState に変換
   b. SearchState にスキル状態 (skill_state) をハイドレート
   c. search_with_iterative_deepening で探索
   d. 探索結果の手を legal_moves で制約
      → 制約後に候補が空なら legal_moves に対してヒューリスティック評価へフォールバック
5. sfen がない場合:
   → legal_moves に対してヒューリスティック評価 (1手先読みなし)
6. スキル効果ボーナスをルート手のスコアに加算 (sfen がある場合のみ)
7. スコア降順ソート後、select_move_index で最終手を確率的に選択
8. selected_move + meta を返す
```

**重要:** `sfen` がある局面では `legal_moves` が存在しても探索を先に使う。
`legal_moves` は整合性のための候補制約として扱い、探索結果をフィルタする。

---

## 4. 探索アルゴリズム: 反復深化 + Negamax (Alpha-Beta)

実装: `src/engine/search.rs`

### 4.1 反復深化 (Iterative Deepening)

```
for depth in 2..=max_depth:
    if nodes >= max_nodes または elapsed >= time_limit_ms: break
    各ルート手について negamax(depth-1) を実行
    全ルート手のスコアが揃った場合のみ last_scores を更新
    揃わなかった場合は前の深さの結果を維持して break
return last_scores
```

- 深さ1の初期スコアは `evaluate_state` による1手評価で初期化する
- 各深さで全手のスコアが出揃わない場合（時間/ノード切れ）は前の深さの結果を使う
- これにより時間切れになっても常に有効なスコアが手元にある

**デフォルト設定 (`src/engine/constants.rs`):**

| 定数 | 値 |
|------|----|
| `DEFAULT_MAX_DEPTH` | 4 |
| `DEFAULT_MAX_NODES` | 50,000 |
| `DEFAULT_TIME_LIMIT_MS` | 500ms |

### 4.2 Negamax + Alpha-Beta 枝刈り

```
negamax(state, depth, alpha, beta):
  if depth == 0 or nodes >= max_nodes or elapsed >= time_limit_ms:
    return evaluate_state(state)

  moves = generate_legal_moves(state)
  if moves.is_empty():
    return -(SCORE_CHECKMATE_BASE - depth)  // 詰まれた。浅いほど重いペナルティ

  best = -SCORE_INF
  for mv in moves:
    next = apply_move(state, mv)
    score = -negamax(next, depth-1, -beta, -alpha)
    best = max(best, score)
    alpha = max(alpha, score)
    if alpha >= beta: break  // Beta cutoff (枝刈り)
  return best
```

- `SCORE_INF = 30,000`
- `SCORE_CHECKMATE_BASE = 29,000`
- Negamax は Minimax の変種で、常に「現在の手番側から見たスコア」を返す。
  再帰呼び出し時に符号反転するだけで手番切り替えを表現する。
- Alpha-Beta 枝刈りにより、明らかにベータ値を超えない枝を探索しない。

**現時点の制約:**
- 指し手の順序付け (move ordering) は未実装。合法手生成順（盤上行優先 → 持ち駒打ち）で探索する。
  ベスト手から先に探索すると Alpha-Beta の効率が最大化されるため、将来的な改善余地がある。
- Quiescence Search は `quiescence_enabled` フラグが存在するが探索ロジックに未接続。

---

## 5. 合法手生成

実装: `src/engine/search.rs` `generate_legal_moves()`

### 5.1 生成順序

1. 盤上の全駒を行優先 (row 0→8, col 0→8) でスキャンして移動手を生成
2. 持ち駒打ちを生成

### 5.2 移動手の種類

**通常駒 (PieceKind::Pawn / Lance / Knight / Silver / Gold / Bishop / Rook / King):**
- 各駒固有の移動ベクトルによる `push_step` (スライド/1歩)
- `rules.extra_vectors_by_piece` に登録されたカスタムベクトルも追加適用
  - `CaptureMode::Normal`: 通常移動として `push_step` で処理

**LeapOverOne (砲型駒) の特殊移動:**
- `CaptureMode::LeapOverOne` のカスタムベクトルに対して2フェーズ処理:
  1. 砲台（最初に当たる駒）の手前まで非取り移動を生成
  2. 砲台を1つ飛び越えた先の最初の敵駒のみ取り可能
- 砲台自体への移動・取りは不可

**成り処理 (`push_promote_variants`):**
- 成れる条件（相手陣 row 0-2 / 自陣 row 6-8）を満たす場合、成りと不成の両方を生成
- `must_promote`（行き場のない駒）は強制成りのみ

### 5.3 合法手フィルタ

疑似手生成後に以下をチェックして非合法手を除外:

1. **自己王手回避**: `apply_move` 後に自玉が攻撃されているか確認
2. **二歩**: 同じ列に味方歩が存在するか
3. **打ち歩詰め (`enforce_uchifuzume=true` 時)**: 歩を打って即詰みになるか確認
4. **ボードハザード制約**: `state.has_board_hazard()` でトラップマス等を回避
5. **駒取り制約**: `state.capture_blocked_by_piece_defense()` で防御スキルによる取り禁止

### 5.4 持ち駒打ち制約

- 歩: 相手陣最終行 (row 0/8) への打ちは禁止
- 香車: 相手陣最終行への打ちは禁止
- 桂馬: 相手陣1・2行目への打ちは禁止
- 歩: 二歩チェック（同列に味方歩がある場合は禁止）

---

## 6. 局面評価関数

実装: `src/engine/search.rs` `evaluate_state()`

スコアは「現在の手番側から見た優位」を cp (センチポーン) 単位で返す。

```
score = material × eval_material_weight
      + center   × eval_position_weight
      + mobility × eval_mobility_weight
```

### 6.1 駒価値 (material)

盤上の全駒について:

```
v = piece_base_value(kind) + (PROMOTION_BONUS_CP=80 if promoted)
s = +1 (自駒) / -1 (敵駒)
material += v * s
```

**駒の基本値 (`src/engine/types.rs` `piece_base_value`):**

| 駒 | cp |
|----|-----|
| 歩 (FU) | 100 |
| 香 (KY) | 300 |
| 桂 (KE) | 320 |
| 銀 (GI) | 500 |
| 金 (KI) | 600 |
| 角 (KA) | 900 |
| 飛 (HI) | 1,000 |
| 王 (OU) | 10,000 |
| カスタム駒 | 700 |

成り駒: 基本値 + 80cp

追加補正:
- `rules.eval_bonus_by_piece` によるカスタムボーナス（board_state から注入）
- スキル由来のペナルティ/ボーナス:
  - `piece_status_penalty()`: 凍結・スタン等の状態異常
  - `movement_modifier_penalty()`: 移動制限
  - `board_hazard_penalty()`: トラップマスの影響
  - `piece_defense_bonus()`: 駒の防御バフ

### 6.2 中央制御 (center)

```
center_bonus = CENTER_DIST_MAX(8.0) - (|row-4| + |col-4|)
center += center_bonus * s  // 中央に近い駒ほど高得点
```

### 6.3 機動力 (mobility)

```
mobility = generate_legal_moves(state, rules, false).len()
```

現在の手番側の合法手数をそのまま使用（相手側の機動力との差分は取らない）。

### 6.4 王安全 (king_safety)

`eval_king_safety_weight` のパラメータは存在するが、現時点で `evaluate_state` 内での計算は未実装。将来の拡張ポイント。

---

## 7. スキル効果のスコア補正

実装: `src/engine/skill_executor.rs` `score_move_with_skill_effects()`

Negamax 探索が完了した後、**ルート手のみ**にスキル評価を加算する:

```
for (idx, score) in &mut scored:
    score += score_move_with_skill_effects(state, moves[idx], rules, cfg)
```

### 7.1 スキルスコア計算

```
base_state  = apply_engine_move(state, mv)      // スキルなしで着手
simulated   = simulate_move_with_skills(...)     // スキル効果を適用

before = evaluate_state_for_side(base_state, mover)
after  = evaluate_state_for_side(simulated.state, mover)
tactical_bonus = skill_trace_tactical_bonus(trace)

skill_score = round((after - before + tactical_bonus) × expected_value)
```

- `expected_value`: スキルの発動確率の積 (1.0 から始まり、確率的スキルで減衰)
- スキルが未定義の駒を動かす場合は `simulate_move_with_skills` が `None` を返し、スキルスコアは 0

### 7.2 対応するトリガー

| trigger type_code | 発動タイミング |
|-------------------|--------------|
| `after_move` | 移動後（常時） |
| `continuous_rule` | 常時適用ルール |
| `continuous_aura` | 常時オーラ効果 |
| `turn_start` | ターン開始時（常時扱い） |
| `after_capture` | 駒取り後（取りの手のみ） |

### 7.3 設計上の注意点

スキル評価は**探索木の外（ルートのみ）**で計算される。
そのため、相手がスキルに対して最適な返し手を打つ前提での読みは行われない。
これは計算コストとのトレードオフであり、将来的には探索木への組み込みが改善余地。

---

## 8. 1手ヒューリスティック評価（フォールバック）

実装: `src/engine/heuristic.rs` `evaluate_move()`

`sfen` がない場合、または探索結果と `legal_moves` が一致しない場合に使用:

```
promote_bonus  = HEURISTIC_PROMOTE_BONUS_CP(60.0) if promote else 0
center_bonus   = CENTER_DIST_MAX(8.0) - (|to_row-4| + |to_col-4|)
capture_value  = piece_capture_cp(captured_piece_code)

positional = (promote_bonus + center_bonus × HEURISTIC_CENTER_WEIGHT(3.0)) × eval_position_weight
material   = capture_value × eval_material_weight
mobility   = HEURISTIC_MOBILITY_BASE_CP(5.0) × eval_mobility_weight
king_safety = HEURISTIC_KING_SAFETY_BASE_CP(2.0) × eval_king_safety_weight

score = (material + positional + mobility + king_safety) × side_bias
```

**`side_bias`:** `side_to_move == "enemy"` なら +1.0、それ以外は -1.0

**駒取り価値 (`piece_capture_cp`):**

| 駒コード | cp |
|---------|-----|
| OU | 10,000 |
| HI, KA | 900 |
| KI | 600 |
| GI | 500 |
| KE, KY | 350 |
| FU | 100 |
| その他 | 150 |

**注意:** この評価は先読みを含まないため戦術的に弱くなる。実運用では `sfen` を渡して探索ルートを使う前提が推奨。

---

## 9. 最終手選択と乱択

実装: `src/engine/util.rs` `select_move_index()`

スコアで降順ソート済みの手リストから、以下の順序で最終手を決定する:

### 9.1 random_topk

上位 `random_topk` 件のみを候補とする（デフォルト1 = 常にベスト手候補のみ）。

### 9.2 blunder_rate（ミス確率）

```
if blunder_rate > 0.0 and rand() < blunder_rate:
    candidates = top_k内で (best_score - score) <= blunder_max_loss_cp の手
    return random.choice(candidates)
```

意図的なミスによる難易度調整用。

### 9.3 temperature（ソフトマックスサンプリング）

```
weight[i] = exp((score[i] - best_score) / temperature)
確率に比例してランダム選択
```

- `temperature = 0.0`（デフォルト）: 常にベスト手（決定論的）
- `temperature` が大きいほど、スコアが低い手も選ばれやすくなる
- 推奨値: `100〜200`（スコア差100〜200cp以内の手を候補に）

### 9.4 乱数シードと再現性

```rust
seed = random_seed ?? make_seed(game_id, move_no)
rng = StdRng::seed_from_u64(seed)
```

`make_seed` は `game_id + move_no` のハッシュを使うため、
**同一 game_id・move_no では常に同じ手が選ばれる**（temperature > 0 でも同様）。

ステージを再挑戦するたびに異なる手を指させたい場合は、
呼び出し側で毎回ランダムな `random_seed` を渡すこと。

---

## 10. 定数一覧

実装: `src/engine/constants.rs`

| 定数名 | 値 | 用途 |
|--------|-----|------|
| `DEFAULT_MAX_DEPTH` | 4 | 探索デフォルト深さ |
| `DEFAULT_MAX_NODES` | 50,000 | 探索ノード上限 |
| `DEFAULT_TIME_LIMIT_MS` | 500 | 思考時間上限 (ms) |
| `DEFAULT_EVAL_MATERIAL_WEIGHT` | 1.0 | 駒価値の重み |
| `DEFAULT_EVAL_POSITION_WEIGHT` | 0.35 | 位置評価の重み |
| `DEFAULT_EVAL_KING_SAFETY_WEIGHT` | 0.25 | 王安全の重み |
| `DEFAULT_EVAL_MOBILITY_WEIGHT` | 0.2 | 機動力の重み |
| `SCORE_INF` | 30,000 | Alpha-Beta の初期値 |
| `SCORE_CHECKMATE_BASE` | 29,000 | 詰まれたときのスコアベース |
| `PROMOTION_BONUS_CP` | 80.0 | 成り駒の評価ボーナス |
| `CENTER_DIST_MAX` | 8.0 | 中央距離の最大値 |
| `HEURISTIC_PROMOTE_BONUS_CP` | 60.0 | ヒューリスティックの成りボーナス |
| `HEURISTIC_CENTER_WEIGHT` | 3.0 | ヒューリスティックの中央重み |
| `HEURISTIC_MOBILITY_BASE_CP` | 5.0 | ヒューリスティックの機動力基礎値 |
| `HEURISTIC_KING_SAFETY_BASE_CP` | 2.0 | ヒューリスティックの王安全基礎値 |

---

## 11. 既知の制約・将来の改善余地

| 項目 | 現状 | 改善案 |
|------|------|--------|
| 指し手順序付け | 未実装（盤走査順） | Killer Move / History / MVV-LVA |
| Quiescence Search | フラグあり・未接続 | 戦術局面の静止探索接続 |
| 王安全評価 | パラメータあり・未実装 | 玉周辺の守り駒カウント等 |
| スキルの探索木統合 | ルートのみ後付け加算 | 探索木内での評価への組み込み |
| 千日手・持将棋 | `max_repeat_draw_bias` は未使用 | 繰り返し検出と回避 |
| 置換表 | 未実装 | 同一局面のスコアキャッシュ |
| 毎回同じ手問題 | game_id+move_no固定seed | 呼び出し側でrandom_seedをランダムに渡す |

---

## 12. 参照

- API 契約: `docs/phase1-contract.md`
- 設計方針: `docs/ai-engine-design-policy.md`
- スキル構造: `docs/skill-structure-v2-design-ja.md`
- 実装:
  - `src/engine/constants.rs` — 定数一覧
  - `src/application/ai_move.rs` — 手選択フロー
  - `src/engine/search.rs` — 探索・合法手生成・評価
  - `src/engine/heuristic.rs` — フォールバック評価
  - `src/engine/skill_executor.rs` — スキル効果スコア補正
  - `src/engine/util.rs` — 最終手選択・乱択
