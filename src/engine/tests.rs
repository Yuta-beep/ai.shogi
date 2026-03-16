use super::config::{build_engine_config, EngineConfig, EngineConfigPatch};
use super::heuristic::is_board_coordinate_valid;
use super::rules::parse_runtime_rules;
use super::search::{apply_move, evaluate_state, generate_legal_moves, is_in_check};
use super::skill_executor::{score_move_with_skill_effects, simulate_move_with_skills};
use super::skills::{
    builtin_skill_registry, parse_skill_definition_document_value, validate_skill_definitions,
    SkillDefinition,
};
use super::types::{EngineMove, GenMove, PieceKind, RuntimeRules, SearchState, Side};
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
