# Shogi AI スキル構造化 v2 設計

最終更新: 2026-03-15

## 1. 目的

このドキュメントは、特殊駒スキルを AI エンジンで解釈可能な形に再構造化するための設計書です。

対象:

- `backend` 側のマスターデータ構造
- `shogi-ai` 側の入力スキーマと実行期待
- 管理 UI のプルダウン選択を支えるグループ定義

前提:

- 元スキルの説明文と元実装は `../../SHOGI_GAME` を一次ソースとする
- `skill_desc` の自然文だけでは AI 実装とテスト駆動開発の単位が曖昧になる
- すべてを共通化するのではなく、`primitive` と `script_hook` を明示的に分離する

## 2. 設計方針

### 2.1 何を解決したいか

現行の `master.m_skill` / `master.m_skill_effect` は「説明文からの要約」には使えるが、AI がそのまま解釈するには粗い。

特に不足しているのは以下です。

- `trigger` が `passive` に潰れすぎている
- `target_rule` がセレクタと対象範囲を兼ねている
- `effect_summary_type` が実行器ではなく要約ラベルになっている
- 固有ロジックが必要なスキルと共通実行できるスキルの境界が曖昧
- 管理画面でプルダウンを組むための選択肢マスタが無い

### 2.2 構造の結論

スキルは以下 4 層で持つ。

1. `source`: 元説明文・元実装関数の対応
2. `registry`: プルダウン用のグループ定義と選択肢
3. `definition`: AI が読む構造化スキル定義
4. `execution`: `primitive` / `composite` / `script_hook` の実行方式

## 3. 論理スキーマ

### 3.1 SkillDefinition

```ts
type SkillDefinition = {
  skillId: number;
  pieceChars: string[];
  source: {
    skillText: string;
    sourceKind: 'piece_info' | 'deck_builder' | 'online_battle' | 'manual';
    sourceFile: string | null;
    sourceFunction: string | null;
  };
  classification: {
    implementationKind: 'primitive' | 'composite' | 'script_hook';
    tags: string[];
  };
  trigger: TriggerNode;
  conditions: ConditionNode[];
  effects: EffectNode[];
  scriptHook: string | null;
  notes: string | null;
};
```

### 3.2 TriggerNode

```ts
type TriggerNode = {
  group: string;
  type: string;
};
```

### 3.3 ConditionNode

```ts
type ConditionNode = {
  order: number;
  group: string;
  type: string;
  params: Record<string, unknown>;
};
```

### 3.4 EffectNode

```ts
type EffectNode = {
  order: number;
  group: string;
  type: string;
  target: {
    group: string;
    selector: string;
  };
  params: Record<string, unknown>;
};
```

## 4. プルダウン用グループ定義

管理 UI では、各スキーマを `group -> option` の二段プルダウンで選べるようにする。

### 4.1 TriggerSchema

| group_code | 用途 | options |
|---|---|---|
| `event_move` | 自駒移動に紐づく発火 | `before_move`, `after_move` |
| `event_capture` | 取る/取られるに紐づく発火 | `before_capture`, `after_capture`, `after_captured` |
| `event_turn` | ターンの境界 | `turn_start`, `turn_end` |
| `event_other` | 他駒イベント反応 | `other_piece_moved`, `other_piece_captured` |
| `continuous` | 常時評価 | `continuous_aura`, `continuous_rule`, `board_state_changed` |
| `special` | 条件成立・専用処理 | `condition_met`, `manual_action`, `script_hook` |

補足:

- `passive` は trigger ではないので禁止する
- `passive` に見えるものは `continuous_*` に分解する

### 4.2 TargetSelectorSchema

| group_code | 用途 | options |
|---|---|---|
| `self` | 自分自身 | `self_piece` |
| `adjacent` | 隣接マス/周囲8マス | `adjacent_enemy`, `adjacent_ally`, `adjacent_any`, `adjacent_empty`, `adjacent_8_enemy` |
| `line` | 行・列・正面・左右 | `left_right_enemy`, `front_enemy`, `same_row_ally`, `same_row_or_col_ally` |
| `board` | 盤全体や起点マス | `random_board_cell`, `origin_cell`, `destination_cell`, `reachable_empty_cell` |
| `hand` | 持ち駒 | `enemy_hand_random`, `ally_hand_random`, `ally_hand_piece` |
| `global` | 盤全体の駒集合 | `all_enemy`, `all_ally`, `all_enemy_high_stroke` |
| `derived` | 選択済み/コピー由来 | `copied_target`, `selected_target`, `script_hook` |

### 4.3 EffectSchema

| group_code | 用途 | options |
|---|---|---|
| `piece_state` | 状態異常・封印・無効化 | `apply_status`, `remove_status`, `seal_skill`, `disable_piece`, `defense_or_immunity` |
| `piece_position` | 位置や移動挙動変更 | `forced_move`, `linked_action`, `reflective_movement`, `modify_movement` |
| `piece_lifecycle` | 駒の消滅・復活・持ち駒化 | `remove_piece`, `return_to_hand`, `revive`, `substitute` |
| `piece_generation` | 召喚・変身・獲得 | `summon_piece`, `transform_piece`, `gain_piece` |
| `capture_rule` | 取り方の変更 | `capture_constraint`, `capture_with_leap`, `multi_capture` |
| `action_economy` | 行動回数変更 | `extra_action` |
| `meta_skill` | コピー・継承 | `copy_ability`, `inherit_ability` |
| `board_control` | 盤面マスの変質 | `board_hazard` |
| `special` | 特殊合成・固有処理 | `composite`, `script_hook` |

### 4.4 ConditionSchema

| group_code | 用途 | options |
|---|---|---|
| `probability` | 確率判定 | `chance_roll` |
| `piece_presence` | 特定駒の存在判定 | `ally_piece_exists`, `enemy_piece_exists`, `same_row_or_col_piece_exists` |
| `board_state` | 盤面条件 | `adjacent_enemy_exists`, `adjacent_empty_exists`, `target_not_king` |
| `history` | 直前手や履歴依存 | `last_enemy_move_exists`, `captured_by_enemy` |
| `special` | 固有条件 | `script_hook` |

### 4.5 ParamSchema

| group_code | 用途 | params |
|---|---|---|
| `probability` | 発動率 | `procChance` |
| `duration` | 持続 | `durationTurns` |
| `range` | 範囲や距離 | `radius`, `distance` |
| `count` | 件数や対象数 | `count`, `selection` |
| `piece_ref` | 参照駒 | `spawnPiece`, `transformTo`, `pieceFilter` |
| `movement_rule` | 移動方式 | `movementRule`, `directionRule` |
| `status_rule` | 状態異常の種類 | `statusType` |
| `special` | 専用引数 | `scriptArgs` |

## 5. DB設計（backend 側に期待する形）

このリポジトリは DB migration を持たないが、AI が安定してスキルを読めるよう、`backend` 側 DB は以下の形を期待する。

方針:

- 既存 `master.m_skill` と `master.m_skill_effect` は活かす
- プルダウン選択肢のための registry テーブルを追加する
- 条件式のために `m_skill_condition` を追加する
- 旧 `effect_summary_type` 依存は段階的に薄める

### 5.1 `master.m_skill_schema_group`

スキーマグループ定義。

| カラム | 型 | 必須 | 説明 |
|---|---|---|---|
| `schema_group_id` | `bigint` | Yes | PK（identity） |
| `schema_kind` | `text` | Yes | `trigger/target/effect/condition/param` |
| `group_code` | `text` | Yes | `event_move` などの内部コード |
| `group_name` | `text` | Yes | 表示名 |
| `description` | `text` | No | 説明 |
| `sort_order` | `smallint` | Yes | UI 表示順 |
| `is_active` | `boolean` | Yes | 有効フラグ |
| `created_at` | `timestamptz` | Yes | 作成日時 |
| `updated_at` | `timestamptz` | Yes | 更新日時 |

主な制約:

- `schema_kind in ('trigger','target','effect','condition','param')`
- `sort_order >= 1`
- `unique (schema_kind, group_code)`

### 5.2 `master.m_skill_schema_option`

プルダウン選択肢定義。

| カラム | 型 | 必須 | 説明 |
|---|---|---|---|
| `schema_option_id` | `bigint` | Yes | PK（identity） |
| `schema_kind` | `text` | Yes | `trigger/target/effect/condition/param` |
| `group_code` | `text` | Yes | 所属グループ |
| `option_code` | `text` | Yes | `after_move` などの内部コード |
| `option_name` | `text` | Yes | 表示名 |
| `description` | `text` | No | 説明 |
| `validation_json` | `jsonb` | Yes | 選択時の制約 |
| `default_params_json` | `jsonb` | Yes | デフォルトパラメータ |
| `is_script_only` | `boolean` | Yes | `script_hook` 専用か |
| `sort_order` | `smallint` | Yes | 表示順 |
| `is_active` | `boolean` | Yes | 有効フラグ |
| `created_at` | `timestamptz` | Yes | 作成日時 |
| `updated_at` | `timestamptz` | Yes | 更新日時 |

主な制約:

- `sort_order >= 1`
- `unique (schema_kind, option_code)`
- FK 相当: `(schema_kind, group_code)` は `m_skill_schema_group` と整合する

### 5.3 `master.m_skill` の追加カラム

既存テーブルを親定義として使い、以下を追加する。

| カラム | 型 | 必須 | 説明 |
|---|---|---|---|
| `implementation_kind` | `text` | Yes | `primitive/composite/script_hook` |
| `trigger_group` | `text` | Yes | `event_move` など |
| `trigger_type` | `text` | Yes | `after_move` など |
| `source_kind` | `text` | Yes | `piece_info/deck_builder/online_battle/manual` |
| `source_file` | `text` | No | 元ファイル |
| `source_function` | `text` | No | 元実装関数名 |
| `tags_json` | `jsonb` | Yes | 検索/集計用タグ配列 |

主な制約:

- `implementation_kind in ('primitive','composite','script_hook')`
- `source_kind in ('piece_info','deck_builder','online_battle','manual')`

補足:

- 旧 `trigger_timing` は互換のため残してよい
- 旧 `effect_summary_type` は一覧表示用に残してよい
- AI は新カラムを優先して読む

### 5.4 `master.m_skill_effect` の追加/読み替え

既存テーブルを effect 配列として使い、以下の意味で読む。

| カラム | 型 | 必須 | 説明 |
|---|---|---|---|
| `effect_order` | `smallint` | Yes | 実行順 |
| `effect_group` | `text` | Yes | `piece_state` など |
| `effect_type` | `text` | Yes | `apply_status` など |
| `target_group` | `text` | Yes | `adjacent` など |
| `target_selector` | `text` | Yes | `adjacent_enemy` など |
| `params_json` | `jsonb` | Yes | effect 固有パラメータ |

既存列との対応:

- `effect_type`: 継続利用
- `target_rule`: 将来的には `target_selector` へ置換または非推奨化
- `proc_chance`, `duration_turns`, `radius`, `value_num`, `value_text`: 当面は残して `params_json` にも正規化して入れる

### 5.5 `master.m_skill_condition`

条件式定義。新規追加。

| カラム | 型 | 必須 | 説明 |
|---|---|---|---|
| `skill_condition_id` | `bigint` | Yes | PK（identity） |
| `skill_id` | `bigint` | Yes | FK -> `master.m_skill(skill_id)` |
| `condition_order` | `smallint` | Yes | 評価順 |
| `condition_group` | `text` | Yes | `probability` など |
| `condition_type` | `text` | Yes | `chance_roll` など |
| `params_json` | `jsonb` | Yes | 条件パラメータ |
| `is_active` | `boolean` | Yes | 有効フラグ |
| `created_at` | `timestamptz` | Yes | 作成日時 |
| `updated_at` | `timestamptz` | Yes | 更新日時 |

主な制約:

- `condition_order >= 1`
- `unique (skill_id, condition_order)`

## 6. 期待する JSON 例

### 6.1 Primitive 例: 滝

```json
{
  "skillId": 65,
  "pieceChars": ["滝"],
  "source": {
    "skillText": "移動時20%で周囲の敵駒を押し流す。",
    "sourceKind": "online_battle",
    "sourceFile": "online_battle.html",
    "sourceFunction": "triggerWaterfallSkill"
  },
  "classification": {
    "implementationKind": "primitive",
    "tags": ["move_trigger", "adjacent", "forced_move"]
  },
  "trigger": {
    "group": "event_move",
    "type": "after_move"
  },
  "conditions": [
    {
      "order": 1,
      "group": "probability",
      "type": "chance_roll",
      "params": {
        "procChance": 0.2
      }
    }
  ],
  "effects": [
    {
      "order": 1,
      "group": "piece_position",
      "type": "forced_move",
      "target": {
        "group": "adjacent",
        "selector": "adjacent_enemy"
      },
      "params": {
        "movementRule": "push_away",
        "count": "all"
      }
    }
  ],
  "scriptHook": null,
  "notes": null
}
```

### 6.2 Script Hook 例: 室

```json
{
  "skillId": 99,
  "pieceChars": ["室"],
  "source": {
    "skillText": "セーフルームを用意して「王」を守る。",
    "sourceKind": "online_battle",
    "sourceFile": "online_battle.html",
    "sourceFunction": "triggerSafeRoomSkill"
  },
  "classification": {
    "implementationKind": "script_hook",
    "tags": ["king_control", "special_rule"]
  },
  "trigger": {
    "group": "event_move",
    "type": "after_move"
  },
  "conditions": [],
  "effects": [],
  "scriptHook": "safe_room_king_relocation",
  "notes": "共通 executor では扱わない"
}
```

## 7. `shogi-ai` 側で期待する実装

### 7.1 ローダ

エンジンは、BFF から受け取るスキル定義を Rust の内部表現へ変換する。

期待責務:

- `registry` の検証
- `SkillDefinition` の整合性検証
- 未知の `group/type` を reject
- `implementationKind` ごとの実行方式を確定

### 7.2 実行器の分離

エンジン側は以下の 3 種類に分離する。

- `primitive executor`
  - `apply_status`
  - `forced_move`
  - `summon_piece`
  - `transform_piece`
  - `board_hazard`
  - `extra_action`
  - など
- `composite executor`
  - 複数 `EffectNode` を順序通りに適用
- `script hook executor`
  - 固有処理名で専用ロジックへ dispatch

### 7.3 実行フェーズ

AI で必要なフェーズは最低限以下。

1. trigger 判定
2. condition 判定
3. target selection
4. effect apply
5. 派生局面生成

### 7.4 script hook の扱い

`script_hook` があること自体は問題ではない。

問題になるのは以下です。

- `primitive` と見せかけて固有処理が混ざる
- 同じ `effect_type` なのに挙動が駒ごとに大きく異なる
- 必要条件が自然文にしか存在しない

したがって、`script_hook` は隠さず露出させる。

## 8. テスト期待

テストはスキル単位ではなく、まず実行器単位で切る。

### 8.1 registry テスト

- `group_code` と `option_code` の重複禁止
- `schema_kind` ごとの所属整合性
- UI 表示順の安定性

### 8.2 definition テスト

- `implementationKind = primitive` なのに `scriptHook != null` は禁止
- `effect_group/effect_type` の組み合わせ検証
- `target_group/selector` の組み合わせ検証

### 8.3 executor テスト

- `apply_status` の対象・持続
- `forced_move` の押し出し方向
- `summon_piece` の空きマス選択
- `transform_piece` の対象更新

### 8.4 integration テスト

- `trigger -> condition -> effect` の一連の適用
- `composite` の順序保証
- `script_hook` の dispatch

## 9. 実装順序

1. `registry` を固定する
2. `m_skill` / `m_skill_effect` / `m_skill_condition` の形を固める
3. `primitive` スキルだけを v2 構造へ移す
4. `script_hook` スキルを明示化する
5. Rust 側の executor とテストを書く

## 10. 参照

- `docs/ai-engine-design-policy.md`
- `docs/ai-algorithm-overview-ja.md`
- `../../SHOGI_GAME/deck_builder.html`
- `../../SHOGI_GAME/piece_info.html`
- `../../SHOGI_GAME/online_battle.html`
