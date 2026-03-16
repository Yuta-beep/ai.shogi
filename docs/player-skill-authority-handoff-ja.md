# プレイヤー Skill Authority Handoff

最終更新: 2026-03-16

## 1. 位置づけ

この文書は、stage battle の server-authoritative 化が通った後の次フェーズとして、  
`プレイヤーが skill-aware に指せるようにする` ための handoff メモです。

今回の対象はプレイヤー UX と合法手生成です。  
`AI がプレイヤーのスキルを先読みしてより賢く応手するアルゴリズム設計` は今回やりません。

## 2. 現在の前提

完了済みの前提:

- backend は player 手 / AI 手を canonical position として commit できる
- backend の `positions` は current position の source of truth として使える
- `shogi-ai` は persisted `board_state.skill_state` を hydrate できる
- frontend stage battle は backend canonical position を採用する

ただし、プレイヤーの合法手 UI はまだ完全には skill-aware ではありません。

## 3. 今の問題

stage battle 画面では、プレイヤーがタップした駒の移動先候補を frontend ローカル関数で計算しています。  
この計算は通常移動寄りで、skill v2 の継続状態や複雑な効果を十分には反映しません。

その結果:

- backend では合法でも frontend で出せない手がある
- frontend では出してしまうが backend commit で弾かれる可能性がある
- skill による移動制限や特殊移動をプレイヤーが自然に使えない

## 4. このフェーズのゴール

1. プレイヤー用の legal moves を backend / engine 側で返す
2. frontend はその legal moves だけを使って移動先や打てる場所を表示する
3. player commit 後の skill 発動結果を canonical position として UI に反映する
4. preview が必要なら backend / engine の canonical apply を再利用する

## 5. やること

### 5.1 backend: player legal moves endpoint

新しい endpoint を追加する。

例:

- `GET /api/v1/games/:gameId/legal-moves`
- または `POST /api/v1/games/:gameId/legal-moves`

責務:

- current position を読む
- 必要な runtime layer / current skill state を読み込む
- engine 側の legal move 生成を呼ぶ
- frontend が使いやすい形で legal moves を返す

返すべき情報の例:

- 駒ごとの legal move 一覧
- drop 候補
- promote 可否
- notation か move id

### 5.2 shogi-ai: legal move generation の API 化

いま engine には内部の合法手生成はあります。  
これを API 経由で player UX に使える形に露出する。

最低限必要なこと:

- persisted `skill_state` を hydrate した `SearchState` から legal moves を生成する
- `apply_status`
- `board_hazard`
- `modify_movement`
- `defense_or_immunity`
  を反映した legal moves を返す

今回やらないこと:

- AI 評価関数の再設計
- AI が player skill を読むための探索変更

### 5.3 frontend: local legal target 計算の置き換え

stage battle 画面では、ローカルの `legalTargetsForCell` を source of truth にしない。

目標:

1. 画面初期化時または局面更新時に legal moves を backend から取得する
2. 駒タップ時は、その cached legal moves から該当候補だけ表示する
3. drop 可能位置も backend 応答に基づいて表示する
4. commit は既存の canonical commit endpoint を使う

### 5.4 frontend: skill 結果表示

player が手を指した後は、backend commit response の canonical position を描画する。  
これにより以下が UI に反映される。

- 召喚
- 変身
- 押し出し
- 状態異常
- 罠
- 防御状態

つまり、`プレイヤーが skill を使えるようにする` とは、  
player 側にも skill-aware な legal move generation と canonical apply 結果表示を入れることです。

### 5.5 preview が必要なら canonical apply を使う

もし UX 上、

- 「この手を指すと何が起きるか」
- 「このスキルでどこへ押し出されるか」

を事前に見せたいなら、backend から engine の canonical apply を preview 用に呼んでよいです。

ただし、まずは legal moves と commit 後の canonical position 反映を優先すること。  
preview は必須ではありません。

## 6. 明示的にやらないこと

今回のフェーズでは以下をやらない。

- AI がプレイヤーの skill を先読みして評価関数や探索を作り直すこと
- プレイヤー用と AI 用で別 skill system を作ること
- online battle を本実装へ広げること

スコープは stage battle の player UX に限定する。

## 7. 推奨実装順

1. shogi-ai で legal move output の API を追加する
2. backend に `games/:id/legal-moves` を追加する
3. backend test で current position -> legal moves 応答を固定する
4. frontend が legal moves endpoint を使うよう変更する
5. stage battle test を更新する
6. 必要なら preview を後から足す

## 8. テスト方針

### 8.1 shogi-ai

- hydrated `skill_state` を含む局面で legal moves を返す test
- `modify_movement` で player の移動先候補が変わる test
- `board_hazard` / `defense_or_immunity` が player legal moves に効く test

### 8.2 backend

- legal moves endpoint handler test
- current position と `board_state.skill_state` を読んで legal moves を返す service test
- player move commit 前後で legal moves が更新される test

### 8.3 frontend

- 駒タップ時に backend legal moves のみを表示する test
- drop 候補が backend 応答由来になる test
- player move commit 後に canonical position を描画する test

### 8.4 E2E

最低 1 本は以下を通す。

1. stage battle 開始
2. legal moves 取得
3. player が skill-aware な手を選ぶ
4. commit
5. canonical position 描画
6. enemy turn へ遷移

## 9. 主に触るファイル

### 9.1 backend

- `backend/src/server/handlers/v1/games/*`
- `backend/src/services/game-move.ts`
- legal moves 用 service / contract / parser

### 9.2 frontend

- `frontend/src/features/stage-shogi/ui/stage-shogi-screen.tsx`
- `frontend/src/usecases/stage-battle/*`
- stage battle の contract / datasource / repository

### 9.3 shogi-ai

- `shogi-ai/src/api/*`
- `shogi-ai/src/application/*`
- `shogi-ai/src/engine/search.rs`
- `shogi-ai/src/engine/types.rs`

## 10. 完了条件

- player legal moves endpoint が追加されている
- legal move generation が hydrated `skill_state` を反映する
- frontend が local legal target 計算ではなく backend legal moves を使う
- player が skill-aware な移動 / drop を UI 上で選べる
- commit 後は canonical position で結果が描画される
- backend / frontend / shogi-ai の test が追加される

## 11. Codex へ渡す短い依頼文

```text
docs/player-skill-authority-handoff-ja.md に従って、stage battle でプレイヤーが skill-aware に指せるようにしてください。今回のスコープは player UX と legal move generation であり、AI がプレイヤーの skill を先読みして賢く応手するアルゴリズム設計はまだやらなくてよいです。まず shogi-ai に legal moves を返す API を追加し、persisted board_state.skill_state を hydrate した局面から player legal moves を生成してください。次に backend に games/:id/legal-moves 相当の endpoint を追加し、最後に frontend stage battle が local legal target 計算ではなく backend legal moves を使うように変更してください。commit 後は既存の canonical position response を描画する形を維持し、backend / frontend / shogi-ai のテストまで通してください。
```
