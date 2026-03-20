use super::config::{build_engine_config, EngineConfig, EngineConfigPatch};
use super::heuristic::is_board_coordinate_valid;
use super::rules::parse_runtime_rules;
use super::search::{apply_move, evaluate_state, generate_legal_moves, is_in_check};
use super::skill_executor::{score_move_with_skill_effects, simulate_move_with_skills};
use super::skills::{
    builtin_skill_registry, parse_skill_definition_document_value, validate_skill_definitions,
    SkillDefinition,
};
use super::types::{piece_code, EngineMove, GenMove, PieceKind, RuntimeRules, SearchState, Side};
use super::util::{make_seed, select_move_index};
use rand::rngs::StdRng;
use rand::SeedableRng;

fn sample_move() -> EngineMove {
    EngineMove {
        from_row: Some(6),
        from_col: Some(4),
        to_row: 5,
        to_col: 4,
        piece_code: "FU".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: Some("7f7e".to_string()),
    }
}

fn load_sample_skill_definitions() -> Vec<SkillDefinition> {
    let doc: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/docs/skill-definition-v2-catalog.json"
    )))
    .expect("sample skill definitions must be valid json");
    parse_skill_definition_document_value(doc)
        .expect("definitions must parse")
        .definitions
}

fn sample_skill_by_id(skill_id: u64) -> SkillDefinition {
    load_sample_skill_definitions()
        .into_iter()
        .find(|definition| definition.skill_id == skill_id)
        .unwrap_or_else(|| panic!("sample skill {skill_id} must exist"))
}

fn sample_runtime_rules() -> RuntimeRules {
    let registry: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/docs/skill-registry-v2-draft.json"
    )))
    .expect("registry json must parse");
    let definitions: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/docs/skill-definition-v2-catalog.json"
    )))
    .expect("sample definition json must parse");
    let board_state = serde_json::json!({
        "skill_registry_v2": registry,
        "skill_definitions_v2": definitions
    });
    parse_runtime_rules(&board_state).expect("runtime rules must parse")
}

#[test]
fn engine_config_defaults_are_applied() {
    let cfg =
        build_engine_config(EngineConfigPatch::default()).expect("default config should be valid");
    assert_eq!(cfg.max_depth, 3);
    assert_eq!(cfg.max_nodes, 20_000);
    assert_eq!(cfg.time_limit_ms, 300);
    assert!(cfg.always_legal_move);
    assert!(cfg.mate_avoidance);
}

#[test]
fn engine_config_rejects_invalid_values() {
    let cfg = EngineConfigPatch {
        max_depth: Some(0),
        ..EngineConfigPatch::default()
    };
    assert!(build_engine_config(cfg).is_err());
}

#[test]
fn engine_config_rejects_false_safety_flags() {
    let cfg = EngineConfigPatch {
        always_legal_move: Some(false),
        ..EngineConfigPatch::default()
    };
    assert!(build_engine_config(cfg).is_err());
}

#[test]
fn coordinate_validation_accepts_normal_move() {
    assert!(is_board_coordinate_valid(&sample_move()));
}

#[test]
fn coordinate_validation_rejects_inconsistent_drop() {
    let mut mv = sample_move();
    mv.drop_piece_code = Some("FU".to_string());
    assert!(!is_board_coordinate_valid(&mv));
}

#[test]
fn select_move_is_deterministic_when_no_randomness() {
    let cfg = EngineConfig {
        random_topk: 3,
        blunder_rate: 0.0,
        temperature: 0.0,
        ..EngineConfig::default()
    };
    let scored = vec![(2, 100), (1, 90), (0, 80)];
    let mut rng = StdRng::seed_from_u64(42);
    let idx = select_move_index(&scored, 100, &cfg, &mut rng);
    assert_eq!(idx, 2);
}

#[test]
fn seed_is_stable_for_same_input() {
    let a = make_seed("game-1", 12);
    let b = make_seed("game-1", 12);
    assert_eq!(a, b);
}

#[test]
fn parse_sfen_basic_position() {
    let sfen = "4k4/9/9/9/9/9/9/9/4K4 b - 1";
    let state = SearchState::from_sfen(sfen).expect("must parse");
    let kings = state
        .board
        .iter()
        .filter(|p| p.map(|x| x.kind == PieceKind::King).unwrap_or(false))
        .count();
    assert_eq!(kings, 2);
}

#[test]
fn generate_moves_from_sfen_produces_candidates() {
    let sfen = "4k4/9/9/9/9/9/9/4P4/4K4 b - 1";
    let state = SearchState::from_sfen(sfen).expect("must parse");
    let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);
    assert!(!moves.is_empty());
}

#[test]
fn nifu_drop_is_filtered() {
    let sfen = "4k4/9/9/9/9/9/4P4/9/4K4 b P 1";
    let state = SearchState::from_sfen(sfen).expect("must parse");
    let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);
    let has_same_file_pawn_drop = moves
        .iter()
        .any(|m| m.drop == Some(PieceKind::Pawn) && m.to.1 == 4);
    assert!(!has_same_file_pawn_drop);
}

#[test]
fn legal_moves_filter_out_self_check() {
    let sfen = "4k4/9/9/9/9/9/9/4r4/4K4 b - 1";
    let state = SearchState::from_sfen(sfen).expect("must parse");
    let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);
    assert!(moves.iter().all(|m| {
        let next = apply_move(&state, m);
        !is_in_check(&next, Side::Black, &RuntimeRules::default())
    }));
}

#[test]
fn custom_vectors_are_loaded() {
    let v: serde_json::Value = serde_json::json!({
        "custom_move_vectors": {
            "FU": [
                { "dr": -1, "dc": -1, "slide": false }
            ]
        },
        "eval_bonus_by_piece": {
            "FU": 10
        }
    });
    let rules = parse_runtime_rules(&v).expect("runtime rules must parse");
    assert_eq!(
        rules.extra_vectors_by_piece.get("FU").map(|x| x.len()),
        Some(1)
    );
    assert_eq!(rules.eval_bonus_by_piece.get("FU").copied(), Some(10));
}

#[test]
fn persisted_skill_state_hydrates_piece_statuses() {
    let mut state = SearchState::from_sfen("4k4/9/9/9/4P4/9/9/9/4K4 b - 1").expect("must parse");
    let board_state = serde_json::json!({
        "skill_state": {
            "piece_statuses": [
                {
                    "row": 4,
                    "col": 4,
                    "side": "player",
                    "status_type": "freeze",
                    "remaining_turns": 2
                }
            ]
        }
    });

    state.hydrate_skill_state_from_board_state(&board_state);

    assert!(state.has_piece_status(4, 4, Side::Black, "freeze"));
}

#[test]
fn hydrated_board_hazard_blocks_legal_move_generation() {
    let mut state = SearchState::from_sfen("4k4/9/9/9/9/9/4P4/9/4K4 b - 1").expect("must parse");
    let board_state = serde_json::json!({
        "skill_state": {
            "board_hazards": [
                {
                    "row": 5,
                    "col": 4,
                    "affects_side": "player",
                    "hazard_type": "poison_pool",
                    "remaining_turns": 2
                }
            ]
        }
    });

    state.hydrate_skill_state_from_board_state(&board_state);
    let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);

    assert!(!moves
        .iter()
        .any(|mv| mv.from == Some((6, 4)) && mv.to == (5, 4)));
}

#[test]
fn hydrated_movement_modifier_changes_legal_move_shape() {
    let mut state = SearchState::from_sfen("4k4/9/9/9/4B4/9/9/9/4K4 b - 1").expect("must parse");
    let board_state = serde_json::json!({
        "skill_state": {
            "movement_modifiers": [
                {
                    "row": 4,
                    "col": 4,
                    "side": "player",
                    "movement_rule": "orthogonal_step_only",
                    "remaining_turns": 2
                }
            ]
        }
    });

    state.hydrate_skill_state_from_board_state(&board_state);
    let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);

    assert!(moves
        .iter()
        .any(|mv| mv.from == Some((4, 4)) && mv.to == (3, 4)));
    assert!(!moves
        .iter()
        .any(|mv| mv.from == Some((4, 4)) && mv.to == (3, 3)));
}

#[test]
fn hydrated_piece_defense_blocks_capture() {
    let mut state = SearchState::from_sfen("4k4/9/9/9/3rP4/9/9/9/4K4 w - 1").expect("must parse");
    let board_state = serde_json::json!({
        "skill_state": {
            "piece_defenses": [
                {
                    "row": 4,
                    "col": 4,
                    "side": "player",
                    "mode": "immune_to_capture",
                    "remaining_turns": 2
                }
            ]
        }
    });

    state.hydrate_skill_state_from_board_state(&board_state);
    let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);

    assert!(!moves
        .iter()
        .any(|mv| mv.from == Some((4, 3)) && mv.to == (4, 4)));
}

#[test]
fn builtin_skill_registry_is_valid() {
    let registry = builtin_skill_registry();
    assert_eq!(registry.version, "skill-registry-v2-draft");
    assert!(registry
        .implementation_kinds
        .iter()
        .any(|kind| kind.code == "primitive"));
}

#[test]
fn sample_skill_definitions_validate_against_builtin_registry() {
    let doc: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/docs/skill-definition-v2-catalog.json"
    )))
    .expect("sample skill definitions must be valid json");
    let parsed = parse_skill_definition_document_value(doc).expect("definitions must parse");
    validate_skill_definitions(builtin_skill_registry(), &parsed.definitions)
        .expect("sample definitions must validate");
    assert_eq!(parsed.definitions.len(), 98);
}

#[test]
fn runtime_rules_load_skill_registry_and_definitions_v2() {
    let rules = sample_runtime_rules();
    assert!(rules.skill_runtime.registry.is_some());
    assert_eq!(rules.skill_runtime.definitions.len(), 98);
    assert_eq!(
        rules.skill_runtime.definitions[0]
            .classification
            .implementation_kind,
        "primitive"
    );
}

#[test]
fn runtime_rules_reject_invalid_skill_definition() {
    let board_state = serde_json::json!({
        "skill_definitions_v2": [
            {
                "skillId": 999,
                "pieceChars": ["仮"],
                "source": {
                    "skillText": "invalid",
                    "sourceKind": "manual",
                    "sourceFile": "manual",
                    "sourceFunction": "manual"
                },
                "classification": {
                    "implementationKind": "primitive",
                    "tags": []
                },
                "trigger": {
                    "group": "event_move",
                    "type": "not_exists"
                },
                "conditions": [],
                "effects": [
                    {
                        "order": 1,
                        "group": "piece_position",
                        "type": "forced_move",
                        "target": {
                            "group": "adjacent",
                            "selector": "adjacent_enemy"
                        },
                        "params": {}
                    }
                ],
                "scriptHook": null
            }
        ]
    });

    let err = parse_runtime_rules(&board_state).expect_err("invalid definition must be rejected");
    assert!(err.contains("unknown trigger option"));
}

#[test]
fn waterfall_sample_is_a_primitive_forced_move_skill() {
    let definition = sample_skill_by_id(65);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);

    let effect = &definition.effects[0];
    assert_eq!(effect.group, "piece_position");
    assert_eq!(effect.type_code, "forced_move");
    assert_eq!(effect.target.group, "adjacent");
    assert_eq!(effect.target.selector, "adjacent_enemy");
    assert_eq!(effect.params["movementRule"], "push_away");
}

#[test]
fn rainbow_sample_is_a_composite_with_stable_effect_order() {
    let definition = sample_skill_by_id(26);
    assert_eq!(definition.classification.implementation_kind, "composite");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 2);

    assert_eq!(definition.effects[0].order, 1);
    assert_eq!(definition.effects[0].target.selector, "self_piece");
    assert_eq!(definition.effects[1].order, 2);
    assert_eq!(definition.effects[1].target.selector, "adjacent_enemy");
    assert_eq!(definition.effects[1].params["durationTurns"], 1);
}

#[test]
fn reflection_sample_is_a_script_hook_without_common_effects() {
    let definition = sample_skill_by_id(9);
    assert_eq!(definition.classification.implementation_kind, "script_hook");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert!(definition.conditions.is_empty());
    assert!(definition.effects.is_empty());
    assert_eq!(
        definition.script_hook.as_deref(),
        Some("reflect_until_blocked")
    );
}

#[test]
fn batch_j_script_hook_samples_use_named_hooks_and_empty_effects() {
    for (skill_id, hook, condition_len) in [
        (97_u64, "bomb_explosion_push", 0_usize),
        (99_u64, "safe_room_king_relocation", 1_usize),
        (100_u64, "fixed_next_turn_restriction", 0_usize),
        (103_u64, "edge_line_imprison", 0_usize),
        (106_u64, "escape_king_follow", 0_usize),
    ] {
        let definition = sample_skill_by_id(skill_id);
        assert_eq!(definition.classification.implementation_kind, "script_hook");
        assert_eq!(definition.trigger.group, "event_move");
        assert_eq!(definition.trigger.type_code, "after_move");
        assert_eq!(definition.conditions.len(), condition_len);
        assert!(definition.effects.is_empty());
        assert_eq!(definition.script_hook.as_deref(), Some(hook));
    }
    assert_eq!(
        sample_skill_by_id(99).conditions[0].type_code,
        "chance_roll"
    );
}

#[test]
fn darkness_sample_is_a_primitive_apply_status_skill() {
    let definition = sample_skill_by_id(11);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);

    let effect = &definition.effects[0];
    assert_eq!(effect.group, "piece_state");
    assert_eq!(effect.type_code, "apply_status");
    assert_eq!(effect.target.group, "adjacent");
    assert_eq!(effect.target.selector, "adjacent_enemy");
    assert_eq!(effect.params["statusType"], "dark_blind");
    assert_eq!(effect.params["durationTurns"], 1);
}

#[test]
fn moss_sample_is_a_primitive_summon_piece_skill() {
    let definition = sample_skill_by_id(23);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);

    let effect = &definition.effects[0];
    assert_eq!(effect.group, "piece_generation");
    assert_eq!(effect.type_code, "summon_piece");
    assert_eq!(effect.target.group, "adjacent");
    assert_eq!(effect.target.selector, "adjacent_empty");
    assert_eq!(effect.params["summonPiece"], "self_clone");
}

#[test]
fn lantern_sample_is_a_primitive_transform_piece_skill() {
    let definition = sample_skill_by_id(93);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);

    let effect = &definition.effects[0];
    assert_eq!(effect.group, "piece_generation");
    assert_eq!(effect.type_code, "transform_piece");
    assert_eq!(effect.target.group, "global");
    assert_eq!(effect.target.selector, "all_ally");
    assert_eq!(effect.params["fromPieceCode"], "FU");
    assert_eq!(effect.params["toPieceChar"], "火");
}

#[test]
fn tin_sample_is_a_probability_gated_apply_status_skill() {
    let definition = sample_skill_by_id(14);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);

    let effect = &definition.effects[0];
    assert_eq!(effect.group, "piece_state");
    assert_eq!(effect.type_code, "apply_status");
    assert_eq!(effect.target.selector, "adjacent_enemy");
    assert_eq!(effect.params["statusType"], "stun");
}

#[test]
fn bulls_sample_is_a_probability_gated_self_clone_summon() {
    let definition = sample_skill_by_id(58);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);

    let effect = &definition.effects[0];
    assert_eq!(effect.group, "piece_generation");
    assert_eq!(effect.type_code, "summon_piece");
    assert_eq!(effect.target.selector, "adjacent_empty");
    assert_eq!(effect.params["summonPiece"], "self_clone");
}

#[test]
fn a_sample_transforms_adjacent_enemy_into_a_pawn() {
    let definition = sample_skill_by_id(30);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);

    let effect = &definition.effects[0];
    assert_eq!(effect.group, "piece_generation");
    assert_eq!(effect.type_code, "transform_piece");
    assert_eq!(effect.target.selector, "adjacent_enemy");
    assert_eq!(effect.params["toPieceCode"], "FU");
}

#[test]
fn water_sample_is_a_continuous_forced_move_skill() {
    let definition = sample_skill_by_id(5);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);

    let effect = &definition.effects[0];
    assert_eq!(effect.type_code, "forced_move");
    assert_eq!(effect.target.selector, "adjacent_enemy");
    assert_eq!(effect.params["movementRule"], "push_away");
}

#[test]
fn wave_sample_is_a_continuous_forced_move_skill() {
    let definition = sample_skill_by_id(6);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.effects[0].type_code, "forced_move");
}

#[test]
fn wind_sample_is_a_continuous_forced_move_skill() {
    let definition = sample_skill_by_id(22);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.effects[0].type_code, "forced_move");
}

#[test]
fn time_sample_is_a_continuous_apply_status_skill() {
    let definition = sample_skill_by_id(18);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "apply_status");
    assert_eq!(definition.effects[0].params["statusType"], "time_stop");
    assert_eq!(definition.effects[0].params["durationTurns"], 2);
}

#[test]
fn ice_sample_is_a_continuous_apply_status_skill() {
    let definition = sample_skill_by_id(19);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.effects[0].type_code, "apply_status");
    assert_eq!(definition.effects[0].params["statusType"], "freeze");
    assert_eq!(definition.effects[0].params["durationTurns"], 2);
}

#[test]
fn heart_sample_is_an_after_move_apply_status_skill() {
    let definition = sample_skill_by_id(74);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.effects[0].type_code, "apply_status");
    assert_eq!(definition.effects[0].params["statusType"], "stun");
    assert_eq!(definition.effects[0].params["durationTurns"], 1);
}

#[test]
fn fish_sample_is_a_probability_gated_apply_status_skill() {
    let definition = sample_skill_by_id(24);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "apply_status");
    assert_eq!(definition.effects[0].params["statusType"], "drown");
    assert_eq!(definition.effects[0].params["durationTurns"], 1);
}

#[test]
fn prison_sample_is_an_after_move_apply_status_skill() {
    let definition = sample_skill_by_id(31);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "apply_status");
    assert_eq!(definition.effects[0].params["statusType"], "stun");
    assert_eq!(definition.effects[0].params["durationTurns"], 2);
}

#[test]
fn beast_sample_is_an_after_move_apply_status_skill() {
    let definition = sample_skill_by_id(72);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "apply_status");
    assert_eq!(definition.effects[0].params["statusType"], "stun");
    assert_eq!(definition.effects[0].params["durationTurns"], 2);
}

#[test]
fn flame_sample_is_a_probability_gated_remove_piece_skill() {
    let definition = sample_skill_by_id(3);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "remove_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn demon_sample_is_a_probability_gated_remove_piece_skill() {
    let definition = sample_skill_by_id(12);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "remove_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn awakened_dragon_sample_is_a_probability_gated_remove_piece_skill() {
    let definition = sample_skill_by_id(48);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "remove_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn second_sample_is_an_after_capture_extra_action_skill() {
    let definition = sample_skill_by_id(76);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].group, "action_economy");
    assert_eq!(definition.effects[0].type_code, "extra_action");
}

#[test]
fn convex_sample_is_an_after_move_extra_action_skill() {
    let definition = sample_skill_by_id(81);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].group, "action_economy");
    assert_eq!(definition.effects[0].type_code, "extra_action");
}

#[test]
fn stir_fry_sample_is_an_after_capture_extra_action_skill() {
    let definition = sample_skill_by_id(83);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].group, "action_economy");
    assert_eq!(definition.effects[0].type_code, "extra_action");
}

#[test]
fn iron_sample_is_a_continuous_forced_move_skill() {
    let definition = sample_skill_by_id(13);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "forced_move");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
    assert_eq!(definition.effects[0].params["movementRule"], "push_away");
}

#[test]
fn roar_sample_is_an_after_move_forced_move_skill() {
    let definition = sample_skill_by_id(57);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "forced_move");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
    assert_eq!(definition.effects[0].params["movementRule"], "push_away");
}

#[test]
fn oni_sample_is_an_after_move_forced_move_skill() {
    let definition = sample_skill_by_id(68);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "forced_move");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
    assert_eq!(definition.effects[0].params["movementRule"], "push_away");
}

#[test]
fn ore_sample_is_a_probability_gated_ally_transform_skill() {
    let definition = sample_skill_by_id(35);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "transform_piece");
    assert_eq!(definition.effects[0].target.selector, "all_ally");
    assert_eq!(definition.effects[0].params["fromPieceCode"], "FU");
    assert_eq!(definition.effects[0].params["toPieceChar"], "金");
}

#[test]
fn experiment_sample_is_an_adjacent_enemy_transform_skill() {
    let definition = sample_skill_by_id(50);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "transform_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
    assert_eq!(definition.effects[0].params["toPieceChar"], "異");
}

#[test]
fn coin_sample_is_a_probability_gated_self_transform_skill() {
    let definition = sample_skill_by_id(89);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "transform_piece");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
    assert_eq!(definition.effects[0].params["toPieceChar"], "金");
}

#[test]
fn tree_sample_is_a_continuous_summon_piece_skill() {
    let definition = sample_skill_by_id(7);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "summon_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_empty");
    assert_eq!(definition.effects[0].params["summonPieceChar"], "木");
}

#[test]
fn ridge_sample_is_a_probability_gated_summon_piece_skill() {
    let definition = sample_skill_by_id(32);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "summon_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_empty");
    assert_eq!(definition.effects[0].params["summonPieceChar"], "山");
}

#[test]
fn grave_sample_is_a_probability_gated_summon_piece_skill() {
    let definition = sample_skill_by_id(36);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "summon_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_empty");
    assert_eq!(definition.effects[0].params["summonPieceChar"], "霊");
}

#[test]
fn rock_sample_is_an_after_move_summon_piece_skill() {
    let definition = sample_skill_by_id(34);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "summon_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_empty");
    assert_eq!(definition.effects[0].params["summonPieceChar"], "岩");
}

#[test]
fn roast_sample_is_an_after_capture_summon_piece_skill() {
    let definition = sample_skill_by_id(82);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "summon_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_empty");
    assert_eq!(definition.effects[0].params["summonPieceChar"], "炎");
}

#[test]
fn boil_sample_is_an_after_capture_summon_piece_skill() {
    let definition = sample_skill_by_id(84);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "summon_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_empty");
    assert_eq!(definition.effects[0].params["summonPieceChar"], "火");
}

#[test]
fn swamp_sample_is_a_continuous_adjacent_enemy_modify_movement_skill() {
    let definition = sample_skill_by_id(28);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "modify_movement");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
    assert_eq!(
        definition.effects[0].params["movementRule"],
        "vertical_step_only"
    );
}

#[test]
fn cherry_sample_is_an_after_move_same_row_modify_movement_skill() {
    let definition = sample_skill_by_id(79);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "modify_movement");
    assert_eq!(definition.effects[0].target.selector, "same_row_ally");
    assert_eq!(
        definition.effects[0].params["movementRule"],
        "extend_range_by_one"
    );
}

#[test]
fn concave_sample_is_a_continuous_self_modify_movement_skill() {
    let definition = sample_skill_by_id(80);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "modify_movement");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
    assert_eq!(
        definition.effects[0].params["movementRule"],
        "penetrate_to_edge"
    );
}

#[test]
fn dragon_sample_is_a_continuous_self_transform_skill() {
    let definition = sample_skill_by_id(2);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "transform_piece");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
    assert_eq!(definition.effects[0].params["toPieceChar"], "辰");
}

#[test]
fn fire_sample_is_an_after_move_destroy_hand_piece_skill() {
    let definition = sample_skill_by_id(4);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "destroy_hand_piece");
    assert_eq!(definition.effects[0].target.selector, "enemy_hand_random");
}

#[test]
fn treasure_sample_is_a_probability_gated_gain_piece_skill() {
    let definition = sample_skill_by_id(15);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "gain_piece");
    assert_eq!(definition.effects[0].target.selector, "ally_hand_piece");
    assert_eq!(definition.effects[0].params["gainPieceCode"], "KI");
}

#[test]
fn electric_sample_is_a_probability_gated_apply_status_skill() {
    let definition = sample_skill_by_id(16);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "apply_status");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
    assert_eq!(definition.effects[0].params["statusType"], "shock");
}

#[test]
fn thunder_sample_is_a_probability_gated_hand_remove_skill() {
    let definition = sample_skill_by_id(17);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "remove_piece");
    assert_eq!(definition.effects[0].target.selector, "enemy_hand_random");
}

#[test]
fn house_sample_is_a_continuous_summon_piece_skill() {
    let definition = sample_skill_by_id(44);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "summon_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_empty");
    assert_eq!(definition.effects[0].params["summonPieceChar"], "民");
}

#[test]
fn people_sample_is_a_continuous_self_modify_movement_skill() {
    let definition = sample_skill_by_id(45);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "modify_movement");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
    assert_eq!(
        definition.effects[0].params["movementRule"],
        "diagonal_step_only"
    );
}

#[test]
fn field_sample_is_a_continuous_self_modify_movement_skill() {
    let definition = sample_skill_by_id(46);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "modify_movement");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
    assert_eq!(
        definition.effects[0].params["movementRule"],
        "diagonal_step_only"
    );
}

#[test]
fn medicine_sample_is_a_continuous_adjacent_ally_modify_movement_skill() {
    let definition = sample_skill_by_id(64);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "modify_movement");
    assert_eq!(definition.effects[0].target.selector, "adjacent_ally");
    assert_eq!(
        definition.effects[0].params["movementRule"],
        "extend_range_by_one"
    );
}

#[test]
fn depression_sample_is_an_after_move_origin_cell_apply_status_skill() {
    let definition = sample_skill_by_id(75);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "apply_status");
    assert_eq!(definition.effects[0].target.selector, "origin_cell");
    assert_eq!(definition.effects[0].params["statusType"], "blocked_cell");
}

#[test]
fn sand_sample_is_an_after_move_linked_action_skill() {
    let definition = sample_skill_by_id(21);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "linked_action");
    assert_eq!(definition.effects[0].target.selector, "same_row_ally");
}

#[test]
fn poison_sample_is_an_after_move_board_hazard_skill() {
    let definition = sample_skill_by_id(27);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "board_hazard");
    assert_eq!(definition.effects[0].target.selector, "origin_cell");
}

#[test]
fn mirror_sample_is_a_continuous_copy_ability_skill() {
    let definition = sample_skill_by_id(29);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "copy_ability");
    assert_eq!(definition.effects[0].target.selector, "front_enemy");
}

#[test]
fn phantom_sample_is_a_probability_gated_defense_skill() {
    let definition = sample_skill_by_id(38);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn boat_sample_is_an_after_move_linked_action_skill() {
    let definition = sample_skill_by_id(41);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "linked_action");
    assert_eq!(definition.effects[0].target.selector, "adjacent_ally");
}

#[test]
fn machine_sample_is_a_continuous_copy_ability_skill() {
    let definition = sample_skill_by_id(42);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "copy_ability");
    assert_eq!(definition.effects[0].target.selector, "adjacent_ally");
}

#[test]
fn armor_sample_is_a_continuous_defense_skill() {
    let definition = sample_skill_by_id(53);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn holy_sword_sample_is_a_continuous_defense_skill() {
    let definition = sample_skill_by_id(61);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn rose_sample_is_a_continuous_board_hazard_skill() {
    let definition = sample_skill_by_id(77);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "board_hazard");
    assert_eq!(definition.effects[0].target.selector, "adjacent_empty");
}

#[test]
fn chrysanthemum_sample_is_a_continuous_revive_skill() {
    let definition = sample_skill_by_id(78);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "revive");
    assert_eq!(definition.effects[0].target.selector, "adjacent_ally");
}

#[test]
fn cloud_sample_is_a_continuous_capture_constraint_skill() {
    let definition = sample_skill_by_id(25);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "capture_constraint");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn peak_sample_is_a_continuous_disable_piece_skill() {
    let definition = sample_skill_by_id(33);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "disable_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn spirit_sample_is_a_continuous_capture_constraint_skill() {
    let definition = sample_skill_by_id(37);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "capture_constraint");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn mist_sample_is_an_after_move_send_to_hand_skill() {
    let definition = sample_skill_by_id(39);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "send_to_hand");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn gear_sample_is_an_after_move_linked_action_skill() {
    let definition = sample_skill_by_id(43);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "linked_action");
    assert_eq!(definition.effects[0].target.selector, "adjacent_ally");
}

#[test]
fn saint_sample_is_a_continuous_disable_piece_skill() {
    let definition = sample_skill_by_id(60);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "disable_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn shield_sample_is_a_probability_gated_disable_piece_skill() {
    let definition = sample_skill_by_id(62);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "disable_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_ally");
}

#[test]
fn hole_sample_is_an_after_capture_board_hazard_skill() {
    let definition = sample_skill_by_id(66);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "board_hazard");
    assert_eq!(definition.effects[0].target.selector, "adjacent_empty");
}

#[test]
fn oboro_sample_is_a_continuous_defense_skill() {
    let definition = sample_skill_by_id(69);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn bird_sample_is_an_after_move_adjacent_ally_defense_skill() {
    let definition = sample_skill_by_id(73);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[0].target.selector, "adjacent_ally");
}

#[test]
fn death_sample_is_a_continuous_adjacent_enemy_remove_piece_skill() {
    let definition = sample_skill_by_id(70);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "remove_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn soul_sample_is_a_continuous_defense_skill() {
    let definition = sample_skill_by_id(71);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn yin_sample_is_a_composite_defense_skill() {
    let definition = sample_skill_by_id(86);
    assert_eq!(definition.classification.implementation_kind, "composite");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 2);
    assert_eq!(definition.effects[0].type_code, "seal_skill");
    assert_eq!(definition.effects[1].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[1].target.selector, "self_piece");
}

#[test]
fn cow_sample_is_a_composite_backward_step_modify_movement_skill() {
    let definition = sample_skill_by_id(87);
    assert_eq!(definition.classification.implementation_kind, "composite");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 2);
    assert_eq!(definition.effects[0].type_code, "modify_movement");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
    assert_eq!(
        definition.effects[0].params["movementRule"],
        "backward_step_only"
    );
    assert_eq!(definition.effects[1].type_code, "multi_capture");
}

#[test]
fn wealth_sample_is_an_after_capture_adjacent_enemy_transform_skill() {
    let definition = sample_skill_by_id(90);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "transform_piece");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn giant_sample_is_a_composite_multi_defense_skill() {
    let definition = sample_skill_by_id(91);
    assert_eq!(definition.classification.implementation_kind, "composite");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 3);
    assert_eq!(definition.effects[0].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[1].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[2].type_code, "multi_capture");
}

#[test]
fn snowman_sample_is_a_probability_gated_gain_piece_skill() {
    let definition = sample_skill_by_id(20);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_move");
    assert_eq!(definition.trigger.type_code, "after_move");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "gain_piece");
    assert_eq!(definition.effects[0].target.selector, "ally_hand_piece");
}

#[test]
fn mutant_sample_is_a_continuous_self_transform_skill() {
    let definition = sample_skill_by_id(51);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "transform_piece");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn disease_sample_is_an_after_capture_apply_status_skill() {
    let definition = sample_skill_by_id(63);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "apply_status");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn abyss_sample_is_an_after_capture_apply_status_skill() {
    let definition = sample_skill_by_id(67);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "apply_status");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn moon_sample_is_a_turn_start_self_modify_movement_skill() {
    let definition = sample_skill_by_id(40);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_turn");
    assert_eq!(definition.trigger.type_code, "turn_start");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "modify_movement");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn book_sample_is_a_turn_start_self_copy_ability_skill() {
    let definition = sample_skill_by_id(55);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_turn");
    assert_eq!(definition.trigger.type_code, "turn_start");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "copy_ability");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn blade_sample_is_an_after_capture_multi_capture_skill() {
    let definition = sample_skill_by_id(52);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "multi_capture");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn gun_sample_is_a_continuous_front_enemy_multi_capture_skill() {
    let definition = sample_skill_by_id(54);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "multi_capture");
    assert_eq!(definition.effects[0].target.selector, "front_enemy");
}

#[test]
fn cannon_sample_is_an_after_capture_capture_with_leap_skill() {
    let definition = sample_skill_by_id(1);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "capture_with_leap");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn leaf_sample_is_a_probability_gated_forced_move_skill() {
    let definition = sample_skill_by_id(8);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "forced_move");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn star_sample_is_a_probability_gated_return_to_hand_skill() {
    let definition = sample_skill_by_id(10);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "return_to_hand");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn spring_sample_is_a_composite_adjacent_ally_defense_skill() {
    let definition = sample_skill_by_id(47);
    assert_eq!(definition.classification.implementation_kind, "composite");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 2);
    assert_eq!(definition.effects[0].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[0].target.selector, "adjacent_ally");
    assert_eq!(definition.effects[1].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[1].target.selector, "self_piece");
}

#[test]
fn experiment_k_sample_is_a_probability_gated_composite_summon_and_defense_skill() {
    let definition = sample_skill_by_id(49);
    assert_eq!(definition.classification.implementation_kind, "composite");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 2);
    assert_eq!(definition.effects.len(), 2);
    assert_eq!(definition.effects[0].type_code, "summon_piece");
    assert_eq!(definition.effects[1].type_code, "defense_or_immunity");
}

#[test]
fn seal_sample_is_a_continuous_adjacent_enemy_seal_skill() {
    let definition = sample_skill_by_id(56);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_aura");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "seal_skill");
    assert_eq!(definition.effects[0].target.selector, "adjacent_enemy");
}

#[test]
fn courtesy_sample_is_an_after_capture_substitute_skill() {
    let definition = sample_skill_by_id(59);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "substitute");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn yang_sample_is_a_probability_gated_composite_self_defense_skill() {
    let definition = sample_skill_by_id(85);
    assert_eq!(definition.classification.implementation_kind, "composite");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert_eq!(definition.conditions.len(), 1);
    assert_eq!(definition.effects.len(), 2);
    assert_eq!(definition.effects[0].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
    assert_eq!(definition.effects[1].type_code, "defense_or_immunity");
    assert_eq!(definition.effects[1].target.selector, "adjacent_ally");
}

#[test]
fn pig_sample_is_an_after_capture_inherit_ability_skill() {
    let definition = sample_skill_by_id(88);
    assert_eq!(definition.classification.implementation_kind, "primitive");
    assert_eq!(definition.trigger.group, "event_capture");
    assert_eq!(definition.trigger.type_code, "after_capture");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 1);
    assert_eq!(definition.effects[0].type_code, "inherit_ability");
    assert_eq!(definition.effects[0].target.selector, "self_piece");
}

#[test]
fn nothing_sample_is_a_composite_capture_constraint_and_defense_skill() {
    let definition = sample_skill_by_id(92);
    assert_eq!(definition.classification.implementation_kind, "composite");
    assert_eq!(definition.trigger.group, "continuous");
    assert_eq!(definition.trigger.type_code, "continuous_rule");
    assert!(definition.conditions.is_empty());
    assert_eq!(definition.effects.len(), 3);
    assert_eq!(definition.effects[0].type_code, "modify_movement");
    assert_eq!(definition.effects[1].type_code, "capture_constraint");
    assert_eq!(definition.effects[2].type_code, "defense_or_immunity");
}

#[test]
fn spec_waterfall_forced_move_pushes_adjacent_enemy_away() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1r2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "滝".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&65));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "forced_move"));
    assert!(simulated.expected_value > 0.19 && simulated.expected_value < 0.21);
    assert!(simulated.state.board[4 * 9 + 6].is_none());
    let pushed_piece = simulated.state.board[4 * 9 + 7].expect("enemy must be pushed away");
    assert_eq!(pushed_piece.side, Side::White);
}

#[test]
fn spec_rainbow_composite_applies_effects_in_order() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/5p3/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 5,
        to_col: 4,
        piece_code: "虹".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&26));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec![
            "1:modify_movement:self_piece".to_string(),
            "2:modify_movement:adjacent_enemy".to_string()
        ]
    );
}

#[test]
fn spec_reflection_dispatches_to_named_script_hook() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 7,
        piece_code: "光".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("hook must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&9));
    assert_eq!(
        simulated.trace.applied_hooks,
        vec!["reflect_until_blocked".to_string()]
    );
    let reflected_piece = simulated.state.board[4 * 9 + 6].expect("piece must reflect left");
    assert_eq!(reflected_piece.side, Side::Black);
}

#[test]
fn spec_bomb_dispatches_to_explosion_push_hook() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/5p3/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "爆".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("hook must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&97));
    assert_eq!(
        simulated.trace.applied_hooks,
        vec!["bomb_explosion_push".to_string()]
    );
    let pushed_right = simulated.state.board[4 * 9 + 7].expect("enemy must be pushed right");
    let pushed_down = simulated.state.board[6 * 9 + 5].expect("enemy must be pushed down");
    assert_eq!(pushed_right.side, Side::White);
    assert_eq!(pushed_down.side, Side::White);
}

#[test]
fn spec_safe_room_relocates_friendly_king() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/3K5 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "室".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("hook must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&99));
    assert_eq!(
        simulated.trace.applied_hooks,
        vec!["safe_room_king_relocation".to_string()]
    );
    let safe_room_king = simulated.state.board[8 * 9 + 4].expect("king must move to safe room");
    assert_eq!(safe_room_king.side, Side::Black);
    assert_eq!(safe_room_king.kind, PieceKind::King);
    assert!(simulated.expected_value > 0.29 && simulated.expected_value < 0.31);
}

#[test]
fn spec_fixed_marks_next_turn_restriction_hook() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "定".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("hook must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&100));
    assert_eq!(
        simulated.trace.applied_hooks,
        vec!["fixed_next_turn_restriction".to_string()]
    );
}

#[test]
fn spec_edge_marks_hook_when_enemy_is_on_board_edge() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("p3k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "辺".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("hook must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&103));
    assert_eq!(
        simulated.trace.applied_hooks,
        vec!["edge_line_imprison".to_string()]
    );
}

#[test]
fn spec_escape_moves_friendly_king_in_the_same_direction() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "逃".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("hook must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&106));
    assert_eq!(
        simulated.trace.applied_hooks,
        vec!["escape_king_follow".to_string()]
    );
    let escaped_king = simulated.state.board[8 * 9 + 5].expect("king must follow the move");
    assert_eq!(escaped_king.side, Side::Black);
    assert_eq!(escaped_king.kind, PieceKind::King);
}

#[test]
fn spec_darkness_continuous_aura_marks_adjacent_enemy_with_status() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "闇".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&11));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:apply_status:adjacent_enemy".to_string()]
    );
}

#[test]
fn stateful_apply_status_blocks_the_marked_enemy_piece() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "氷".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated
        .state
        .has_piece_status(4, 6, Side::White, "freeze"));

    let legal = generate_legal_moves(&simulated.state, &rules, true);
    assert!(legal.iter().all(|candidate| candidate.from != Some((4, 6))));
}

#[test]
fn stateful_modify_movement_limits_the_marked_enemy_piece_to_vertical_steps() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1r2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "沼".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated
        .state
        .has_movement_modifier(4, 6, Side::White, "vertical_step_only"));

    let legal = generate_legal_moves(&simulated.state, &rules, true);
    let rook_moves: Vec<_> = legal
        .iter()
        .filter(|candidate| candidate.from == Some((4, 6)))
        .collect();
    assert!(!rook_moves.is_empty());
    assert!(rook_moves
        .iter()
        .all(|candidate| { candidate.to.1 == 6 && (candidate.to.0 as i32 - 4).abs() == 1 }));
}

#[test]
fn spec_moss_summons_a_clone_into_the_first_adjacent_empty_cell() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "苔".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&23));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "summon_piece"));

    let clone = simulated.state.board[3 * 9 + 4].expect("clone must appear at first empty cell");
    assert_eq!(clone.side, Side::Black);
}

#[test]
fn spec_lantern_marks_transform_when_an_ally_pawn_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/4P4/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "灯".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&93));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "transform_piece"));
    assert!(simulated.expected_value > 0.19 && simulated.expected_value < 0.21);
}

#[test]
fn spec_tin_marks_apply_status_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "錫".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&14));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "apply_status"));
    assert!(simulated.expected_value > 0.09 && simulated.expected_value < 0.11);
}

#[test]
fn spec_bulls_summons_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "犇".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&58));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "summon_piece"));
    assert!(simulated.expected_value > 0.09 && simulated.expected_value < 0.11);
}

#[test]
fn spec_a_transforms_an_adjacent_enemy_piece_into_a_pawn() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1r2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "あ".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&30));
    let transformed = simulated.state.board[4 * 9 + 6].expect("adjacent enemy must remain");
    assert_eq!(transformed.side, Side::White);
    assert_eq!(transformed.kind, PieceKind::Pawn);
    assert!(!transformed.promoted);
}

#[test]
fn spec_water_continuous_forced_move_pushes_adjacent_enemy_away() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1r2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "水".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&5));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "forced_move"));
    assert!(simulated.state.board[4 * 9 + 6].is_none());
    let pushed_piece = simulated.state.board[4 * 9 + 7].expect("enemy must be pushed away");
    assert_eq!(pushed_piece.side, Side::White);
}

#[test]
fn spec_time_continuous_aura_marks_adjacent_enemy_with_time_stop() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "時".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&18));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "apply_status"));
}

#[test]
fn spec_ice_continuous_aura_marks_adjacent_enemy_with_freeze() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "氷".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&19));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "apply_status"));
}

#[test]
fn spec_heart_after_move_marks_adjacent_enemy_with_stun() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "心".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&74));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:apply_status:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_fish_marks_apply_status_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "魚".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&24));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "apply_status"));
    assert!(simulated.expected_value > 0.29 && simulated.expected_value < 0.31);
}

#[test]
fn spec_prison_after_move_marks_adjacent_enemy_with_stun() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "牢".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&31));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:apply_status:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_beast_after_move_marks_adjacent_enemy_with_stun() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "獣".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&72));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:apply_status:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_flame_removes_an_adjacent_enemy_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "炎".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&3));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "remove_piece"));
    assert!(simulated.expected_value > 0.09 && simulated.expected_value < 0.11);
    assert!(simulated.state.board[4 * 9 + 6].is_none());
}

#[test]
fn spec_demon_removes_an_adjacent_enemy_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "魔".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&12));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "remove_piece"));
    assert!(simulated.expected_value > 0.09 && simulated.expected_value < 0.11);
    assert!(simulated.state.board[4 * 9 + 6].is_none());
}

#[test]
fn spec_awakened_dragon_removes_an_adjacent_enemy_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "辰".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&48));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "remove_piece"));
    assert!(simulated.expected_value > 0.09 && simulated.expected_value < 0.11);
    assert!(simulated.state.board[4 * 9 + 6].is_none());
}

#[test]
fn spec_second_marks_extra_action_when_capture_occurs() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "乙".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&76));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "extra_action"));
}

#[test]
fn spec_convex_marks_extra_action_after_move() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "凸".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&81));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "extra_action"));
}

#[test]
fn spec_stir_fry_marks_extra_action_when_capture_occurs() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "炒".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&83));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "extra_action"));
}

#[test]
fn spec_iron_continuous_forced_move_pushes_adjacent_enemy_away() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1r2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "鉄".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&13));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "forced_move"));
    assert!(simulated.state.board[4 * 9 + 6].is_none());
    let pushed_piece = simulated.state.board[4 * 9 + 7].expect("enemy must be pushed away");
    assert_eq!(pushed_piece.side, Side::White);
}

#[test]
fn spec_roar_after_move_pushes_adjacent_enemy_away() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1r2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "轟".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&57));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "forced_move"));
    assert!(simulated.state.board[4 * 9 + 6].is_none());
}

#[test]
fn spec_oni_after_move_pushes_adjacent_enemy_away() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1r2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "鬼".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&68));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "forced_move"));
    assert!(simulated.state.board[4 * 9 + 6].is_none());
}

#[test]
fn spec_ore_marks_transform_when_an_ally_pawn_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/4P4/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "鉱".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&35));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "transform_piece"));
    assert!(simulated.expected_value > 0.19 && simulated.expected_value < 0.21);
}

#[test]
fn spec_experiment_marks_adjacent_enemy_transform() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1r2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "実".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&50));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:transform_piece:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_coin_marks_self_transform_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "銭".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&89));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "transform_piece"));
    assert!(simulated.expected_value > 0.19 && simulated.expected_value < 0.21);
}

#[test]
fn spec_tree_marks_summon_piece_when_adjacent_empty_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "木".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&7));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "summon_piece"));
}

#[test]
fn spec_ridge_marks_summon_piece_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "嶺".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&32));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "summon_piece"));
    assert!(simulated.expected_value > 0.19 && simulated.expected_value < 0.21);
}

#[test]
fn spec_grave_marks_summon_piece_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "墓".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&36));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "summon_piece"));
    assert!(simulated.expected_value > 0.19 && simulated.expected_value < 0.21);
}

#[test]
fn spec_rock_marks_summon_piece_after_move() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "岩".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&34));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "summon_piece"));
}

#[test]
fn spec_roast_marks_summon_piece_after_capture() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "焼".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&82));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "summon_piece"));
}

#[test]
fn spec_boil_marks_summon_piece_after_capture() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "煮".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&84));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "summon_piece"));
}

#[test]
fn spec_swamp_marks_adjacent_enemy_modify_movement() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "沼".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&28));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:modify_movement:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_cherry_marks_same_row_ally_modify_movement() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/4P4/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 6,
        to_col: 5,
        piece_code: "桜".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&79));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:modify_movement:same_row_ally".to_string()]
    );
}

#[test]
fn spec_concave_marks_self_modify_movement_for_long_move() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 8,
        piece_code: "凹".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&80));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:modify_movement:self_piece".to_string()]
    );
}

#[test]
fn spec_dragon_marks_self_transform() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "竜".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&2));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "transform_piece"));
}

#[test]
fn spec_fire_burns_one_enemy_hand_piece_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b p2g 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "火".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&4));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "destroy_hand_piece"));
    assert!(simulated.expected_value > 0.19 && simulated.expected_value < 0.21);
}

#[test]
fn spec_treasure_gains_a_gold_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "宝".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&15));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "gain_piece"));
    assert!(simulated.expected_value > 0.19 && simulated.expected_value < 0.21);
}

#[test]
fn spec_electric_marks_adjacent_enemy_with_shock() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "電".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&16));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "apply_status"));
    assert!(simulated.expected_value > 0.19 && simulated.expected_value < 0.21);
}

#[test]
fn spec_thunder_removes_one_enemy_hand_piece_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b p2g 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "雷".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&17));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "remove_piece"));
    assert!(simulated.expected_value > 0.09 && simulated.expected_value < 0.11);
}

#[test]
fn spec_house_marks_summon_piece_when_adjacent_empty_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "家".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&44));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "summon_piece"));
}

#[test]
fn spec_people_marks_self_modify_movement_for_diagonal_step() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 5,
        piece_code: "民".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&45));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:modify_movement:self_piece".to_string()]
    );
}

#[test]
fn spec_field_marks_self_modify_movement_for_diagonal_step() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 5,
        piece_code: "畑".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&46));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:modify_movement:self_piece".to_string()]
    );
}

#[test]
fn spec_medicine_marks_adjacent_ally_modify_movement() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/5P3/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "薬".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&64));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:modify_movement:adjacent_ally".to_string()]
    );
}

#[test]
fn spec_depression_marks_origin_cell_apply_status() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "鬱".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&75));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:apply_status:origin_cell".to_string()]
    );
}

#[test]
fn spec_sand_marks_linked_action_when_same_row_ally_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/4P4/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 6,
        to_col: 5,
        piece_code: "砂".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&21));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:linked_action:same_row_ally".to_string()]
    );
}

#[test]
fn spec_poison_marks_board_hazard_on_origin_cell() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "毒".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&27));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:board_hazard:origin_cell".to_string()]
    );
}

#[test]
fn stateful_board_hazard_blocks_enemy_entry_and_changes_evaluation() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/1r2R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "毒".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.state.has_board_hazard(4, 4, Side::White));

    let legal = generate_legal_moves(&simulated.state, &rules, true);
    assert!(!legal
        .iter()
        .any(|candidate| candidate.from == Some((4, 1)) && candidate.to == (4, 4)));

    let mut without_hazard = simulated.state.clone();
    without_hazard.skill_state.board_hazards.clear();
    let cfg = EngineConfig::default();
    assert!(
        evaluate_state(&simulated.state, &cfg, &rules)
            < evaluate_state(&without_hazard, &cfg, &rules)
    );
}

#[test]
fn stateful_defense_or_immunity_blocks_enemy_capture_and_changes_evaluation() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P1r/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "禽".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated
        .state
        .has_piece_defense(4, 6, Side::Black, "grant_invulnerability"));

    let legal = generate_legal_moves(&simulated.state, &rules, true);
    assert!(!legal
        .iter()
        .any(|candidate| candidate.from == Some((4, 8)) && candidate.to == (4, 6)));

    let mut without_defense = simulated.state.clone();
    without_defense.skill_state.piece_defenses.clear();
    let cfg = EngineConfig::default();
    assert!(
        evaluate_state(&simulated.state, &cfg, &rules)
            < evaluate_state(&without_defense, &cfg, &rules)
    );
}

#[test]
fn spec_mirror_marks_copy_ability_when_front_enemy_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/5p3/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "鏡".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&29));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:copy_ability:front_enemy".to_string()]
    );
}

#[test]
fn spec_phantom_marks_defense_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "幻".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&38));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "defense_or_immunity"));
    assert!(simulated.expected_value > 0.49 && simulated.expected_value < 0.51);
}

#[test]
fn spec_boat_marks_linked_action_when_adjacent_ally_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "舟".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&41));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:linked_action:adjacent_ally".to_string()]
    );
}

#[test]
fn spec_machine_marks_copy_ability_when_adjacent_ally_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "機".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&42));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:copy_ability:adjacent_ally".to_string()]
    );
}

#[test]
fn spec_armor_marks_defense_or_immunity() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "鎧".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&53));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "defense_or_immunity"));
}

#[test]
fn spec_holy_sword_marks_defense_when_adjacent_empty_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "剣".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&61));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "defense_or_immunity"));
}

#[test]
fn spec_rose_marks_board_hazard_when_adjacent_empty_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "薔".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&77));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:board_hazard:adjacent_empty".to_string()]
    );
}

#[test]
fn spec_chrysanthemum_marks_revive_when_adjacent_ally_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "菊".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&78));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:revive:adjacent_ally".to_string()]
    );
}

#[test]
fn spec_cloud_marks_capture_constraint() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "雲".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&25));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:capture_constraint:self_piece".to_string()]
    );
}

#[test]
fn spec_peak_marks_disable_piece_when_adjacent_enemy_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "峰".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&33));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:disable_piece:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_spirit_marks_capture_constraint() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "霊".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&37));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:capture_constraint:self_piece".to_string()]
    );
}

#[test]
fn spec_mist_sends_adjacent_enemy_to_hand() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "霧".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&39));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:send_to_hand:adjacent_enemy".to_string()]
    );
    assert!(simulated.expected_value > 0.19 && simulated.expected_value < 0.21);
    assert!(simulated.state.board[4 * 9 + 6].is_none());
}

#[test]
fn spec_gear_marks_linked_action_when_adjacent_ally_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "歯".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&43));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:linked_action:adjacent_ally".to_string()]
    );
}

#[test]
fn spec_saint_marks_disable_piece_when_adjacent_enemy_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "聖".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&60));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:disable_piece:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_shield_marks_disable_piece_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "盾".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&62));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:disable_piece:adjacent_ally".to_string()]
    );
    assert!(simulated.expected_value > 0.49 && simulated.expected_value < 0.51);
}

#[test]
fn spec_hole_marks_board_hazard_on_after_capture() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "穴".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&66));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:board_hazard:adjacent_empty".to_string()]
    );
}

#[test]
fn spec_oboro_marks_defense_or_immunity() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "朧".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&69));
    assert!(simulated
        .trace
        .applied_effects
        .iter()
        .any(|effect| effect == "defense_or_immunity"));
}

#[test]
fn spec_bird_marks_defense_when_adjacent_ally_exists() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "禽".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&73));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:defense_or_immunity:adjacent_ally".to_string()]
    );
}

#[test]
fn spec_death_removes_an_adjacent_enemy() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "死".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&70));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:remove_piece:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_soul_marks_defense_or_immunity() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "魂".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&71));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:defense_or_immunity:self_piece".to_string()]
    );
}

#[test]
fn spec_yin_composite_marks_seal_and_defense_effects() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "陰".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&86));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec![
            "1:seal_skill:adjacent_enemy".to_string(),
            "2:defense_or_immunity:self_piece".to_string()
        ]
    );
}

#[test]
fn spec_cow_marks_backward_step_modify_movement() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/9/4R4/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(5),
        from_col: Some(4),
        to_row: 6,
        to_col: 4,
        piece_code: "牛".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&87));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:modify_movement:self_piece".to_string()]
    );
}

#[test]
fn spec_wealth_marks_after_capture_adjacent_enemy_transform() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4Rp3/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "財".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&90));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:transform_piece:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_giant_marks_two_defense_effects() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "巨".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&91));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec![
            "1:defense_or_immunity:self_piece".to_string(),
            "2:defense_or_immunity:self_piece".to_string()
        ]
    );
}

#[test]
fn spec_snowman_marks_gain_piece_with_probability_weight() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "雪".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&20));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:gain_piece:ally_hand_piece".to_string()]
    );
    assert!(simulated.expected_value > 0.19 && simulated.expected_value < 0.21);
}

#[test]
fn spec_mutant_marks_self_transform() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "異".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&51));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:transform_piece:self_piece".to_string()]
    );
}

#[test]
fn spec_disease_marks_after_capture_adjacent_enemy_status() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4Rp3/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "病".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&63));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:apply_status:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_abyss_marks_after_capture_adjacent_enemy_status() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4Rp3/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "淵".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&67));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:apply_status:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_moon_marks_turn_start_modify_movement() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "月".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&40));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:modify_movement:self_piece".to_string()]
    );
}

#[test]
fn turn_boundaries_refresh_moon_cycle_and_expire_swamp_restrictions() {
    let rules = sample_runtime_rules();

    let moon_state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let moon_move = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "月".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let moon_simulated =
        simulate_move_with_skills(&moon_state, &moon_move, &rules).expect("skill must apply");
    let white_reply = apply_move(
        &moon_simulated.state,
        &GenMove {
            from: Some((0, 4)),
            to: (0, 3),
            piece: moon_simulated.state.board[4].expect("white king must exist"),
            promote: false,
            capture: None,
            drop: None,
        },
    );
    assert!(white_reply.has_movement_modifier(4, 5, Side::Black, "orthogonal_step_only"));

    let moon_legal = generate_legal_moves(&white_reply, &rules, true);
    let moon_moves: Vec<_> = moon_legal
        .iter()
        .filter(|candidate| candidate.from == Some((4, 5)))
        .collect();
    assert!(!moon_moves.is_empty());
    assert!(moon_moves.iter().all(|candidate| {
        let dr = (candidate.to.0 as i32 - 4).abs();
        let dc = (candidate.to.1 as i32 - 5).abs();
        (dr == 1 && dc == 0) || (dr == 0 && dc == 1)
    }));

    let swamp_state =
        SearchState::from_sfen("4k4/9/9/9/4R1r2/9/9/9/4K4 b - 1").expect("must parse");
    let swamp_move = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "沼".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let swamp_simulated =
        simulate_move_with_skills(&swamp_state, &swamp_move, &rules).expect("skill must apply");
    assert!(swamp_simulated
        .state
        .has_movement_modifier(4, 6, Side::White, "vertical_step_only"));

    let expired = apply_move(
        &swamp_simulated.state,
        &GenMove {
            from: Some((0, 4)),
            to: (0, 3),
            piece: swamp_simulated.state.board[4].expect("white king must exist"),
            promote: false,
            capture: None,
            drop: None,
        },
    );
    assert!(!expired.has_movement_modifier(4, 6, Side::White, "vertical_step_only"));
}

#[test]
fn spec_book_marks_turn_start_copy_ability() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "書".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&55));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:copy_ability:self_piece".to_string()]
    );
}

#[test]
fn spec_blade_marks_after_capture_multi_capture() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/3pRp3/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "刀".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&52));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:multi_capture:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_gun_marks_front_enemy_multi_capture() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/9/4p4/9/4R4/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(7),
        from_col: Some(4),
        to_row: 6,
        to_col: 4,
        piece_code: "銃".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&54));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:multi_capture:front_enemy".to_string()]
    );
}

#[test]
fn spec_cannon_marks_after_capture_capture_with_leap() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "砲".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&1));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:capture_with_leap:self_piece".to_string()]
    );
}

#[test]
fn spec_leaf_marks_probability_gated_forced_move() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "葉".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&8));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:forced_move:adjacent_enemy".to_string()]
    );
    assert!(simulated.expected_value > 0.09 && simulated.expected_value < 0.11);
}

#[test]
fn spec_star_marks_probability_gated_return_to_hand() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "星".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&10));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:return_to_hand:self_piece".to_string()]
    );
    assert!(simulated.expected_value > 0.39 && simulated.expected_value < 0.41);
}

#[test]
fn spec_spring_marks_adjacent_ally_defense() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "泉".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&47));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec![
            "1:defense_or_immunity:adjacent_ally".to_string(),
            "2:defense_or_immunity:self_piece".to_string()
        ]
    );
}

#[test]
fn spec_experiment_k_marks_probability_gated_summon_and_defense() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 4,
        piece_code: "K".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&49));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec![
            "1:summon_piece:adjacent_empty".to_string(),
            "2:defense_or_immunity:self_piece".to_string()
        ]
    );
    assert!(simulated.expected_value > 0.39 && simulated.expected_value < 0.41);
}

#[test]
fn spec_seal_marks_adjacent_enemy_seal_skill() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "封".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&56));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:seal_skill:adjacent_enemy".to_string()]
    );
}

#[test]
fn spec_courtesy_marks_after_capture_substitute() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "礼".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&59));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:substitute:self_piece".to_string()]
    );
}

#[test]
fn spec_yang_marks_probability_gated_self_defense() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 4,
        piece_code: "陽".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&85));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:defense_or_immunity:self_piece".to_string()]
    );
    assert!(simulated.expected_value > 0.29 && simulated.expected_value < 0.31);
}

#[test]
fn spec_pig_marks_after_capture_inherit_ability() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "豚".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&88));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec!["1:inherit_ability:self_piece".to_string()]
    );
}

#[test]
fn spec_nothing_marks_capture_constraint_and_defense() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 4,
        piece_code: "無".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&92));
    assert_eq!(
        simulated.trace.applied_effect_steps,
        vec![
            "2:capture_constraint:self_piece".to_string(),
            "3:defense_or_immunity:self_piece".to_string()
        ]
    );
}

#[test]
fn waterfall_skill_improves_move_score_when_push_is_available() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1r2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "滝".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn darkness_skill_improves_move_score_when_adjacent_enemy_exists() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "闇".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn flame_skill_improves_move_score_when_adjacent_enemy_exists() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "炎".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn convex_skill_improves_move_score_when_extra_action_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "凸".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn iron_skill_improves_move_score_when_push_is_available() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1r2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "鉄".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn coin_skill_improves_move_score_when_self_transform_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "銭".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn tree_skill_improves_move_score_when_summon_piece_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "木".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn roast_skill_improves_move_score_when_after_capture_summon_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "焼".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn concave_skill_improves_move_score_when_long_move_matches_rule() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 8,
        piece_code: "凹".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn fire_skill_improves_move_score_when_enemy_hand_burn_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b p 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "火".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn treasure_skill_improves_move_score_when_gain_piece_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "宝".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn sand_skill_improves_move_score_when_linked_action_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/4P4/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 6,
        to_col: 5,
        piece_code: "砂".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn poison_skill_improves_move_score_when_board_hazard_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "毒".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn mirror_skill_improves_move_score_when_copy_ability_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/5p3/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "鏡".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn phantom_skill_improves_move_score_when_defense_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "幻".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn chrysanthemum_skill_improves_move_score_when_revive_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "菊".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn cloud_skill_improves_move_score_when_capture_constraint_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "雲".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn peak_skill_improves_move_score_when_disable_piece_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "峰".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn mist_skill_improves_move_score_when_send_to_hand_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "霧".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn hole_skill_improves_move_score_when_after_capture_hazard_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "穴".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn oboro_skill_improves_move_score_when_defense_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "朧".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn bird_skill_improves_move_score_when_adjacent_ally_defense_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "禽".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn death_skill_improves_move_score_when_remove_piece_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "死".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn soul_skill_improves_move_score_when_defense_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "魂".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn yin_skill_improves_move_score_when_seal_and_defense_apply() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "陰".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn cow_skill_improves_move_score_when_backward_step_rule_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/9/4R4/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(5),
        from_col: Some(4),
        to_row: 6,
        to_col: 4,
        piece_code: "牛".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn wealth_skill_improves_move_score_when_after_capture_transform_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4Rp3/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "財".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn giant_skill_improves_move_score_when_double_defense_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "巨".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn snowman_skill_improves_move_score_when_gain_piece_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "雪".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn mutant_skill_improves_move_score_when_transform_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "異".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn disease_skill_improves_move_score_when_after_capture_status_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4Rp3/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "病".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn abyss_skill_improves_move_score_when_after_capture_status_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4Rp3/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "淵".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn moon_skill_improves_move_score_when_turn_start_modify_movement_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "月".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn book_skill_improves_move_score_when_turn_start_copy_ability_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "書".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn blade_skill_improves_move_score_when_after_capture_multi_capture_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/3pRp3/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "刀".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn gun_skill_improves_move_score_when_front_enemy_multi_capture_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/9/4p4/9/4R4/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(7),
        from_col: Some(4),
        to_row: 6,
        to_col: 4,
        piece_code: "銃".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn cannon_skill_improves_move_score_when_after_capture_capture_with_leap_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "砲".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn leaf_skill_improves_move_score_when_probability_gated_forced_move_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "葉".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn star_skill_improves_move_score_when_return_to_hand_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "星".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn spring_skill_improves_move_score_when_adjacent_ally_defense_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "泉".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn experiment_k_skill_improves_move_score_when_summon_and_defense_apply() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 4,
        piece_code: "K".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn seal_skill_improves_move_score_when_adjacent_enemy_is_present() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "封".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn courtesy_skill_improves_move_score_when_substitute_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "礼".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn yang_skill_improves_move_score_when_probability_gated_self_defense_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 4,
        piece_code: "陽".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn pig_skill_improves_move_score_when_inherit_ability_applies() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/4p4/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "豚".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

#[test]
fn nothing_skill_improves_move_score_when_capture_constraint_and_defense_apply() {
    let cfg = EngineConfig::default();
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4R4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 4,
        piece_code: "無".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };

    let adjustment = score_move_with_skill_effects(&state, &mv, &rules, &cfg);
    assert!(adjustment > 0);
}

// ── piece_mapping: 特殊SFENコード処理 (Phase 3 TDD) ──────────────────────────

/// 標準SFEN (P/L/N/S/G/B/R/K) で from_sfen が正常にパースできる（リグレッション）
#[test]
fn piece_mapping_standard_sfen_parses_correctly() {
    let result = SearchState::from_sfen(
        "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
    );
    assert!(result.is_ok(), "標準SFEN は Ok を返すべき: {:?}", result.err());
    let state = result.unwrap();
    let pieces: Vec<_> = state.board.iter().filter_map(|p| *p).collect();
    assert!(!pieces.is_empty());
}

/// 成り駒 (+P) が正しくパースされる
#[test]
fn piece_mapping_promoted_sfen_parses_correctly() {
    let result = SearchState::from_sfen("9/9/9/9/4+P4/9/9/9/4K4 b - 1");
    assert!(result.is_ok(), "成り駒SFENはOkを返すべき: {:?}", result.err());
    let state = result.unwrap();
    let piece = state.board[4 * 9 + 4].expect("row=4 col=4 に駒があるべき");
    assert!(piece.promoted);
    assert_eq!(piece.kind, PieceKind::Pawn);
}

/// 特殊SFENコード (C/D/E/F/H 等) は Err を返さずスキップされる
#[test]
fn piece_mapping_special_sfen_chars_are_parsed_not_skipped() {
    for (ch, name) in [('C', "NIN"), ('D', "KAG"), ('E', "HOU"), ('F', "RYU"), ('H', "HOO"),
                       ('I', "ENN"), ('J', "FIR"), ('M', "SUI"), ('Q', "NAM"), ('T', "MOK"),
                       ('U', "HAA"), ('V', "HIK"), ('W', "HOS"), ('X', "YAM"), ('Y', "MAK")] {
        let sfen = format!("9/9/9/9/4{}4/9/9/9/4K4 b - 1", ch);
        let result = SearchState::from_sfen(&sfen);
        assert!(
            result.is_ok(),
            "特殊SFENコード {}={} を含む SFEN は Ok であるべき: {:?}",
            ch, name, result.err()
        );
        let state = result.unwrap();
        assert!(state.board[4 * 9 + 4].is_some(), "{} should stay on board", name);
    }
}

/// 特殊SFENコードが存在しても王将の位置は正しい
#[test]
fn piece_mapping_king_survives_after_special_sfen_parse() {
    let result = SearchState::from_sfen("9/9/9/9/4C4/9/9/9/4K4 b - 1");
    assert!(result.is_ok());
    let state = result.unwrap();
    let king = state.board[8 * 9 + 4].expect("王将が row=8 col=4 に存在すべき");
    assert_eq!(king.kind, PieceKind::King);
}

/// displayChar ↔ PieceKind のラウンドトリップが一致している
#[test]
fn piece_mapping_standard_display_codes_round_trip() {
    use super::types::{piece_code, piece_kind_from_code};
    let pairs = [
        ("FU", PieceKind::Pawn),
        ("KY", PieceKind::Lance),
        ("KE", PieceKind::Knight),
        ("GI", PieceKind::Silver),
        ("KI", PieceKind::Gold),
        ("KA", PieceKind::Bishop),
        ("HI", PieceKind::Rook),
        ("OU", PieceKind::King),
    ];
    for (display, kind) in &pairs {
        assert_eq!(piece_kind_from_code(display), Some(*kind));
        assert_eq!(piece_code(kind), *display);
    }
}

#[test]
fn special_piece_sfen_generates_legal_moves_with_custom_vectors() {
    let state = SearchState::from_sfen("4k4/9/9/9/4C4/9/9/9/4K4 b - 1").expect("must parse");
    let rules = parse_runtime_rules(&serde_json::json!({
        "custom_move_vectors": {
            "NIN": [
                { "dr": -1, "dc": 0, "slide": false }
            ]
        }
    }))
    .expect("rules must parse");

    let moves = generate_legal_moves(&state, &rules, true);
    assert!(moves.iter().any(|mv| mv.piece.kind == PieceKind::Custom("NIN")));
    assert!(moves.iter().any(|mv| mv.to == (3, 4)));
}

#[test]
fn special_piece_in_hand_generates_drop_moves() {
    let state = SearchState::from_sfen("4k4/9/9/9/9/9/9/9/4K4 b C 1").expect("must parse");
    let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);
    assert!(moves.iter().any(|mv| mv.drop == Some(PieceKind::Custom("NIN"))));
}

#[test]
fn special_piece_skill_path_is_applied() {
    use super::skills::{RegistryRef, SkillClassification, SkillEffect, SkillSource, TargetRef};

    let state = SearchState::from_sfen("4k4/9/9/9/4E4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "HOU".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let mut rules = parse_runtime_rules(&serde_json::json!({
        "custom_move_vectors": {
            "HOU": [
                { "dr": -1, "dc": 0, "slide": false }
            ]
        }
    }))
    .expect("rules must parse");
    rules.skill_runtime.definitions.push(SkillDefinition {
        skill_id: 999,
        piece_chars: vec!["HOU".to_string()],
        source: SkillSource {
            skill_text: "special test".to_string(),
            source_kind: "test".to_string(),
            source_file: "tests".to_string(),
            source_function: "special_piece_skill_path_is_applied".to_string(),
        },
        classification: SkillClassification {
            implementation_kind: "primitive".to_string(),
            tags: vec![],
        },
        trigger: RegistryRef {
            group: "event_move".to_string(),
            type_code: "after_move".to_string(),
        },
        conditions: vec![],
        effects: vec![SkillEffect {
            order: 1,
            group: "extra_action".to_string(),
            type_code: "extra_action".to_string(),
            target: TargetRef {
                group: "self".to_string(),
                selector: "self_piece".to_string(),
            },
            params: serde_json::json!({ "extraActions": 1 }),
        }],
        script_hook: None,
        notes: None,
    });

    let simulated = simulate_move_with_skills(&state, &mv, &rules).expect("skill must apply");
    assert!(simulated.trace.applied_skill_ids.contains(&999));
}

// ─── Section: 特殊駒 14種 合法手テスト (NIN は既存テストで保証済み) ───────────────

fn make_custom_rules(code: &str, vectors: &[(i32, i32, bool)]) -> RuntimeRules {
    let vec_arr: Vec<serde_json::Value> = vectors
        .iter()
        .map(|(dr, dc, slide)| serde_json::json!({"dr": dr, "dc": dc, "slide": slide}))
        .collect();
    let mut cmv_map = serde_json::Map::new();
    cmv_map.insert(code.to_string(), serde_json::Value::Array(vec_arr));
    let board_state = serde_json::json!({"custom_move_vectors": cmv_map});
    parse_runtime_rules(&board_state).expect("rules must parse")
}

/// 特殊駒15種それぞれが custom_move_vectors 経由で合法手を1つ以上生成する
#[test]
fn all_15_special_pieces_generate_at_least_one_legal_move() {
    // (display_code, sfen_char, vectors, expected_to)
    // NIN(C) は既存テストで保証済みのため全15種再確認として含める
    let cases: &[(&str, char, &[(i32, i32, bool)], (usize, usize))] = &[
        ("NIN", 'C', &[(-1,-1,false),(-1,0,false),(-1,1,false),(0,-1,false),(0,1,false),(1,-1,false),(1,0,false),(1,1,false)], (3,4)),
        ("KAG", 'D', &[(-1,-1,true),(1,-1,true),(-1,0,false),(1,0,false),(-1,1,true),(1,1,true)], (3,3)),
        ("HOU", 'E', &[(0,-1,true),(-1,0,true),(1,0,true),(0,1,true)], (3,4)),
        ("RYU", 'F', &[(-1,-1,true),(0,-1,false),(1,-1,true),(-1,0,false),(1,0,false),(-1,1,true),(0,1,false),(1,1,true)], (3,3)),
        ("HOO", 'H', &[(-1,-1,false),(0,-1,true),(1,-1,false),(-1,0,true),(1,0,true),(-1,1,false),(0,1,true),(1,1,false)], (3,3)),
        ("ENN", 'I', &[(-1,-1,false),(-1,0,false),(-1,1,false),(0,-1,false),(0,1,false),(1,-1,false),(1,0,false),(1,1,false)], (3,4)),
        ("FIR", 'J', &[(0,-1,true),(-1,0,true),(1,0,true),(0,1,true)], (3,4)),
        ("SUI", 'M', &[(-1,-1,true),(1,-1,true),(-1,1,true),(1,1,true)], (3,3)),
        ("NAM", 'Q', &[(0,-1,true),(-1,0,true),(1,0,true),(0,1,true)], (3,4)),
        ("MOK", 'T', &[(-1,-1,false),(-1,0,false),(-1,1,false),(0,-1,false),(0,1,false),(1,-1,false),(1,0,false),(1,1,false)], (3,4)),
        ("HAA", 'U', &[(-1,-1,true),(1,-1,true),(-1,1,true),(1,1,true)], (3,3)),
        ("HIK", 'V', &[(-1,-1,true),(1,-1,true),(-1,1,true),(1,1,true)], (3,3)),
        ("HOS", 'W', &[(0,-1,true),(-1,0,true),(1,0,true),(0,1,true)], (3,4)),
        ("YAM", 'X', &[(-1,-1,false),(-1,0,false),(-1,1,false),(0,-1,false),(0,1,false),(1,-1,false),(1,0,false),(1,1,false)], (3,4)),
        ("MAK", 'Y', &[(0,-1,true),(-1,0,true),(1,0,true),(0,1,true)], (3,4)),
    ];
    for (code, sfen_ch, vectors, expected_to) in cases {
        let sfen = format!("4k4/9/9/9/4{}4/9/9/9/4K4 b - 1", sfen_ch);
        let state = SearchState::from_sfen(&sfen)
            .unwrap_or_else(|e| panic!("{} SFEN parse error: {}", code, e));
        let rules = make_custom_rules(code, vectors);
        let moves = generate_legal_moves(&state, &rules, true);
        assert!(!moves.is_empty(), "{}: must generate at least one legal move", code);
        assert!(
            moves.iter().any(|mv| mv.to == *expected_to),
            "{}: expected move to {:?} not found; got {:?}",
            code,
            expected_to,
            moves.iter().map(|m| m.to).collect::<Vec<_>>()
        );
    }
}

/// 合法手にスライド方向が含まれる: 例として HOO は正面にスライドできる
#[test]
fn hoo_slides_forward_past_first_square() {
    // HOO at (4,4): forward slide (-1,0,slide=true) → (3,4),(2,4),(1,4) should all be reachable
    let state = SearchState::from_sfen("4k4/9/9/9/4H4/9/9/9/4K4 b - 1").expect("must parse");
    let rules = make_custom_rules("HOO", &[(-1, 0, true)]);
    let moves = generate_legal_moves(&state, &rules, true);
    assert!(moves.iter().any(|mv| mv.to == (3, 4)), "HOO: must reach (3,4)");
    assert!(moves.iter().any(|mv| mv.to == (2, 4)), "HOO: must reach (2,4) via slide");
    assert!(moves.iter().any(|mv| mv.to == (1, 4)), "HOO: must reach (1,4) via slide");
}

/// 禁止方向が出ないことの確認: ENN は斜め前方にのみ移動できる場合に正面が出ない
#[test]
fn custom_vector_does_not_produce_moves_in_absent_direction() {
    // ENN at (4,4): only diagonal forward-left vector → (3,3) only
    let state = SearchState::from_sfen("4k4/9/9/9/4I4/9/9/9/4K4 b - 1").expect("must parse");
    let rules = make_custom_rules("ENN", &[(-1, -1, false)]);
    let moves = generate_legal_moves(&state, &rules, true);
    assert!(moves.iter().any(|mv| mv.to == (3, 3)), "ENN: must have (3,3)");
    assert!(!moves.iter().any(|mv| mv.to == (3, 4)), "ENN: must NOT have forward (3,4)");
    assert!(!moves.iter().any(|mv| mv.to == (3, 5)), "ENN: must NOT have (3,5)");
}

// ─── Section: 特殊駒の capture / hand / drop テスト ────────────────────────────

/// 特殊駒を取ったら持ち駒に入る
#[test]
fn capturing_special_piece_adds_it_to_custom_hand() {
    // Black HOU at (4,4), White pawn at (4,5). Black captures.
    // When Black captures White's pawn, pawn goes to Black hand (not HOU).
    let state = SearchState::from_sfen("4k4/9/9/9/4Ep3/9/9/9/4K4 b - 1").expect("must parse");
    let rules = make_custom_rules("HOU", &[(0, 1, true), (-1, 0, true), (1, 0, true), (0, -1, true)]);
    let moves = generate_legal_moves(&state, &rules, true);
    let capture_move = moves
        .iter()
        .find(|mv| mv.from == Some((4, 4)) && mv.to == (4, 5) && mv.capture.is_some())
        .expect("HOU must be able to capture at (4,5)");
    let next = apply_move(&state, capture_move);
    // pawn (FU) captured by Black goes to Black's standard hand
    assert!(next.hands.standard[0][0] >= 1, "captured pawn must be in Black's hand");
    // HOU remains Black's piece on the board (after having moved to (4,5))
    let piece = next.board[4 * 9 + 5].expect("HOU must be on board at (4,5)");
    assert_eq!(piece.kind, PieceKind::Custom("HOU"));

    // White rook captures Black HOU: White 'r' at (5,4), HOU at (4,4)
    let state2 = SearchState::from_sfen("4k4/9/9/9/4E4/4r4/9/9/4K4 w - 1").expect("must parse");
    let rules2 = RuntimeRules::default(); // White's rook can already slide
    let moves2 = generate_legal_moves(&state2, &rules2, true);
    let w_capture = moves2
        .iter()
        .find(|mv| mv.from == Some((5, 4)) && mv.to == (4, 4) && mv.capture.is_some())
        .expect("White rook must capture HOU at (4,4)");
    let next2 = apply_move(&state2, w_capture);
    // White side_index = 1
    let hou_in_white_hand = next2.hands.custom[1].get("HOU").copied().unwrap_or(0);
    assert_eq!(hou_in_white_hand, 1, "HOU must be in white's custom hand after capture");
    // Board at (4,4) now has the capturing rook, not the original HOU
    let at_44 = next2.board[4 * 9 + 4].expect("white rook must be at (4,4) after capture");
    assert_eq!(at_44.side, Side::White, "the piece at (4,4) must be the capturing white rook");
    assert_eq!(at_44.kind, PieceKind::Rook, "piece at (4,4) must be the rook, not HOU");
}

/// 持ち駒からドロップできる（全15種）
#[test]
fn all_special_pieces_can_be_dropped_from_hand() {
    let codes = ["NIN","KAG","HOU","RYU","HOO","ENN","FIR","SUI","NAM","MOK","HAA","HIK","HOS","YAM","MAK"];
    let sfen_chars = ['C','D','E','F','H','I','J','M','Q','T','U','V','W','X','Y'];
    for (code, sfen_ch) in codes.iter().zip(sfen_chars.iter()) {
        // Use hand notation with SFEN char
        let sfen = format!("4k4/9/9/9/9/9/9/9/4K4 b {} 1", sfen_ch);
        let state = SearchState::from_sfen(&sfen)
            .unwrap_or_else(|e| panic!("{} hand SFEN parse error: {}", code, e));
        let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);
        let has_drop = moves.iter().any(|mv| {
            mv.drop.is_some()
                && mv.from.is_none()
        });
        assert!(has_drop, "{}: must have at least one drop move from hand", code);
    }
}

/// ドロップ後に駒が盤上に正しく残る（代表: SUI）
#[test]
fn dropped_special_piece_appears_on_board() {
    let state = SearchState::from_sfen("4k4/9/9/9/9/9/9/9/4K4 b M 1").expect("must parse");
    let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);
    let drop = moves
        .iter()
        .find(|mv| mv.drop == Some(PieceKind::Custom("SUI")) && mv.to == (4, 4))
        .expect("SUI drop to center must exist");
    let next = apply_move(&state, drop);
    let piece = next.board[4 * 9 + 4].expect("SUI must appear on board after drop");
    assert_eq!(piece.kind, PieceKind::Custom("SUI"));
    assert_eq!(piece.side, Side::Black);
    assert!(!piece.promoted);
    // Hand count reduced to 0
    assert_eq!(next.hands.custom[0].get("SUI").copied().unwrap_or(0), 0);
}

/// 取られた特殊駒が不正に複製されない
#[test]
fn captured_special_piece_does_not_duplicate() {
    // White has RYU at (4,4). Black rook captures it.
    let state = SearchState::from_sfen("4k4/9/9/9/4f4/4R4/9/9/4K4 b - 1").expect("must parse");
    let moves = generate_legal_moves(&state, &RuntimeRules::default(), true);
    let cap = moves
        .iter()
        .find(|mv| mv.from == Some((5, 4)) && mv.to == (4, 4) && mv.capture.is_some())
        .expect("Black rook must capture RYU");
    let next = apply_move(&state, cap);
    let ryu_on_board = next.board.iter().filter(|p| {
        p.map(|x| x.kind == PieceKind::Custom("RYU")).unwrap_or(false)
    }).count();
    assert_eq!(ryu_on_board, 0, "RYU must not remain on board after capture");
    let ryu_in_black_hand = next.hands.custom[0].get("RYU").copied().unwrap_or(0);
    assert_eq!(ryu_in_black_hand, 1, "RYU must be in black hand exactly once");
}

// ─── Section: スキル定義あり全駒 – display code でのスキル発動テスト ─────────────────
//
// 以下のテストは piece_code に displayChar (HOU, RYU, ...) を使って
// catalog のスキル定義が発動することを保証する。
// pieceChars は漢字 (砲, 竜, ...) なので matches_piece_code が
// 漢字→displayChar の変換テーブルを持つまでは失敗する。

/// HOU (砲) after_capture スキルが displayCode で発動する
#[test]
fn spec_hou_cannon_after_capture_triggers_with_display_code() {
    let rules = sample_runtime_rules();
    // HOU at (4,4) captures enemy at (4,5)
    let state = SearchState::from_sfen("4k4/9/9/9/4E1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "HOU".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };
    let simulated = simulate_move_with_skills(&state, &mv, &rules)
        .expect("HOU skill must apply on capture");
    assert!(
        simulated.trace.applied_skill_ids.contains(&1),
        "skill_id=1 (HOU after_capture) must fire; got {:?}",
        simulated.trace.applied_skill_ids
    );
}

/// RYU (竜) continuous_rule スキルが displayCode で発動する
#[test]
fn spec_ryu_dragon_continuous_rule_triggers_with_display_code() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4F4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "RYU".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let simulated = simulate_move_with_skills(&state, &mv, &rules)
        .expect("RYU skill must apply");
    assert!(
        simulated.trace.applied_skill_ids.contains(&2),
        "skill_id=2 (RYU continuous_rule) must fire; got {:?}",
        simulated.trace.applied_skill_ids
    );
}

/// ENN (炎) continuous_aura スキルが displayCode + 隣接敵あり で発動する
#[test]
fn spec_enn_flame_continuous_aura_triggers_with_display_code() {
    let rules = sample_runtime_rules();
    // ENN at (4,4), moves to (4,5), enemy at (4,6) → adjacent to destination
    let state = SearchState::from_sfen("4k4/9/9/9/4I1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "ENN".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let simulated = simulate_move_with_skills(&state, &mv, &rules)
        .expect("ENN skill must apply");
    assert!(
        simulated.trace.applied_skill_ids.contains(&3),
        "skill_id=3 (ENN continuous_aura) must fire; got {:?}",
        simulated.trace.applied_skill_ids
    );
}

/// FIR (火) after_move スキルが displayCode で発動する
#[test]
fn spec_fir_fire_after_move_triggers_with_display_code() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4J4/9/9/9/4K4 b p 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 3,
        to_col: 4,
        piece_code: "FIR".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let simulated = simulate_move_with_skills(&state, &mv, &rules)
        .expect("FIR skill must apply");
    assert!(
        simulated.trace.applied_skill_ids.contains(&4),
        "skill_id=4 (FIR after_move) must fire; got {:?}",
        simulated.trace.applied_skill_ids
    );
}

/// NAM (波) continuous_aura スキルが displayCode + 隣接敵あり で発動する
#[test]
fn spec_nam_wave_continuous_aura_triggers_with_display_code() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4Q1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "NAM".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let simulated = simulate_move_with_skills(&state, &mv, &rules)
        .expect("NAM skill must apply");
    assert!(
        simulated.trace.applied_skill_ids.contains(&6),
        "skill_id=6 (NAM continuous_aura) must fire; got {:?}",
        simulated.trace.applied_skill_ids
    );
}

/// MOK (木) continuous_aura スキルが displayCode + 隣接空きマスあり で発動する
#[test]
fn spec_mok_tree_continuous_aura_triggers_with_display_code() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4T4/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "MOK".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let simulated = simulate_move_with_skills(&state, &mv, &rules)
        .expect("MOK skill must apply");
    assert!(
        simulated.trace.applied_skill_ids.contains(&7),
        "skill_id=7 (MOK continuous_aura summon) must fire; got {:?}",
        simulated.trace.applied_skill_ids
    );
}

/// HAA (葉) continuous_aura スキルが displayCode + 隣接敵あり で発動する
#[test]
fn spec_haa_leaf_continuous_aura_triggers_with_display_code() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4U1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "HAA".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let simulated = simulate_move_with_skills(&state, &mv, &rules)
        .expect("HAA skill must apply");
    assert!(
        simulated.trace.applied_skill_ids.contains(&8),
        "skill_id=8 (HAA continuous_aura) must fire; got {:?}",
        simulated.trace.applied_skill_ids
    );
}

/// HIK (光) after_move script_hook スキルが displayCode で発動する
#[test]
fn spec_hik_light_after_move_script_hook_triggers_with_display_code() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4V1P2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 7,
        piece_code: "HIK".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let simulated = simulate_move_with_skills(&state, &mv, &rules)
        .expect("HIK skill must apply");
    assert!(
        simulated.trace.applied_skill_ids.contains(&9),
        "skill_id=9 (HIK reflect) must fire; got {:?}",
        simulated.trace.applied_skill_ids
    );
}

/// HOS (星) after_capture スキルが displayCode で発動する
#[test]
fn spec_hos_star_after_capture_triggers_with_display_code() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4W1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "HOS".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: Some("FU".to_string()),
        notation: None,
    };
    let simulated = simulate_move_with_skills(&state, &mv, &rules)
        .expect("HOS skill must apply");
    assert!(
        simulated.trace.applied_skill_ids.contains(&10),
        "skill_id=10 (HOS after_capture) must fire; got {:?}",
        simulated.trace.applied_skill_ids
    );
}

/// YAM (闇) continuous_aura スキルが displayCode + 隣接敵あり で発動する
#[test]
fn spec_yam_darkness_continuous_aura_triggers_with_display_code() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4X1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "YAM".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let simulated = simulate_move_with_skills(&state, &mv, &rules)
        .expect("YAM skill must apply");
    assert!(
        simulated.trace.applied_skill_ids.contains(&11),
        "skill_id=11 (YAM darkness) must fire; got {:?}",
        simulated.trace.applied_skill_ids
    );
}

/// MAK (魔) continuous_aura スキルが displayCode + 隣接敵あり で発動する
#[test]
fn spec_mak_demon_continuous_aura_triggers_with_display_code() {
    let rules = sample_runtime_rules();
    let state = SearchState::from_sfen("4k4/9/9/9/4Y1p2/9/9/9/4K4 b - 1").expect("must parse");
    let mv = EngineMove {
        from_row: Some(4),
        from_col: Some(4),
        to_row: 4,
        to_col: 5,
        piece_code: "MAK".to_string(),
        promote: false,
        drop_piece_code: None,
        captured_piece_code: None,
        notation: None,
    };
    let simulated = simulate_move_with_skills(&state, &mv, &rules)
        .expect("MAK skill must apply");
    assert!(
        simulated.trace.applied_skill_ids.contains(&12),
        "skill_id=12 (MAK demon) must fire; got {:?}",
        simulated.trace.applied_skill_ids
    );
}

// ─── Section: catalog / mapping / moveVectors 整合性テスト ────────────────────

/// piece_mapping.rs の MAPPINGS が全15種の特殊駒 displayCode を含む
#[test]
fn piece_mapping_covers_all_15_special_pieces() {
    use super::piece_mapping::piece_kind_from_code;
    let codes = ["NIN","KAG","HOU","RYU","HOO","ENN","FIR","SUI","NAM","MOK","HAA","HIK","HOS","YAM","MAK"];
    for code in &codes {
        assert!(
            piece_kind_from_code(code).is_some(),
            "piece_mapping must contain display code: {}", code
        );
    }
}

/// 全15種の特殊駒が SFEN char を持つ (sfen_char_from_piece_kind が Some を返す)
#[test]
fn all_special_pieces_have_sfen_char() {
    use super::piece_mapping::{piece_kind_from_code, sfen_char_from_piece_kind};
    let codes = ["NIN","KAG","HOU","RYU","HOO","ENN","FIR","SUI","NAM","MOK","HAA","HIK","HOS","YAM","MAK"];
    for code in &codes {
        let kind = piece_kind_from_code(code)
            .unwrap_or_else(|| panic!("{} must exist in piece_mapping", code));
        assert!(
            sfen_char_from_piece_kind(&kind).is_some(),
            "{} must have a SFEN char", code
        );
    }
}

/// 全15種の特殊駒が SFEN から round-trip でパースできる
#[test]
fn all_special_pieces_round_trip_sfen() {
    use super::piece_mapping::{piece_kind_from_code, sfen_char_from_piece_kind};
    let codes_and_sfen: &[(&str, char)] = &[
        ("NIN",'C'),("KAG",'D'),("HOU",'E'),("RYU",'F'),("HOO",'H'),
        ("ENN",'I'),("FIR",'J'),("SUI",'M'),("NAM",'Q'),("MOK",'T'),
        ("HAA",'U'),("HIK",'V'),("HOS",'W'),("YAM",'X'),("MAK",'Y'),
    ];
    for (code, expected_sfen) in codes_and_sfen {
        let kind = piece_kind_from_code(code).unwrap();
        let sfen = sfen_char_from_piece_kind(&kind)
            .unwrap_or_else(|| panic!("{} must have SFEN char", code));
        assert_eq!(sfen, *expected_sfen, "{} SFEN char mismatch", code);
    }
}

/// catalog の skill_id 1–12 の pieceChars に含まれる漢字が
/// piece_mapping の kanji_to_code テーブルで displayCode に解決できる
#[test]
fn catalog_skill_piece_chars_resolve_to_known_display_codes() {
    use super::piece_mapping::kanji_to_code;
    // 特殊駒15種のスキル (skill_id 1–12) の pieceChars は全て漢字
    // それぞれが kanji_to_code で解決できることを保証する
    let definitions = load_sample_skill_definitions();
    let special_skill_ids: &[u64] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    for &sid in special_skill_ids {
        let def = definitions.iter().find(|d| d.skill_id == sid)
            .unwrap_or_else(|| panic!("skill_id={} must exist in catalog", sid));
        for piece_char in &def.piece_chars {
            // piece_char が displayCode として直接使えるか、または漢字→code で解決できること
            let resolves = crate::engine::piece_mapping::piece_kind_from_code(piece_char).is_some()
                || kanji_to_code(piece_char).is_some();
            assert!(
                resolves,
                "skill_id={} pieceChar='{}' must resolve to a known display code via kanji_to_code",
                sid, piece_char
            );
        }
    }
}

/// SUI (水) の displayCode がカタログの skill_5 pieceChars と matches_piece_code で一致する
/// (kanji_to_code 経由で水→SUI が通ること)
#[test]
fn kanji_to_code_resolves_all_12_special_skill_piece_chars() {
    use super::piece_mapping::kanji_to_code;
    let expected: &[(&str, &str)] = &[
        ("砲","HOU"),("竜","RYU"),("炎","ENN"),("火","FIR"),
        ("水","SUI"),("波","NAM"),("木","MOK"),("葉","HAA"),
        ("光","HIK"),("星","HOS"),("闇","YAM"),("魔","MAK"),
    ];
    for (kanji, code) in expected {
        assert_eq!(
            kanji_to_code(kanji),
            Some(*code),
            "kanji_to_code('{}') must return Some(\"{}\")", kanji, code
        );
    }
}

// ─── Section: HOU 砲型合法手テスト ────────────────────────────────────────────

fn make_cannon_rules() -> RuntimeRules {
    let vec_arr = serde_json::json!([
        {"dr": -1, "dc":  0, "slide": true, "capture_mode": "leap_over_one"},
        {"dr":  1, "dc":  0, "slide": true, "capture_mode": "leap_over_one"},
        {"dr":  0, "dc": -1, "slide": true, "capture_mode": "leap_over_one"},
        {"dr":  0, "dc":  1, "slide": true, "capture_mode": "leap_over_one"},
    ]);
    let board_state = serde_json::json!({"custom_move_vectors": {"HOU": vec_arr}});
    parse_runtime_rules(&board_state).expect("cannon rules must parse")
}

/// HOU は砲台なし方向には非取り移動のみ生成し、取り手は生成しない。
#[test]
fn hou_cannot_capture_without_platform() {
    let rules = make_cannon_rules();
    // E=HOU(黒) が (4,4)、敵 k が (0,4) にいる。間に駒なし → 取れない。
    let state =
        SearchState::from_sfen("4k4/9/9/9/4E4/9/9/9/4K4 b - 1").expect("must parse");
    let moves = generate_legal_moves(&state, &rules, false);
    let hou_captures: Vec<_> = moves
        .iter()
        .filter(|m| piece_code(&m.piece.kind) == "HOU" && m.capture.is_some())
        .collect();
    assert!(
        hou_captures.is_empty(),
        "HOU must not capture without a platform piece; got {} capture(s)",
        hou_captures.len()
    );
}

/// HOU は砲台ありの場合、砲台の先の敵駒を取れる。
#[test]
fn hou_can_capture_with_platform() {
    let rules = make_cannon_rules();
    // E=HOU(黒) (4,4)、味方 P=歩(黒) (2,4) が砲台、敵 k が (0,4)。
    let state =
        SearchState::from_sfen("4k4/9/4P4/9/4E4/9/9/9/4K4 b - 1").expect("must parse");
    let moves = generate_legal_moves(&state, &rules, false);
    let hou_capture = moves.iter().find(|m| {
        piece_code(&m.piece.kind) == "HOU"
            && m.from == Some((4, 4))
            && m.to == (0, 4)
            && m.capture.is_some()
    });
    assert!(
        hou_capture.is_some(),
        "HOU must capture enemy king at (0,4) over platform at (2,4)"
    );
}

/// HOU は砲台の手前のマスにしか移動手を生成しない（砲台のマス・砲台の先は非取り移動不可）。
#[test]
fn hou_move_only_reaches_squares_before_platform() {
    let rules = make_cannon_rules();
    // E=HOU(黒) (6,4)、味方 P=歩(黒) (3,4) が砲台、(0,4) に敵 k。
    // 上方向の非取り移動は (5,4),(4,4) のみ（砲台 (3,4) の手前まで）。
    let state =
        SearchState::from_sfen("4k4/9/9/4P4/9/9/4E4/9/4K4 b - 1").expect("must parse");
    let moves = generate_legal_moves(&state, &rules, false);
    let hou_up_non_captures: Vec<(usize, usize)> = moves
        .iter()
        .filter(|m| {
            piece_code(&m.piece.kind) == "HOU"
                && m.from == Some((6, 4))
                && m.to.1 == 4  // 同列 = 縦移動
                && m.to.0 < 6   // 上方向
                && m.capture.is_none()
        })
        .map(|m| m.to)
        .collect();
    assert!(
        hou_up_non_captures.contains(&(5, 4)) && hou_up_non_captures.contains(&(4, 4)),
        "HOU must move to rows 5 and 4 (before platform); got {:?}",
        hou_up_non_captures
    );
    assert!(
        !hou_up_non_captures.contains(&(3, 4))
            && !hou_up_non_captures.contains(&(2, 4))
            && !hou_up_non_captures.contains(&(1, 4)),
        "HOU must NOT move to rows 3,2,1 (platform or beyond); got {:?}",
        hou_up_non_captures
    );
}
