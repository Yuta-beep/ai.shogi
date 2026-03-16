# Skill v2 次フェーズ 完了メモ

最終更新: 2026-03-16

## 1. 位置づけ

この文書は、skill v2 の「次フェーズ」として進めていた 4 本の作業について、  
未着手メモではなく完了済みの実績メモとして残すものです。

完了済みの対象:

1. catalog / apply script の正式化
2. stateful 実装の第 1 弾
3. stateful 実装の第 2 弾
4. backend E2E の本番寄せと横展開

## 2. 完了サマリ

以下はすべて完了済みです。

- `skill_definition_v2_catalog.json` / `apply-skill-definition-v2-catalog.mjs` への正式化完了
- `samples` 命名を主要フローから除去
- `SearchState` に stateful skill state を導入
- `apply_status` / `board_hazard` / `modify_movement` / `defense_or_immunity` が legal move / evaluation に反映される状態まで実装
- `turn_end` で expire、`turn_start` で refresh / next-turn restriction を処理する最小 state transition を導入
- stateful test を追加済み
- backend E2E が catalog-backed client ベースで 8 family を検証する形まで拡張済み

## 3. 実装済み内容

### 3.1 catalog / apply script 正式化

正式名は以下で確定しています。

- `backend/data/ability/skill_definition_v2_catalog.json`
- `backend/scripts/apply-skill-definition-v2-catalog.mjs`
- `shogi-ai/docs/skill-definition-v2-catalog.json`

この段階で、sample 扱いだった skill v2 定義は正式 catalog と apply script に移行済みです。  
backend 側の適用経路と shogi-ai 側の参照先は、正式名称ベースに揃っています。

### 3.2 `SearchState` 上の stateful skill state

`SearchState` は `skill_state` を持つ構造へ更新済みです。  
継続効果は探索木の状態として保持され、少なくとも以下が stateful に動作します。

- `apply_status`
- `board_hazard`
- `modify_movement`
- `defense_or_immunity`

反映済みの要点:

- `piece_statuses` を状態として保持
- `board_hazards` を状態として保持
- `movement_modifiers` を状態として保持
- `piece_defenses` を状態として保持
- `turn_start_rules` を状態として保持
- `apply_move()` が `turn_end` / `turn_start` の state transition を明示する
- legal move 生成が継続状態を読む
- evaluation が継続状態を読む

これにより、immediate bonus だけに依存せず、探索中の状態遷移として継続効果を扱えるようになっています。

### 3.3 stateful test 実績

以下の stateful test が追加済みです。

- `stateful_apply_status_blocks_the_marked_enemy_piece`
- `stateful_board_hazard_blocks_enemy_entry_and_changes_evaluation`
- `stateful_modify_movement_limits_the_marked_enemy_piece_to_vertical_steps`
- `stateful_defense_or_immunity_blocks_enemy_capture_and_changes_evaluation`
- `turn_boundaries_refresh_moon_cycle_and_expire_swamp_restrictions`

検証している内容:

- `apply_status` がマーク済みの敵駒の行動を実際にブロックする
- `board_hazard` が敵の侵入先を制限し、評価値にも差分を与える
- `modify_movement` が対象敵駒の合法手を state ベースで制限する
- `defense_or_immunity` が protected piece への capture を state ベースでブロックする
- `turn_start` / `turn_end` の境界で expire と refresh が動作する

### 3.4 backend E2E 実績

backend E2E は、単発ケースではなく catalog-backed client を使って 8 つの代表 family をまとめて検証する構成へ拡張済みです。

検証済み family:

1. `forced_move`
2. `apply_status`
3. `summon_piece`
4. `transform_piece`
5. `script_hook`
6. `board_hazard`
7. `modify_movement`
8. `defense_or_immunity`

検証範囲は `catalog / registry -> backend payload -> shogi-ai API -> selected move` です。  
実 DB seed を直接使わなくても、`attachSkillEffectsToAiRequestWithClient()` の entrypoint はそのまま通し、  
手組み fixture ではなく正式 catalog / registry から DB row 風 fixture を起こす形に寄せています。

## 4. テスト実績

確認済みの結果は以下です。

- `cargo test`: `259 passed`
- `ai-skill-effects.test.ts`: `15 passed`
- `tsc`: success
- `ai-turn-e2e.test.ts`: success

`ai-turn-e2e.test.ts` は localhost bind が必要なため、sandbox 外 / localhost bind 許可環境での成功実績として扱います。

## 5. 現在のベースライン

この文書時点の baseline は以下です。

- skill v2 の catalog / apply script 正式化は完了済み
- `SearchState` ベースの stateful 実装は導入済み
- `apply_status` / `board_hazard` / `modify_movement` / `defense_or_immunity` は stateful 検証まで完了済み
- `turn_start` / `turn_end` の最小 state transition は導入済み
- backend E2E は catalog-backed client で 8 family を継続的に守る状態まで到達済み

以後の docs では、この文書を「次にやること」ではなく、  
次の拡張を考えるための完了済みベースラインとして参照する前提です。
