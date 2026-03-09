use super::config::{build_engine_config, EngineConfig, EngineConfigPatch};
use super::heuristic::is_board_coordinate_valid;
use super::rules::parse_runtime_rules;
use super::search::{apply_move, generate_legal_moves, is_in_check};
use super::types::{EngineMove, PieceKind, RuntimeRules, SearchState, Side};
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
    let rules = parse_runtime_rules(&v);
    assert_eq!(
        rules.extra_vectors_by_piece.get("FU").map(|x| x.len()),
        Some(1)
    );
    assert_eq!(rules.eval_bonus_by_piece.get("FU").copied(), Some(10));
}
