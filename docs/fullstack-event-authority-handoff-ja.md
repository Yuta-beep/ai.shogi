# 将棋アプリ 一体イベント化 Handoff

最終更新: 2026-03-16

## 1. 位置づけ

この文書は、skill v2 実装後の次フェーズとして、  
`frontend -> backend -> shogi-ai -> backend persistence -> frontend` の一連を  
server-authoritative に寄せるための handoff メモです。

対象は skill v2 単体ではなく、将棋アプリ全体のイベント整合です。

## 2. 現在の到達点

以下は完了済みです。

- backend は skill v2 catalog から `skill_registry_v2` / `skill_definitions_v2` を組み立てて `shogi-ai` に渡せる
- `shogi-ai` は skill v2 の loader / validator / execution / stateful skill state を持つ
- `apply_status` / `board_hazard` / `modify_movement` / `defense_or_immunity` は legal move / evaluation に反映される
- backend E2E は catalog-backed client 経路で 8 family を検証済み

現在つながっている AI 呼び出し経路:

1. frontend が `/api/v1/ai/move` を呼ぶ
2. backend が skill payload を補完する
3. backend が `shogi-ai` の `/v1/ai/move` を呼ぶ
4. `shogi-ai` が探索し、selected move を返す
5. backend が AI inference log と move を保存する

## 3. まだ未完の点

skill v2 は通っているが、対局イベント全体の source of truth はまだ分裂しています。

### 3.1 frontend が局面の主導権を持っている

- stage battle は player 手も AI 手も frontend ローカル state で盤面更新している
- AI request では `boardState: {}` を送っており、継続状態は frontend から渡していない
- canonical position が backend ではなく frontend にある

### 3.2 backend の `positions` は次局面を正しく保存していない

- `persistAiTurn()` は move 自体は保存するが、`positions` には request 側の `sfen` / `hands` / `board_state` をそのまま upsert している
- つまり AI 手適用後の局面が game DB の source of truth になっていない

### 3.3 player move endpoint がない

- 現状は `POST /api/v1/ai/move` があるだけで、player の一手を backend に送る server-authoritative endpoint がない
- player 手の保存、検証、次局面生成が backend の責務になっていない

### 3.4 `shogi-ai` は persisted skill state を hydrate していない

- `SearchState` は `skill_state` を持つ
- ただし current `board_state` から `piece_statuses` / `board_hazards` / `movement_modifiers` / `piece_defenses` / `turn_start_rules` を復元して探索開始していない
- いまの stateful 実装は、その局面でシミュレートした skill に強く、DB に保存された継続状態の再開にはまだ弱い

### 3.5 online battle は対象外のまま

- online battle はまだ mock usecase ベース
- 今回は stage battle を server-authoritative にするのが先

## 4. このフェーズのゴール

1. player 手を backend へ送る endpoint を追加する
2. AI 手も player 手も共通の move apply / position persist で保存する
3. backend の `positions` を canonical position とする
4. persisted `board_state` から `shogi-ai` が `skill_state` を hydrate する
5. frontend stage battle が backend response を canonical position として採用する
6. frontend -> backend -> shogi-ai -> backend persistence までの E2E を通す

## 4.1 既存 `/api/v1/ai/move` があるのに新しい endpoint が必要な理由

既存の `/api/v1/ai/move` は、基本的に `AI に次の一手を選ばせる` API です。  
責務は以下に寄っています。

- request を受ける
- skill payload を補完する
- `shogi-ai` に渡す
- `selectedMove` を返す

一方で、今回必要なのは `対局イベントを確定する` API です。  
必要な責務は以下です。

- player が指した手を受ける
- 現在局面に対して move を検証する
- move を適用して canonical next position を作る
- `moves` と `positions` を保存する
- 次に誰の手番かを確定する

つまり、`thinking API` と `move commit API` は責務が違います。

整理すると:

- 既存 `/api/v1/ai/move`
  - AI の思考 API
  - selected move を返す
- 新しい `POST /api/v1/games/:gameId/moves` 相当
  - 対局イベント確定 API
  - player 手も AI 手も同じ event model で commit する

設計上は、この 2 つを分ける方が自然です。  
もし実装上どうしても endpoint 数を増やしたくない場合でも、少なくとも内部責務として
`select move` と `apply/persist move` を分離すること。

## 5. 推奨実装順

### 5.1 backend: move apply/persist 共通サービス

まず backend に、手を適用して次局面を作る共通サービスを作る。

最低限必要な責務:

- 現在 position の取得
- move の検証
- move の適用
- 次局面の `sfen` / `hands` / `board_state` 生成
- `moves` 保存
- `positions` 更新

AI 手と player 手で別々に盤面適用しないこと。  
共通の apply service を呼ぶ構成に寄せること。

### 5.2 backend: player move endpoint

新しい endpoint を追加する。

例:

- `POST /api/v1/games/:gameId/moves`

最低限の request:

- `moveNo`
- `actorSide`
- `move`
- 必要なら `stateHash`

この endpoint は以下を行うこと。

- game/position を読む
- actor side と moveNo を検証する
- move を合法性チェックする
- canonical next position を保存する
- 次局面を response で返す

### 5.3 backend: AI turn persistence の修正

`persistAiTurn()` / `upsertPosition()` は現在の request position ではなく、  
selected move 適用後の canonical position を保存するように修正する。

保存対象:

- `sfen`
- `hands`
- `board_state`
- `side_to_move`
- `turn_number`
- `move_count`

### 5.4 shogi-ai: persisted skill state hydrate

`parse_runtime_rules()` の責務は registry / definition / legacy runtime 解釈に留める。  
current skill state は別処理で `SearchState` に hydrate する。

最低限 hydrate 対象:

- `piece_statuses`
- `board_hazards`
- `movement_modifiers`
- `piece_defenses`
- `turn_start_rules`

`board_state` に current state が無い場合は、従来どおり空 state で動くこと。

### 5.5 frontend: stage battle の canonical position 採用

stage battle では frontend ローカル apply を source of truth にしない。

目標フロー:

1. player が手を選ぶ
2. frontend が backend の player move endpoint に送る
3. backend が canonical next position を返す
4. enemy turn なら frontend が `/api/v1/ai/move` を呼ぶ
5. backend が AI 手を保存し、次局面を返す
6. frontend は返ってきた canonical position を描画する

optimistic UI を入れるのはよいが、最終的には backend response を正とすること。

## 6. `board_state` の整理方針

今後は `board_state` に runtime 定義と current state が混在しうるため、  
責務の分離が分かるキー構造に整理すること。

最低限の考え方:

- runtime layer
  - `skill_registry_v2`
  - `skill_definitions_v2`
  - legacy `skill_effects`
- current state layer
  - `skill_state`
    - `piece_statuses`
    - `board_hazards`
    - `movement_modifiers`
    - `piece_defenses`
    - `turn_start_rules`

実際のキー名は実装と migration に合わせてよいが、  
`runtime rules` と `current state` が区別できる形にすること。

## 7. テスト方針

### 7.1 backend

追加するべき test:

- move apply service の unit test
- `POST /api/v1/games/:gameId/moves` handler test
- player move 後に `positions` が next position へ更新される test
- AI move 後に `positions` が next position へ更新される test

### 7.2 shogi-ai

追加するべき test:

- persisted `board_state.skill_state` を hydrate できる test
- hydrate した `piece_statuses` が legal move / evaluation に効く test
- hydrate した `board_hazards` / `movement_modifiers` / `piece_defenses` が探索に効く test

### 7.3 frontend

追加するべき test:

- player move 後に backend response の canonical position を採用する test
- AI move 後も local apply ではなく backend canonical position を採用する test
- stage battle screen から player move endpoint と AI move endpoint が正しい順で呼ばれる test

### 7.4 E2E

最低 1 本は以下を通すこと。

1. game 作成
2. player move
3. canonical next position 保存
4. AI move
5. canonical next position 保存
6. frontend がその position を描画

localhost bind が必要な E2E は必要に応じて権限付きで実行すること。

## 8. 主に触るファイル

### 8.1 backend

- `backend/src/services/ai-turn.ts`
- `backend/src/services/game-runtime.ts`
- `backend/src/services/game-session.ts`
- `backend/src/server/handlers/v1/ai/move.ts`
- `backend/src/server/handlers/v1/games/create.ts`
- `backend/src/lib/ai-engine-client.ts`
- `backend/src/lib/ai-engine-contract.ts`
- `backend/src/services/ai-skill-effects.ts`

### 8.2 frontend

- `frontend/src/features/stage-shogi/ui/stage-shogi-screen.tsx`
- `frontend/src/usecases/stage-battle/request-ai-move-usecase.ts`
- stage battle 関連 usecase / repository / handler

### 8.3 shogi-ai

- `shogi-ai/src/application/ai_move.rs`
- `shogi-ai/src/engine/types.rs`
- `shogi-ai/src/engine/search.rs`
- `shogi-ai/src/engine/rules.rs`
- `shogi-ai/src/engine/skill_executor.rs`

## 9. 完了条件

- player move endpoint が追加されている
- backend が AI 手 / player 手の両方で canonical next position を保存する
- `positions.board_state` に current skill state が保存される
- `shogi-ai` が persisted `skill_state` を hydrate できる
- frontend stage battle が backend canonical position を採用する
- unit / integration / E2E test が追加されている
- 少なくとも以下が通る
  - backend test
  - frontend test
  - `cargo test`
  - typecheck
  - localhost bind 許可環境での E2E

## 10. Codex へ渡す短い依頼文

```text
docs/fullstack-event-authority-handoff-ja.md に従って、stage battle を server-authoritative に寄せてください。まず backend に player move endpoint と canonical position 更新の共通 apply/persist サービスを実装し、次に AI turn でも selected move 適用後の next position を保存するよう修正してください。その後、persisted board_state から shogi-ai が skill_state を hydrate できるようにし、最後に frontend stage battle を backend response の canonical position を採用する形へ変更してください。backend / frontend / shogi-ai のテストと、必要なら権限付きでの E2E 実行まで確認してください。
```
