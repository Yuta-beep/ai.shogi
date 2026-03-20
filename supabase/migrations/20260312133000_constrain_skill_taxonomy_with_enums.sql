begin;

-- Freeze current skill taxonomy into DB enums so new values require explicit migration.

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM pg_type t
    JOIN pg_namespace n ON n.oid = t.typnamespace
    WHERE t.typname = 'skill_type_enum'
      AND n.nspname = 'master'
  ) THEN
    CREATE TYPE master.skill_type_enum AS ENUM (
      'active_or_passive',
      'active_or_triggered',
      'passive'
    );
  END IF;
END
$$;

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM pg_type t
    JOIN pg_namespace n ON n.oid = t.typnamespace
    WHERE t.typname = 'skill_target_rule_enum'
      AND n.nspname = 'master'
  ) THEN
    CREATE TYPE master.skill_target_rule_enum AS ENUM (
      'adjacent_8',
      'adjacent_area',
      'ally_piece',
      'board_cell',
      'enemy_hand',
      'enemy_piece',
      'front_enemy',
      'hand_piece',
      'left_right',
      'same_row_or_col',
      'self',
      'unspecified'
    );
  END IF;
END
$$;

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM pg_type t
    JOIN pg_namespace n ON n.oid = t.typnamespace
    WHERE t.typname = 'skill_trigger_timing_enum'
      AND n.nspname = 'master'
  ) THEN
    CREATE TYPE master.skill_trigger_timing_enum AS ENUM (
      'after_move',
      'on_capture',
      'on_capture_attempt',
      'on_captured',
      'on_condition_met',
      'on_move',
      'on_other_piece_move',
      'on_turn_start',
      'passive'
    );
  END IF;
END
$$;

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM pg_type t
    JOIN pg_namespace n ON n.oid = t.typnamespace
    WHERE t.typname = 'skill_effect_type_enum'
      AND n.nspname = 'master'
  ) THEN
    CREATE TYPE master.skill_effect_type_enum AS ENUM (
      'apply_status',
      'board_hazard',
      'buff',
      'capture_constraint',
      'capture_with_leap',
      'copy_ability',
      'custom',
      'defense_or_immunity',
      'destroy_hand_piece',
      'disable_piece',
      'extra_action',
      'forced_move',
      'gain_piece',
      'inherit_ability',
      'linked_action',
      'modify_movement',
      'multi_capture',
      'reflective_movement',
      'remove_piece',
      'return_to_hand',
      'revive',
      'seal_skill',
      'send_to_hand',
      'substitute',
      'summon_piece',
      'transform_piece'
    );
  END IF;
END
$$;

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM pg_type t
    JOIN pg_namespace n ON n.oid = t.typnamespace
    WHERE t.typname = 'skill_effect_summary_type_enum'
      AND n.nspname = 'master'
  ) THEN
    CREATE TYPE master.skill_effect_summary_type_enum AS ENUM (
      'apply_status',
      'board_hazard',
      'capture_constraint',
      'capture_with_leap',
      'composite',
      'copy_ability',
      'defense_or_immunity',
      'destroy_hand_piece',
      'disable_piece',
      'extra_action',
      'forced_move',
      'gain_piece',
      'inherit_ability',
      'linked_action',
      'modify_movement',
      'multi_capture',
      'reflective_movement',
      'remove_piece',
      'return_to_hand',
      'revive',
      'scripted',
      'seal_skill',
      'send_to_hand',
      'substitute',
      'summon_piece',
      'transform_piece'
    );
  END IF;
END
$$;

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM pg_type t
    JOIN pg_namespace n ON n.oid = t.typnamespace
    WHERE t.typname = 'skill_parse_status_enum'
      AND n.nspname = 'master'
  ) THEN
    CREATE TYPE master.skill_parse_status_enum AS ENUM (
      'manual',
      'manual_admin',
      'rule_only_v2'
    );
  END IF;
END
$$;

ALTER TABLE master.m_skill
  ALTER COLUMN skill_type DROP DEFAULT,
  ALTER COLUMN target_rule DROP DEFAULT,
  ALTER COLUMN effect_summary_type DROP DEFAULT,
  ALTER COLUMN parse_status DROP DEFAULT;

ALTER TABLE master.m_skill
  ALTER COLUMN skill_type TYPE master.skill_type_enum
    USING skill_type::master.skill_type_enum,
  ALTER COLUMN target_rule TYPE master.skill_target_rule_enum
    USING target_rule::master.skill_target_rule_enum,
  ALTER COLUMN effect_summary_type TYPE master.skill_effect_summary_type_enum
    USING effect_summary_type::master.skill_effect_summary_type_enum,
  ALTER COLUMN parse_status TYPE master.skill_parse_status_enum
    USING parse_status::master.skill_parse_status_enum,
  ALTER COLUMN trigger_timing TYPE master.skill_trigger_timing_enum
    USING trigger_timing::master.skill_trigger_timing_enum;

ALTER TABLE master.m_skill
  ALTER COLUMN skill_type SET DEFAULT 'active_or_passive'::master.skill_type_enum,
  ALTER COLUMN target_rule SET DEFAULT 'unspecified'::master.skill_target_rule_enum,
  ALTER COLUMN effect_summary_type SET DEFAULT 'scripted'::master.skill_effect_summary_type_enum,
  ALTER COLUMN parse_status SET DEFAULT 'rule_only_v2'::master.skill_parse_status_enum;

ALTER TABLE master.m_skill_effect
  ALTER COLUMN target_rule DROP DEFAULT;

ALTER TABLE master.m_skill_effect
  ALTER COLUMN effect_type TYPE master.skill_effect_type_enum
    USING effect_type::master.skill_effect_type_enum,
  ALTER COLUMN target_rule TYPE master.skill_target_rule_enum
    USING target_rule::master.skill_target_rule_enum,
  ALTER COLUMN trigger_timing TYPE master.skill_trigger_timing_enum
    USING trigger_timing::master.skill_trigger_timing_enum;

ALTER TABLE master.m_skill_effect
  ALTER COLUMN target_rule SET DEFAULT 'unspecified'::master.skill_target_rule_enum;

commit;
