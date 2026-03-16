# Shogi AI アルゴリズム概要

最終更新: 2026-03-10

## 1. 目的

このドキュメントは、`shogi-ai` の現行手選択ロジックを日本語で俯瞰するための概要です。
実装の入口は `POST /v1/ai/move` で、最終的に `selected_move` を 1 手返します。

## 2. レイヤ構成

- API層: リクエスト受信・DTO変換・エラー整形
- Application層: 手選択フロー制御 (`compute_ai_move`)
- Engine層: 設定検証、合法手生成、探索、評価、最終選択

## 3. 手選択フロー（現行）

`compute_ai_move` は以下の順序で処理します。

1. `engine_config` をデフォルト補完 + バリデーション
2. `legal_moves` を座標検証して正規化
3. `sfen` があれば SFEN から局面を復元し、探索を実行
4. 探索結果を `legal_moves`（BFF提供）で制約
5. 制約後の候補が空なら、`legal_moves` に対して1手ヒューリスティック評価へフォールバック
6. `sfen` が無い場合は `legal_moves` の1手ヒューリスティック評価
7. スコア上位群から `random_topk` / `temperature` / `blunder_rate` を使って最終手を選択

要点:

- `sfen` がある局面では、`legal_moves` が存在しても探索を先に使う。
- `legal_moves` は整合性のための候補制約として扱う。

## 4. 探索アルゴリズム

探索は反復深化 + negamax(alpha-beta) です。

- 深さ: `2..=max_depth`
- 打ち切り: `max_nodes` または `time_limit_ms` の早い方
- 末端評価: 材料 + 位置(中央) + 機動力

合法手生成で扱う主な制約:

- 自己王手回避
- 二歩
- 打ち歩詰め
- 成り・不成、持ち駒打ち

## 5. 1手ヒューリスティック評価（フォールバック）

探索が使えないとき、以下を使って即時評価します。

- 駒取り価値
- 成りボーナス
- 中央寄りボーナス
- 機動力/王安全の簡易重み

注意:

- この評価は先読みを含まないため、戦術的には弱くなる。
- 実運用では `sfen` を渡して探索ルートを使う前提が推奨。

## 6. 乱択と再現性

最終選択は次で制御します。

- `random_topk`
- `temperature`
- `blunder_rate`
- `blunder_max_loss_cp`
- `random_seed`

`random_seed` 指定時は再現しやすく、未指定時は `game_id + move_no` 由来のseedを使用します。

## 7. 既知の制約

- `quiescence_enabled` は現時点で探索ロジックに未接続
- `max_repeat_draw_bias` は現時点で未使用
- 1手ヒューリスティック評価は防御・交換損得の把握が弱い

## 8. 参照

- API契約: `docs/phase1-contract.md`
- 設計方針: `docs/ai-engine-design-policy.md`
- 実装: `src/application/ai_move.rs`, `src/engine/search.rs`, `src/engine/heuristic.rs`
