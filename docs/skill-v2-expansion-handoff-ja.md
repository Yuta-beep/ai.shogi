# Skill v2 展開ハンドオフ

最終更新: 2026-03-16

## 1. 目的

このドキュメントは、skill v2 の基盤実装が入った後の「量産フェーズ」を別の Codex に引き継ぐための実務ガイドです。

このフェーズでやることは 2 つです。

1. skill 定義を増やす
2. executor を横展開する

ここでは `backend` の DB 定義から `shogi-ai` の execution / test までを一貫して扱います。

## 2. 現在の到達点

現時点で通っていること:

- `backend` が `master.m_skill*` から `boardState.skill_registry_v2` / `boardState.skill_definitions_v2` を組み立てられる
- `shogi-ai` が registry / definition を load / validate できる
- `shogi-ai` が skill execution を move score overlay + `SearchState` stateful transition の hybrid として使える
- `apply_status` / `board_hazard` / `modify_movement` / `defense_or_immunity` が stateful に legal move / evaluation へ反映される
- `apply_move()` が `turn_end` / `turn_start` の最小 state transition を持つ
- `backend -> shogi-ai` の E2E が catalog-backed client ベースで 8 family 通っている
- `A〜J` に相当する effect family は unit / execution spec ベースで展開済み

確認済みの代表実装:

- `滝`: `primitive / forced_move`
- `虹`: `composite / modify_movement`
- `光`: `script_hook / reflect_until_blocked`

関連ファイル:

- `shogi-ai/src/engine/skills.rs`
- `shogi-ai/src/engine/skill_executor.rs`
- `shogi-ai/src/engine/tests.rs`
- `shogi-ai/docs/skill-registry-v2-draft.json`
- `shogi-ai/docs/skill-definition-v2-catalog.json`
- `backend/src/services/ai-skill-effects.ts`
- `backend/data/ability/skill_definition_v2_catalog.json`
- `backend/scripts/apply-skill-definition-v2-catalog.mjs`

## 3. このフェーズの原則

- 一度に全 skill をやらない
- 1 つの effect family ごとにまとめて増やす
- 必ず `DB定義 -> tests -> executor -> E2E` の順で進める
- `script_hook` を無理に `primitive` に潰さない
- 既存の legacy `skill_effects` と v2 を二重適用しない

## 4. 作業 1: skill 定義を増やす

### 4.1 何を増やすか

最初は `primitive` を優先する。理由は executor を共通化しやすく、失敗時の切り戻しも簡単だからです。

推奨の優先順:

1. `apply_status`
2. `summon_piece`
3. `transform_piece`
4. `defense_or_immunity`
5. `board_hazard`
6. `return_to_hand`
7. `copy_ability` / `inherit_ability`
8. `script_hook`

### 4.2 1 バッチのサイズ

1 回の作業で増やすのは 3〜8 skill に留める。

悪い進め方:

- 20 件以上を一気に追加する
- effect family が混ざりすぎる

良い進め方:

- `apply_status` だけ 5 件
- `summon_piece` だけ 4 件

### 4.3 候補のまとめ方

各バッチで対象 skill ごとに以下を埋める。

- `skillId`
- `pieceChars`
- `source.skillText`
- `source.sourceFile`
- `source.sourceFunction`
- `classification.implementationKind`
- `trigger`
- `conditions[]`
- `effects[]`
- `scriptHook`

一次ソース:

- `../../SHOGI_GAME/piece_info.html`
- `../../SHOGI_GAME/deck_builder.html`
- `../../SHOGI_GAME/online_battle.html`

### 4.4 追加先

実データ追加の入口:

- `backend/data/ability/skill_definition_v2_catalog.json`

このファイルは正式名称として `catalog` を使う。

### 4.5 DB 反映

追加したら以下を実行する。

```bash
cd /Users/malmalon/Documents/codebase/private/shogi-mobile-app/backend
bun run skills:v2:catalog:apply
```

確認したい場合:

```bash
bun run skills:v2:catalog
```

### 4.6 skill 定義バッチの受け入れ条件

- JSON が registry に対して validate される
- `backend` から v2 payload に乗る
- legacy `skill_effects` に二重で出ない
- 最低 1 本の execution spec test が追加される

## 5. 作業 2: executor を横展開する

### 5.1 実装場所

中心は以下:

- `shogi-ai/src/engine/skill_executor.rs`
- `shogi-ai/src/application/ai_move.rs`
- `shogi-ai/src/engine/tests.rs`

### 5.2 今の execution モデル

現状の skill executor は root move に対する immediate overlay を残しつつ、  
一部 family では `SearchState` を使う stateful effect へ移行しています。

意味:

- search 木全体の完全 state machine ではまだない
- `selected_move` の選好を主目的にしつつ、継続状態は次局面の legal move / evaluation に反映する
- `after_move`, `after_capture`, `continuous_rule`, `continuous_aura`, `turn_start` の一部を hybrid に扱う

これは暫定ではなく、量産フェーズではこの hybrid を前提に拡張してよい。

### 5.3 1 effect family ごとの実装方針

#### `apply_status`

最低限やること:

- 対象 selector が満たされるか判定
- `trace` に status 系 effect を記録
- move score に status の tactical bonus を足す

最初は本当の持続 state を持たなくてよい。

#### `summon_piece`

最低限やること:

- 召喚先 selector を解釈
- 可能なら盤面に piece を置く
- 置けない場合は no-op ではなく「未適用」で扱う

#### `transform_piece`

最低限やること:

- 対象 piece を別 kind として評価盤面に反映
- 変身先が特殊駒コードしかない場合は trace のみでも可

#### `defense_or_immunity`

最低限やること:

- 防御成立条件を trace に記録
- score bonus に反映

#### `board_hazard`

最低限やること:

- origin / destination / adjacent_empty などの selector を扱う
- hazard 自体の継続 state を持たなくても、設置成功に bonus を与える

#### `return_to_hand`

最低限やること:

- 対象 piece を board から除去
- 手駒加算を反映

#### `copy_ability` / `inherit_ability`

最低限やること:

- 完全コピー実装が無理なら trace に反映し、明示的に score bonus を与える
- あいまいな擬似コピーはしない

### 5.4 `script_hook` の扱い

以下のどちらかで進める。

1. `execute_script_hook()` に個別 hook を追加
2. まだ再現性が足りない場合は `trace` のみ実装して score bonus を与える

禁止:

- `script_hook` を `primitive` のふりで中途半端に実装する

### 5.5 tactical bonus の扱い

完全盤面評価にまだ落ちない effect は、`skill_trace_tactical_bonus` 相当の関数で move score へ加点する。

注意:

- bonus は family ごとに一定ルールで管理する
- skill ごとに ad-hoc な値を増やしすぎない
- bonus を足す理由を tests で説明できる状態にする

## 6. テスト方針

### 6.1 追加順

1. 構造テスト
2. execution spec test
3. score improvement test
4. backend -> shogi-ai E2E

### 6.2 `shogi-ai` 側テスト

追加先:

- `shogi-ai/src/engine/tests.rs`

各 skill につき最低でも以下 2 本を追加する。

1. 構造テスト
2. `spec_*` 実行テスト

可能なら加える:

3. `*_skill_improves_move_score_*`

### 6.3 `backend` 側テスト

追加先:

- `backend/src/services/__tests__/ai-skill-effects.test.ts`
- `backend/src/services/__tests__/ai-turn-e2e.test.ts`

やること:

- 新しい skill 群が v2 payload に載ること
- E2E の representative family 群に新しい skill family を追加するか、既存 family case を拡張すること

### 6.4 実行コマンド

`shogi-ai`

```bash
cd /Users/malmalon/Documents/codebase/private/shogi-mobile-app/shogi-ai
cargo fmt --all
cargo test
```

`backend`

```bash
cd /Users/malmalon/Documents/codebase/private/shogi-mobile-app/backend
bun test src/services/__tests__/ai-skill-effects.test.ts
bun x tsc --noEmit --incremental false
```

E2E:

```bash
bun test src/services/__tests__/ai-turn-e2e.test.ts
```

補足:

- E2E は localhost bind が必要なので sandbox 外実行が必要なことがある

## 7. バッチ進行状況

現時点の判定:

- バッチ A: 完了
- バッチ B: 完了
- バッチ C: 完了
- バッチ D: 完了
- バッチ E: 完了
- バッチ F: 完了
- バッチ G: 完了
- バッチ H: 完了
- バッチ I: 完了
- バッチ J: 完了

判定根拠:

- `shogi-ai/src/engine/tests.rs` に A〜J 対応 family の構造 test / spec test / score test が入っている
- `cargo test` は 254 件成功
- `backend/src/services/__tests__/ai-skill-effects.test.ts` に batch A〜J の payload test が入っている

## 8. 推奨バッチ順

### バッチ A: `apply_status`

候補:

- 闇
- 氷
- 時
- 牢
- 病

期待成果:

- `apply_status` family が複数 trigger / target で安定する

### バッチ B: `summon_piece`

候補:

- 苔
- 木
- 嶺
- 墓

期待成果:

- `adjacent_empty` / `reachable_empty_cell` 系 selector が安定する

### バッチ C: `transform_piece`

候補:

- 灯
- あ
- 財
- 鉱

期待成果:

- self / ally / enemy transform の違いが整理される

### バッチ D: `board_hazard` / `return_to_hand`

候補:

- 毒
- 穴
- 薔
- 星

期待成果:

- origin / destination / capture 系 trigger の扱いが安定する

### バッチ E: `copy_ability` / `inherit_ability`

候補:

- 鏡
- 機
- 書
- 豚

期待成果:

- trace-only 実装と本実装の境界が整理される

### バッチ F: `defense_or_immunity`

候補:

- 鎧
- 聖
- 朧
- 魂
- 陽
- 陰

期待成果:

- defense 系の selector / condition の扱いが統一される
- `continuous` と `probability_gated` の差が整理される

### バッチ G: `capture_rule`

候補:

- 砲
- 刀
- 銃
- 雲
- 霊

期待成果:

- `capture_with_leap`
- `multi_capture`
- `capture_constraint`

が同じ family として安定する

### バッチ H: `extra_action`

候補:

- 乙
- 凸
- 煮

期待成果:

- `after_capture` と `after_move` 由来の追加行動が整理される
- tactical bonus だけでなく execution trace の粒度が揃う

### バッチ I: `linked_action`

候補:

- 砂
- 舟
- 機

期待成果:

- 隣接味方 / 同列味方 / 連動駒の判定が整理される
- selector family の再利用が進む

### バッチ J: `script_hook` 個別実装群

候補:

- 光
- 室
- 定
- 辺
- 逃
- 爆

期待成果:

- `execute_script_hook()` の入口が整理される
- trace-only で済ませるものと本実装するものの線引きが明確になる
- 共通 executor に落とせない特殊 skill の backlog が明文化される

## 9. 完了条件

各バッチの完了条件:

- 対象 skill が DB に入っている
- `backend` が v2 payload を返す
- `shogi-ai` が validate できる
- execution test が通る
- 必要な score test が通る
- 既存 test を壊していない

このフェーズ全体の完了条件:

- 代表 skill 3 件だけでなく、複数 family が安定して execution できる
- sample ファイルが実質的に「運用中の定義群」になっている
- 次の工程として「完全な継続状態管理」へ進めるだけの土台ができる

## 10. やってはいけないこと

- 全 skill を一気に入れる
- `script_hook` を曖昧な共通 effect に押し込む
- テストなしで effect family を追加する
- legacy `skill_effects` と v2 を両方有効にする
- `piece_code` と `pieceChars` の対応問題を放置したまま大量投入する

## 11. Codex への短い指示文

次の Codex にはこれで十分です。

> `docs/skill-v2-expansion-handoff-ja.md` に従って、A〜J は完了済みとして扱ってください。次に進めるなら、sample 定義の正式化、継続状態の厳密管理、または E2E ケースの横展開に移ってください。
