# Remaining Skill Backlog

## 概要

- 対象: review 台帳の未対応 10 件（98, 101, 102, 104, 105, 107, 108, 109, 110, 111）
- 今回の結論:
  - `implemented`: 2 件（102 宋, 105 進）
  - `script_hook`: 2 件（101 安, 104 逸）
  - `out_of_scope`: 6 件（98 煽, 107 艸, 108 閹, 109 賚, 110 殲, 111 膠）
- stage battle 基準:
  - `SHOGI_GAME` に具体的な runtime 根拠があるものだけ catalog 化する
  - 共通 executor に落ちるものは `primitive`、落ちないものは `script_hook`
  - 盤面効果や legal move への意味が曖昧なものは backlog へ送る

## 判定一覧

| ID | 駒 | 元説明 | `SHOGI_GAME` 根拠 | 判定 | 理由 | 次にやるなら |
| --- | --- | --- | --- | --- | --- | --- |
| 98 | 煽 | 相手を煽りたい人の為に。 | `piece_info.html:900` に「移動時相手を煽る」、`online_battle.html:12040-12054` に `triggerTauntEffect`。盤面・手駒・合法手への影響は無し。 | `out_of_scope` | 現状は演出とメッセージのみで、stage battle の canonical position や legal move に寄与しない。 | 挑発が AI 評価値、ヘイト、行動制約などに影響する仕様を別途定義する。 |
| 101 | 安 | 敵の駒を安くする。 | `piece_info.html:904`、`online_battle.html:12344-12390` の `triggerAnSkill`。移動時に敵の「金」「銀」「銅」1体を「歩」に変える。 | `script_hook` | board-wide な対象選択と `銅` を含む変換対象は現行 primitive selector では表しにくい。stage battle では決定論 hook で近似可能。 | 乱択ポリシーと `銅` を含む custom piece 変換 substrate を入れる。 |
| 102 | 宋 | 味方に繁栄をもたらす。 | `piece_info.html:905`、`online_battle.html:12396-12456` の `triggerSongSkill`。移動時20%で周囲1マスの空きに「金」を召喚。 | `implemented` | `chance_roll` + `adjacent_empty_exists` + `summon_piece` で stage battle 向けの最小表現が可能。 | 原作どおりのランダム配置順を canonical server state に寄せる。 |
| 104 | 逸 | 敵駒を盤面から逸脱させる。 | `piece_info.html:907`、`online_battle.html:12106-12164` の `triggerEscapeSkill`。移動時30%で敵駒1体を相手手駒へ送る。 | `script_hook` | board-wide の敵ランダム選択は現行 `send_to_hand` primitive の隣接 selector では不足。専用 hook なら stage battle 近似が可能。 | 乱択対象選択の canonical 仕様と replay 再現方法を決める。 |
| 105 | 進 | 次はどこに進んでいくのか。 | `piece_info.html:908`、`online_battle.html:11904-11907`, `14119-14124`。毎ターン `advanceMoveType` を更新し、`resolvePieceType` がその移動型を返す。 | `implemented` | 既存の `turn_start` + `modify_movement(cyclic_pattern_change)` に素直にマッピングできる。 | 原作の完全ランダム系列と stage battle の deterministic cycle を一致させる。 |
| 107 | 艸 | 草の力を操り盤面を支配する自然の駒。 | `deck_builder.html:1370` にコストのみ。`piece_info.html` / `online_battle.html` に skill 記述も runtime も見当たらない。 | `out_of_scope` | 説明文以外の根拠が無く、対象・発動条件・持続・盤面効果が未確定。 | 元ゲーム側に piece_info と runtime 実装を追加し、盤面効果を明文化する。 |
| 108 | 閹 | 敵の動きを封じる封印の駒。 | `deck_builder.html:1371` にコストのみ。`piece_info.html` / `online_battle.html` に skill 記述も runtime も見当たらない。 | `out_of_scope` | `封` と似た雰囲気はあるが、封印対象・範囲・持続が特定できない。 | seal / stun / move restriction のどれかを仕様化し、対象 selector を定義する。 |
| 109 | 賚 | 報酬を与え味方を強化する恩恵の駒。 | `deck_builder.html:1372` にコストのみ。`piece_info.html` / `online_battle.html` に skill 記述も runtime も見当たらない。 | `out_of_scope` | 「報酬」「強化」が gain_piece / buff / summon のどれか決められない。 | 強化対象、報酬種別、持続、発動契機を追加する。 |
| 110 | 殲 | 敵を一掃する殲滅の駒。 | `deck_builder.html:1373` にコストのみ。`piece_info.html` / `online_battle.html` に skill 記述も runtime も見当たらない。 | `out_of_scope` | remove_piece / multi_capture / board wipe のどれか判断できず、stage battle へ落とせない。 | 範囲、除外対象、確率、王への適用可否を含めて仕様を固定する。 |
| 111 | 膠 | 盤面を膠着させ敵の動きを止める粘着の駒。 | `deck_builder.html:1374` にコストのみ。`piece_info.html` / `online_battle.html` に skill 記述も runtime も見当たらない。 | `out_of_scope` | freeze / hazard / move restriction のどれか未確定で、既存 status への寄せ先も決められない。 | 盤面マス効果か駒 status か、持続ターンと解除条件を定義する。 |

## 実装メモ

### `implemented`

| ID | catalog 化 | stage battle 近似 |
| --- | --- | --- |
| 102 宋 | `primitive / summon_piece` | 20% 発動、隣接空き 1 マスのうち最初の空きへ `金` を召喚 |
| 105 進 | `primitive / modify_movement` | `turn_start` ごとに `cyclic_pattern_change` を積み、既存の turn boundary 更新へ寄せる |

### `script_hook`

| ID | catalog 化 | stage battle 近似 |
| --- | --- | --- |
| 101 安 | `script_hook / discount_enemy_elite_to_pawn` | 原作の乱択代わりに、盤面先頭の敵 `金/銀` を `歩` へ変換 |
| 104 逸 | `script_hook / eject_enemy_piece_to_hand` | 原作の乱択代わりに、盤面先頭の敵 1 体を相手手駒へ送る |

### `out_of_scope`

- 98 煽: 演出だけで盤面 state に影響しない。
- 107 艸: コスト情報しか無い。
- 108 閹: コスト情報しか無い。
- 109 賚: コスト情報しか無い。
- 110 殲: コスト情報しか無い。
- 111 膠: コスト情報しか無い。
