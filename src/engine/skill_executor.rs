use crate::engine::config::EngineConfig;
use crate::engine::piece_mapping::sfen_char_from_piece_kind;
use crate::engine::search::{apply_move, evaluate_state};
use crate::engine::skills::{SkillCondition, SkillDefinition, SkillEffect};
use crate::engine::types::{
    piece_kind_from_code, side_index, EngineMove, GenMove, Piece, PieceKind,
    RuntimeRules, SearchState, Side,
};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct SkillExecutionTrace {
    pub applied_skill_ids: Vec<u64>,
    pub applied_effects: Vec<String>,
    pub applied_effect_steps: Vec<String>,
    pub applied_hooks: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SimulatedSkillState {
    pub state: SearchState,
    pub trace: SkillExecutionTrace,
    pub expected_value: f64,
}

pub fn score_move_with_skill_effects(
    state: &SearchState,
    mv: &EngineMove,
    rules: &RuntimeRules,
    cfg: &EngineConfig,
) -> i32 {
    let Some(base_state) = apply_engine_move(state, mv) else {
        return 0;
    };
    let Some(simulated) = simulate_move_with_skills(state, mv, rules) else {
        return 0;
    };

    let mover = state.side_to_move;
    let before = evaluate_state_for_side(&base_state, mover, cfg, rules);
    let after = evaluate_state_for_side(&simulated.state, mover, cfg, rules);
    let tactical_bonus = skill_trace_tactical_bonus(&simulated.trace);
    ((((after - before + tactical_bonus) as f64) * simulated.expected_value).round()) as i32
}

pub fn simulate_move_with_skills(
    state: &SearchState,
    mv: &EngineMove,
    rules: &RuntimeRules,
) -> Option<SimulatedSkillState> {
    let matching: Vec<&SkillDefinition> = rules
        .skill_runtime
        .definitions
        .iter()
        .filter(|definition| matches_piece_code(definition, &mv.piece_code))
        .collect();
    if matching.is_empty() {
        return None;
    }

    let mut current = apply_engine_move(state, mv)?;
    let mut trace = SkillExecutionTrace::default();
    let mut expected_value = 1.0_f64;
    let mut any_applied = false;

    for definition in matching {
        let trigger_matches = match definition.trigger.type_code.as_str() {
            "after_move" | "continuous_rule" | "continuous_aura" | "turn_start" => true,
            "after_capture" => move_captures_piece(state, mv),
            _ => false,
        };
        if !trigger_matches {
            continue;
        }

        let activation = activation_weight(&current, mv, &definition.conditions)?;
        if activation <= 0.0 {
            continue;
        }

        match definition.classification.implementation_kind.as_str() {
            "primitive" | "composite" => {
                let mut applied_this_skill = false;
                for effect in &definition.effects {
                    if execute_common_effect(&mut current, mv, effect) {
                        trace.applied_effects.push(effect.type_code.clone());
                        trace.applied_effect_steps.push(format!(
                            "{}:{}:{}",
                            effect.order, effect.type_code, effect.target.selector
                        ));
                        applied_this_skill = true;
                    }
                }
                if applied_this_skill {
                    trace.applied_skill_ids.push(definition.skill_id);
                    expected_value *= activation;
                    any_applied = true;
                }
            }
            "script_hook" => {
                if let Some(hook) = definition.script_hook.as_deref() {
                    if execute_script_hook(&mut current, mv, hook) {
                        trace.applied_skill_ids.push(definition.skill_id);
                        trace.applied_hooks.push(hook.to_string());
                        expected_value *= activation;
                        any_applied = true;
                    }
                }
            }
            _ => {}
        }
    }

    if any_applied {
        Some(SimulatedSkillState {
            state: current,
            trace,
            expected_value,
        })
    } else {
        None
    }
}

fn move_captures_piece(state: &SearchState, mv: &EngineMove) -> bool {
    if mv.captured_piece_code.is_some() {
        return true;
    }
    if !(0..=8).contains(&mv.to_row) || !(0..=8).contains(&mv.to_col) {
        return false;
    }
    let idx = mv.to_row as usize * 9 + mv.to_col as usize;
    matches!(state.board.get(idx).and_then(|piece| *piece), Some(piece) if piece.side != state.side_to_move)
}

fn apply_engine_move(state: &SearchState, mv: &EngineMove) -> Option<SearchState> {
    let gen = engine_move_to_gen_move(state, mv)?;
    Some(apply_move(state, &gen))
}

fn engine_move_to_gen_move(state: &SearchState, mv: &EngineMove) -> Option<GenMove> {
    if let Some(kind) = mv.drop_piece_code.as_deref().and_then(piece_kind_from_code) {
        return Some(GenMove {
            from: None,
            to: (mv.to_row as usize, mv.to_col as usize),
            piece: Piece {
                side: state.side_to_move,
                kind,
                promoted: false,
                sfen_char: sfen_char_from_piece_kind(&kind)?,
            },
            promote: false,
            capture: None,
            drop: Some(kind),
        });
    }

    let (from_row, from_col) = (mv.from_row?, mv.from_col?);
    if !(0..=8).contains(&from_row)
        || !(0..=8).contains(&from_col)
        || !(0..=8).contains(&mv.to_row)
        || !(0..=8).contains(&mv.to_col)
    {
        return None;
    }

    let from_idx = from_row as usize * 9 + from_col as usize;
    let to_idx = mv.to_row as usize * 9 + mv.to_col as usize;
    let piece = state.board[from_idx]?;
    let capture = state.board[to_idx];

    Some(GenMove {
        from: Some((from_row as usize, from_col as usize)),
        to: (mv.to_row as usize, mv.to_col as usize),
        piece,
        promote: mv.promote,
        capture,
        drop: None,
    })
}

fn activation_weight(
    state: &SearchState,
    mv: &EngineMove,
    conditions: &[SkillCondition],
) -> Option<f64> {
    let mut weight = 1.0_f64;
    for condition in conditions {
        match condition.type_code.as_str() {
            "chance_roll" => {
                weight *= condition
                    .params
                    .get("procChance")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0);
            }
            "adjacent_enemy_exists" => {
                if !has_adjacent_enemy(
                    state,
                    mv.to_row as usize,
                    mv.to_col as usize,
                    state.side_to_move,
                ) {
                    return None;
                }
            }
            "adjacent_empty_exists" => {
                if !has_adjacent_empty(state, mv.to_row as usize, mv.to_col as usize) {
                    return None;
                }
            }
            "target_not_king" => {
                if !has_transformable_adjacent_enemy(
                    state,
                    mv.to_row as usize,
                    mv.to_col as usize,
                    state.side_to_move,
                ) {
                    return None;
                }
            }
            "ally_piece_exists" => {
                if !has_matching_ally_piece(
                    state,
                    state.side_to_move.opposite(),
                    condition.params.get("pieceCode").and_then(Value::as_str),
                ) {
                    return None;
                }
            }
            _ => {}
        }
    }
    Some(weight)
}

fn execute_common_effect(state: &mut SearchState, mv: &EngineMove, effect: &SkillEffect) -> bool {
    match (
        effect.type_code.as_str(),
        effect.target.group.as_str(),
        effect.target.selector.as_str(),
    ) {
        ("forced_move", "adjacent", "adjacent_enemy") => {
            execute_forced_push_away(state, mv, effect)
        }
        ("modify_movement", "self", "self_piece") => execute_modify_movement(state, mv, effect),
        ("modify_movement", "adjacent", "adjacent_enemy") => {
            execute_modify_movement(state, mv, effect)
        }
        ("modify_movement", "adjacent", "adjacent_ally") => {
            execute_modify_movement(state, mv, effect)
        }
        ("modify_movement", "line", "same_row_ally") => execute_modify_movement(state, mv, effect),
        ("extra_action", "self", "self_piece") => execute_extra_action(effect),
        ("apply_status", "adjacent", "adjacent_enemy") => execute_apply_status(state, mv, effect),
        ("apply_status", "board", "origin_cell") => execute_apply_status(state, mv, effect),
        ("board_hazard", "board", "origin_cell") => execute_board_hazard(state, mv, effect),
        ("board_hazard", "adjacent", "adjacent_empty") => execute_board_hazard(state, mv, effect),
        ("copy_ability", "line", "front_enemy") => execute_copy_ability(state, mv),
        ("copy_ability", "adjacent", "adjacent_ally") => execute_copy_ability(state, mv),
        ("copy_ability", "self", "self_piece") => execute_copy_ability_self(effect),
        ("capture_with_leap", "self", "self_piece") => execute_capture_with_leap(effect),
        ("linked_action", "line", "same_row_ally") => execute_linked_action(state, mv, effect),
        ("linked_action", "adjacent", "adjacent_ally") => execute_linked_action(state, mv, effect),
        ("disable_piece", "adjacent", "adjacent_enemy") => execute_disable_piece(state, mv, effect),
        ("disable_piece", "adjacent", "adjacent_ally") => execute_disable_piece(state, mv, effect),
        ("capture_constraint", "self", "self_piece") => execute_capture_constraint(effect),
        ("multi_capture", "adjacent", "adjacent_enemy") => execute_multi_capture(state, mv, effect),
        ("multi_capture", "line", "front_enemy") => execute_multi_capture(state, mv, effect),
        ("seal_skill", "adjacent", "adjacent_enemy") => execute_seal_skill(state, mv, effect),
        ("defense_or_immunity", "self", "self_piece") => {
            execute_defense_or_immunity(state, mv, effect)
        }
        ("defense_or_immunity", "adjacent", "adjacent_ally") => {
            execute_defense_or_immunity(state, mv, effect)
        }
        ("remove_piece", "adjacent", "adjacent_enemy") => execute_remove_piece(state, mv),
        ("remove_piece", "hand", "enemy_hand_random") => execute_remove_enemy_hand_piece(state),
        ("destroy_hand_piece", "hand", "enemy_hand_random") => execute_destroy_hand_piece(state),
        ("return_to_hand", "self", "self_piece") => execute_return_to_hand(effect),
        ("send_to_hand", "adjacent", "adjacent_enemy") => execute_send_to_hand(state, mv),
        ("substitute", "self", "self_piece") => execute_substitute(effect),
        ("revive", "adjacent", "adjacent_ally") => execute_revive(state, mv),
        ("summon_piece", "adjacent", "adjacent_empty") => execute_summon_piece(state, mv, effect),
        ("gain_piece", "hand", "ally_hand_piece") => execute_gain_piece(state, effect),
        ("inherit_ability", "self", "self_piece") => execute_inherit_ability(effect),
        ("transform_piece", "global", "all_ally") => execute_transform_piece(state, effect),
        ("transform_piece", "self", "self_piece") => execute_transform_self(effect),
        ("transform_piece", "adjacent", "adjacent_enemy") => {
            execute_transform_adjacent_enemy(state, mv, effect)
        }
        _ => false,
    }
}

fn execute_modify_movement(state: &mut SearchState, mv: &EngineMove, effect: &SkillEffect) -> bool {
    let movement_rule = effect
        .params
        .get("movementRule")
        .and_then(Value::as_str)
        .unwrap_or("");
    let duration_turns = effect_duration_turns(effect);
    let immediate_applied = match (effect.target.selector.as_str(), movement_rule) {
        ("self_piece", "orthogonal_step_only") => is_orthogonal_step(mv, 1),
        ("self_piece", "diagonal_step_only") => is_diagonal_step(mv, 1),
        ("self_piece", "backward_step_only") => {
            is_backward_step(mv, state.side_to_move.opposite(), 1)
        }
        ("self_piece", "cyclic_pattern_change") => true,
        ("self_piece", "penetrate_to_edge") => reaches_board_edge(mv),
        ("adjacent_enemy", "orthogonal_step_only") => has_adjacent_enemy(
            state,
            mv.to_row as usize,
            mv.to_col as usize,
            state.side_to_move,
        ),
        ("adjacent_enemy", "vertical_step_only") => has_adjacent_enemy(
            state,
            mv.to_row as usize,
            mv.to_col as usize,
            state.side_to_move,
        ),
        ("adjacent_ally", "extend_range_by_one") => has_adjacent_ally(
            state,
            mv.to_row as usize,
            mv.to_col as usize,
            state.side_to_move.opposite(),
        ),
        ("same_row_ally", "extend_range_by_one") => has_same_row_ally(
            state,
            mv.to_row as usize,
            mv.to_col as usize,
            state.side_to_move.opposite(),
        ),
        _ => false,
    };

    let actor_side = state.side_to_move.opposite();
    let stateful_applied = match (effect.target.selector.as_str(), movement_rule) {
        ("adjacent_enemy", "vertical_step_only") | ("adjacent_enemy", "orthogonal_step_only") => {
            let mut applied = false;
            for dr in -1..=1 {
                for dc in -1..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = mv.to_row as isize + dr;
                    let nc = mv.to_col as isize + dc;
                    if !in_bounds(nr, nc) {
                        continue;
                    }
                    let idx = nr as usize * 9 + nc as usize;
                    let Some(piece) = state.board[idx] else {
                        continue;
                    };
                    if piece.side != state.side_to_move {
                        continue;
                    }
                    // 標準駒（カスタム駒以外）には移動制限を付与しない。
                    // 香車・歩兵など方向が固定された駒に付与すると逆方向への移動が生成されるため。
                    if !matches!(piece.kind, PieceKind::Custom(_)) {
                        continue;
                    }
                    state.add_movement_modifier(
                        nr as usize,
                        nc as usize,
                        piece.side,
                        movement_rule,
                        duration_turns,
                    );
                    applied = true;
                }
            }
            applied
        }
        ("self_piece", "cyclic_pattern_change") => {
            state.add_turn_start_rule(
                mv.to_row as usize,
                mv.to_col as usize,
                actor_side,
                movement_rule,
            );
            true
        }
        _ => false,
    };

    immediate_applied || stateful_applied
}

fn execute_apply_status(state: &mut SearchState, mv: &EngineMove, effect: &SkillEffect) -> bool {
    let Some(status_type) = effect
        .params
        .get("statusType")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let duration_turns = effect_duration_turns(effect);

    match (
        effect.target.group.as_str(),
        effect.target.selector.as_str(),
    ) {
        ("adjacent", "adjacent_enemy") => {
            let mut applied = false;
            for dr in -1..=1 {
                for dc in -1..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = mv.to_row as isize + dr;
                    let nc = mv.to_col as isize + dc;
                    if !in_bounds(nr, nc) {
                        continue;
                    }
                    let idx = nr as usize * 9 + nc as usize;
                    let Some(piece) = state.board[idx] else {
                        continue;
                    };
                    if piece.side != state.side_to_move {
                        continue;
                    }
                    state.add_piece_status(
                        nr as usize,
                        nc as usize,
                        piece.side,
                        status_type,
                        duration_turns,
                    );
                    applied = true;
                }
            }
            applied
        }
        ("board", "origin_cell") => {
            let (Some(from_row), Some(from_col)) = (mv.from_row, mv.from_col) else {
                return false;
            };
            state.add_board_hazard(
                from_row as usize,
                from_col as usize,
                state.side_to_move,
                format!("status:{status_type}"),
                duration_turns,
            );
            true
        }
        _ => false,
    }
}

fn execute_extra_action(effect: &SkillEffect) -> bool {
    effect
        .params
        .get("extraActions")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        > 0
}

fn execute_board_hazard(state: &mut SearchState, mv: &EngineMove, effect: &SkillEffect) -> bool {
    let Some(hazard_type) = effect
        .params
        .get("hazardType")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let duration_turns = effect_duration_turns(effect);

    match (
        effect.target.group.as_str(),
        effect.target.selector.as_str(),
    ) {
        ("board", "origin_cell") => {
            let (Some(from_row), Some(from_col)) = (mv.from_row, mv.from_col) else {
                return false;
            };
            state.add_board_hazard(
                from_row as usize,
                from_col as usize,
                state.side_to_move,
                hazard_type,
                duration_turns,
            );
            true
        }
        ("adjacent", "adjacent_empty") => {
            let mut applied = false;
            for dr in -1..=1 {
                for dc in -1..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = mv.to_row as isize + dr;
                    let nc = mv.to_col as isize + dc;
                    if !in_bounds(nr, nc) {
                        continue;
                    }
                    let idx = nr as usize * 9 + nc as usize;
                    if state.board[idx].is_some() {
                        continue;
                    }
                    state.add_board_hazard(
                        nr as usize,
                        nc as usize,
                        state.side_to_move,
                        hazard_type,
                        duration_turns,
                    );
                    applied = true;
                }
            }
            applied
        }
        _ => false,
    }
}

fn execute_copy_ability(state: &mut SearchState, mv: &EngineMove) -> bool {
    match (
        mv.piece_code.as_str(),
        state.side_to_move,
        mv.to_row as usize,
        mv.to_col as usize,
    ) {
        ("鏡", enemy_side, row, col) => has_front_enemy(state, row, col, enemy_side),
        _ => has_adjacent_ally(
            state,
            mv.to_row as usize,
            mv.to_col as usize,
            state.side_to_move.opposite(),
        ),
    }
}

fn execute_copy_ability_self(effect: &SkillEffect) -> bool {
    effect
        .params
        .get("copySource")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
        || effect
            .params
            .get("copyMode")
            .and_then(Value::as_str)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
}

fn execute_capture_with_leap(effect: &SkillEffect) -> bool {
    effect
        .params
        .get("mode")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

fn execute_linked_action(state: &mut SearchState, mv: &EngineMove, effect: &SkillEffect) -> bool {
    match (
        effect.target.group.as_str(),
        effect.target.selector.as_str(),
    ) {
        ("line", "same_row_ally") => has_same_row_ally(
            state,
            mv.to_row as usize,
            mv.to_col as usize,
            state.side_to_move.opposite(),
        ),
        ("adjacent", "adjacent_ally") => has_adjacent_ally(
            state,
            mv.to_row as usize,
            mv.to_col as usize,
            state.side_to_move.opposite(),
        ),
        _ => false,
    }
}

fn execute_disable_piece(state: &mut SearchState, mv: &EngineMove, effect: &SkillEffect) -> bool {
    if effect
        .params
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        return false;
    }

    match (
        effect.target.group.as_str(),
        effect.target.selector.as_str(),
    ) {
        ("adjacent", "adjacent_enemy") => has_adjacent_enemy(
            state,
            mv.to_row as usize,
            mv.to_col as usize,
            state.side_to_move,
        ),
        ("adjacent", "adjacent_ally") => has_adjacent_ally(
            state,
            mv.to_row as usize,
            mv.to_col as usize,
            state.side_to_move.opposite(),
        ),
        _ => false,
    }
}

fn execute_capture_constraint(effect: &SkillEffect) -> bool {
    !effect
        .params
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
}

fn execute_multi_capture(state: &mut SearchState, mv: &EngineMove, effect: &SkillEffect) -> bool {
    match (
        effect.target.group.as_str(),
        effect.target.selector.as_str(),
    ) {
        ("adjacent", "adjacent_enemy") => {
            let mut removed = false;
            for dr in -1..=1 {
                for dc in -1..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = mv.to_row as isize + dr;
                    let nc = mv.to_col as isize + dc;
                    if !in_bounds(nr, nc) {
                        continue;
                    }
                    let idx = nr as usize * 9 + nc as usize;
                    let Some(piece) = state.board[idx] else {
                        continue;
                    };
                    if piece.side != state.side_to_move || piece.kind == PieceKind::King {
                        continue;
                    }
                    state.board[idx] = None;
                    removed = true;
                }
            }
            removed
        }
        ("line", "front_enemy") => {
            let mover_side = state.side_to_move.opposite();
            let front_row = match mover_side {
                Side::Black => mv.to_row as isize - 1,
                Side::White => mv.to_row as isize + 1,
            };
            let front_col = mv.to_col as isize;
            if !in_bounds(front_row, front_col) {
                return false;
            }
            let idx = front_row as usize * 9 + front_col as usize;
            let Some(piece) = state.board[idx] else {
                return false;
            };
            if piece.side != state.side_to_move || piece.kind == PieceKind::King {
                return false;
            }
            state.board[idx] = None;
            true
        }
        _ => false,
    }
}

fn execute_seal_skill(state: &mut SearchState, mv: &EngineMove, effect: &SkillEffect) -> bool {
    if effect
        .params
        .get("statusType")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        return false;
    }

    has_adjacent_enemy(
        state,
        mv.to_row as usize,
        mv.to_col as usize,
        state.side_to_move,
    )
}

fn execute_defense_or_immunity(
    state: &mut SearchState,
    mv: &EngineMove,
    effect: &SkillEffect,
) -> bool {
    let Some(mode) = effect
        .params
        .get("mode")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let duration_turns = effect_duration_turns(effect);
    let actor_side = state.side_to_move.opposite();

    let immediate_applied = match (
        effect.target.group.as_str(),
        effect.target.selector.as_str(),
    ) {
        ("self", "self_piece") => true,
        ("adjacent", "adjacent_ally") => {
            has_adjacent_ally(state, mv.to_row as usize, mv.to_col as usize, actor_side)
        }
        _ => false,
    };

    let stateful_applied = match (
        effect.target.group.as_str(),
        effect.target.selector.as_str(),
    ) {
        ("self", "self_piece") => {
            state.add_piece_defense(
                mv.to_row as usize,
                mv.to_col as usize,
                actor_side,
                mode,
                duration_turns,
            );
            true
        }
        ("adjacent", "adjacent_ally") => {
            let mut applied = false;
            for dr in -1..=1 {
                for dc in -1..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = mv.to_row as isize + dr;
                    let nc = mv.to_col as isize + dc;
                    if !in_bounds(nr, nc) {
                        continue;
                    }
                    let idx = nr as usize * 9 + nc as usize;
                    let Some(piece) = state.board[idx] else {
                        continue;
                    };
                    if piece.side != actor_side {
                        continue;
                    }
                    state.add_piece_defense(
                        nr as usize,
                        nc as usize,
                        piece.side,
                        mode,
                        duration_turns,
                    );
                    applied = true;
                }
            }
            applied
        }
        _ => false,
    };

    immediate_applied || stateful_applied
}

fn execute_remove_piece(state: &mut SearchState, mv: &EngineMove) -> bool {
    let enemy_side = state.side_to_move;
    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = mv.to_row as isize + dr;
            let nc = mv.to_col as isize + dc;
            if !in_bounds(nr, nc) {
                continue;
            }
            let idx = nr as usize * 9 + nc as usize;
            let Some(piece) = state.board[idx] else {
                continue;
            };
            if piece.side != enemy_side || piece.kind == PieceKind::King {
                continue;
            }
            state.board[idx] = None;
            return true;
        }
    }

    false
}

fn execute_remove_enemy_hand_piece(state: &mut SearchState) -> bool {
    decrement_first_hand_piece(&mut state.hands.standard[side_index(state.side_to_move)])
}

fn execute_destroy_hand_piece(state: &mut SearchState) -> bool {
    execute_remove_enemy_hand_piece(state)
}

fn execute_send_to_hand(state: &mut SearchState, mv: &EngineMove) -> bool {
    let enemy_side = state.side_to_move;
    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = mv.to_row as isize + dr;
            let nc = mv.to_col as isize + dc;
            if !in_bounds(nr, nc) {
                continue;
            }
            let idx = nr as usize * 9 + nc as usize;
            let Some(piece) = state.board[idx] else {
                continue;
            };
            if piece.side != enemy_side || piece.kind == PieceKind::King {
                continue;
            }
            state.board[idx] = None;
            state.hands.add_piece(enemy_side, &piece.kind, 1);
            return true;
        }
    }

    false
}

fn execute_return_to_hand(effect: &SkillEffect) -> bool {
    effect
        .params
        .get("handOwner")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

fn execute_gain_piece(state: &mut SearchState, effect: &SkillEffect) -> bool {
    if effect
        .params
        .get("gainPieceChar")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
    {
        return true;
    }

    let kind = effect
        .params
        .get("gainPieceCode")
        .and_then(Value::as_str)
        .and_then(piece_kind_from_code);
    let Some(kind) = kind else {
        return false;
    };
    let actor_side = state.side_to_move.opposite();
    state.hands.add_piece(actor_side, &kind, 1);
    true
}

fn execute_substitute(effect: &SkillEffect) -> bool {
    effect
        .params
        .get("mode")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

fn execute_revive(state: &mut SearchState, mv: &EngineMove) -> bool {
    has_adjacent_ally(
        state,
        mv.to_row as usize,
        mv.to_col as usize,
        state.side_to_move.opposite(),
    )
}

fn execute_forced_push_away(
    state: &mut SearchState,
    mv: &EngineMove,
    effect: &SkillEffect,
) -> bool {
    if effect.params.get("movementRule").and_then(Value::as_str) != Some("push_away") {
        return false;
    }

    push_adjacent_enemy_pieces_away(
        state,
        mv.to_row as isize,
        mv.to_col as isize,
        state.side_to_move.opposite(),
    )
}

fn execute_summon_piece(state: &mut SearchState, mv: &EngineMove, effect: &SkillEffect) -> bool {
    let origin = (mv.to_row as isize, mv.to_col as isize);
    let summon_piece = effect.params.get("summonPiece").and_then(Value::as_str);
    let summon_piece_char = effect
        .params
        .get("summonPieceChar")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());

    let source_piece = if summon_piece == Some("self_clone") {
        let source_idx = origin.0 as usize * 9 + origin.1 as usize;
        let Some(source_piece) = state.board[source_idx] else {
            return false;
        };
        Some(source_piece)
    } else {
        None
    };

    if source_piece.is_none() && summon_piece_char.is_none() {
        return false;
    }

    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = origin.0 + dr;
            let nc = origin.1 + dc;
            if !in_bounds(nr, nc) {
                continue;
            }
            let idx = nr as usize * 9 + nc as usize;
            if state.board[idx].is_some() {
                continue;
            }
            if let Some(source_piece) = source_piece {
                state.board[idx] = Some(source_piece);
            }
            return true;
        }
    }

    false
}

fn execute_inherit_ability(effect: &SkillEffect) -> bool {
    effect
        .params
        .get("inheritSource")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

fn execute_transform_piece(state: &mut SearchState, effect: &SkillEffect) -> bool {
    let actor_side = state.side_to_move.opposite();
    let from_piece_code = effect.params.get("fromPieceCode").and_then(Value::as_str);

    has_matching_ally_piece(state, actor_side, from_piece_code)
}

fn execute_transform_self(effect: &SkillEffect) -> bool {
    effect
        .params
        .get("toPieceChar")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
        || effect
            .params
            .get("toPieceCode")
            .and_then(Value::as_str)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
        || effect
            .params
            .get("inheritMode")
            .and_then(Value::as_str)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
}

fn execute_transform_adjacent_enemy(
    state: &mut SearchState,
    mv: &EngineMove,
    effect: &SkillEffect,
) -> bool {
    if let Some(to_kind) = effect
        .params
        .get("toPieceCode")
        .and_then(Value::as_str)
        .and_then(piece_kind_from_code)
    {
        let enemy_side = state.side_to_move;
        for dr in -1..=1 {
            for dc in -1..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let nr = mv.to_row as isize + dr;
                let nc = mv.to_col as isize + dc;
                if !in_bounds(nr, nc) {
                    continue;
                }
                let idx = nr as usize * 9 + nc as usize;
                let Some(piece) = state.board[idx] else {
                    continue;
                };
                if piece.side != enemy_side || piece.kind == PieceKind::King {
                    continue;
                }
                state.board[idx] = Some(Piece {
                    side: piece.side,
                    kind: to_kind,
                    promoted: false,
                    sfen_char: sfen_char_from_piece_kind(&to_kind).unwrap_or(piece.sfen_char),
                });
                return true;
            }
        }
        return false;
    }

    effect
        .params
        .get("toPieceChar")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .is_some()
        && has_transformable_adjacent_enemy(
            state,
            mv.to_row as usize,
            mv.to_col as usize,
            state.side_to_move,
        )
}

fn execute_script_hook(state: &mut SearchState, mv: &EngineMove, hook: &str) -> bool {
    match hook {
        "reflect_until_blocked" => execute_reflect_until_blocked(state, mv),
        "bomb_explosion_push" => execute_bomb_explosion_push(state, mv),
        "safe_room_king_relocation" => execute_safe_room_king_relocation(state),
        "fixed_next_turn_restriction" => execute_fixed_next_turn_restriction(),
        "edge_line_imprison" => execute_edge_line_imprison(state),
        "escape_king_follow" => execute_escape_king_follow(state, mv),
        _ => false,
    }
}

fn execute_reflect_until_blocked(state: &mut SearchState, mv: &EngineMove) -> bool {
    let (Some(from_row), Some(from_col)) = (mv.from_row, mv.from_col) else {
        return false;
    };

    let mut dr = (mv.to_row - from_row).signum();
    let mut dc = (mv.to_col - from_col).signum();
    if dr == 0 && dc == 0 {
        return false;
    }

    let mover_side = state.side_to_move.opposite();
    let mut current_row = mv.to_row;
    let mut current_col = mv.to_col;
    let mut moved = false;

    for _ in 0..16 {
        let mut next_row = current_row + dr;
        let mut next_col = current_col + dc;
        if !(0..=8).contains(&next_row) {
            dr *= -1;
            next_row = current_row + dr;
        }
        if !(0..=8).contains(&next_col) {
            dc *= -1;
            next_col = current_col + dc;
        }
        if !(0..=8).contains(&next_row) || !(0..=8).contains(&next_col) {
            break;
        }

        let from_idx = current_row as usize * 9 + current_col as usize;
        let to_idx = next_row as usize * 9 + next_col as usize;
        let Some(piece) = state.board[from_idx] else {
            break;
        };

        match state.board[to_idx] {
            Some(other) if other.side == mover_side => break,
            Some(other) => {
                state.board[to_idx] = Some(piece);
                state.board[from_idx] = None;
                if other.kind != PieceKind::King {
                    state.hands.add_piece(mover_side, &other.kind, 1);
                }
                moved = true;
                break;
            }
            None => {
                state.board[to_idx] = Some(piece);
                state.board[from_idx] = None;
                current_row = next_row;
                current_col = next_col;
                moved = true;
            }
        }
    }

    moved
}

fn execute_bomb_explosion_push(state: &mut SearchState, mv: &EngineMove) -> bool {
    push_adjacent_enemy_pieces_away(
        state,
        mv.to_row as isize,
        mv.to_col as isize,
        state.side_to_move.opposite(),
    )
}

fn execute_safe_room_king_relocation(state: &mut SearchState) -> bool {
    let mover_side = state.side_to_move.opposite();
    let (target_row, target_col) = match mover_side {
        Side::Black => (8usize, 4usize),
        Side::White => (0usize, 4usize),
    };
    let target_idx = target_row * 9 + target_col;
    if state.board[target_idx].is_some() {
        return false;
    }

    let Some((king_row, king_col)) = find_king_position(state, mover_side) else {
        return false;
    };
    let king_idx = king_row * 9 + king_col;
    let Some(king_piece) = state.board[king_idx] else {
        return false;
    };

    state.board[king_idx] = None;
    state.board[target_idx] = Some(king_piece);
    true
}

fn execute_fixed_next_turn_restriction() -> bool {
    true
}

fn execute_edge_line_imprison(state: &mut SearchState) -> bool {
    has_piece_on_board_edge(state, state.side_to_move)
}

fn execute_escape_king_follow(state: &mut SearchState, mv: &EngineMove) -> bool {
    let (Some(from_row), Some(from_col)) = (mv.from_row, mv.from_col) else {
        return false;
    };

    let dr = (mv.to_row - from_row).signum();
    let dc = (mv.to_col - from_col).signum();
    if dr == 0 && dc == 0 {
        return false;
    }

    let mover_side = state.side_to_move.opposite();
    let Some((king_row, king_col)) = find_king_position(state, mover_side) else {
        return false;
    };
    let target_row = king_row as isize + dr as isize;
    let target_col = king_col as isize + dc as isize;
    if !in_bounds(target_row, target_col) {
        return false;
    }

    let target_idx = target_row as usize * 9 + target_col as usize;
    if state.board[target_idx].is_some() {
        return false;
    }

    let king_idx = king_row * 9 + king_col;
    let Some(king_piece) = state.board[king_idx] else {
        return false;
    };
    state.board[king_idx] = None;
    state.board[target_idx] = Some(king_piece);
    true
}

fn push_adjacent_enemy_pieces_away(
    state: &mut SearchState,
    origin_row: isize,
    origin_col: isize,
    mover_side: Side,
) -> bool {
    let mut pushed = false;

    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let enemy_r = origin_row + dr;
            let enemy_c = origin_col + dc;
            if !in_bounds(enemy_r, enemy_c) {
                continue;
            }
            let enemy_idx = enemy_r as usize * 9 + enemy_c as usize;
            let Some(enemy_piece) = state.board[enemy_idx] else {
                continue;
            };
            if enemy_piece.side == mover_side {
                continue;
            }

            let dest_r = enemy_r + dr;
            let dest_c = enemy_c + dc;
            if !in_bounds(dest_r, dest_c) {
                continue;
            }
            let dest_idx = dest_r as usize * 9 + dest_c as usize;
            if state.board[dest_idx].is_some() {
                continue;
            }

            state.board[dest_idx] = Some(enemy_piece);
            state.board[enemy_idx] = None;
            pushed = true;
        }
    }

    pushed
}

fn find_king_position(state: &SearchState, side: Side) -> Option<(usize, usize)> {
    for row in 0..9 {
        for col in 0..9 {
            let idx = row * 9 + col;
            let Some(piece) = state.board[idx] else {
                continue;
            };
            if piece.side == side && piece.kind == PieceKind::King {
                return Some((row, col));
            }
        }
    }
    None
}

fn has_piece_on_board_edge(state: &SearchState, side: Side) -> bool {
    for row in 0..9 {
        for col in 0..9 {
            if row != 0 && row != 8 && col != 0 && col != 8 {
                continue;
            }
            let idx = row * 9 + col;
            if state.board[idx]
                .map(|piece| piece.side == side)
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

fn has_adjacent_enemy(state: &SearchState, row: usize, col: usize, enemy_side: Side) -> bool {
    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = row as isize + dr;
            let nc = col as isize + dc;
            if !in_bounds(nr, nc) {
                continue;
            }
            let idx = nr as usize * 9 + nc as usize;
            if state.board[idx]
                .map(|piece| piece.side == enemy_side)
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

fn has_adjacent_empty(state: &SearchState, row: usize, col: usize) -> bool {
    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = row as isize + dr;
            let nc = col as isize + dc;
            if !in_bounds(nr, nc) {
                continue;
            }
            let idx = nr as usize * 9 + nc as usize;
            if state.board[idx].is_none() {
                return true;
            }
        }
    }
    false
}

fn has_front_enemy(state: &SearchState, row: usize, col: usize, enemy_side: Side) -> bool {
    let mover_side = enemy_side.opposite();
    let front_row = match mover_side {
        Side::Black => row as isize - 1,
        Side::White => row as isize + 1,
    };
    let front_col = col as isize;
    if !in_bounds(front_row, front_col) {
        return false;
    }
    let idx = front_row as usize * 9 + front_col as usize;
    state.board[idx]
        .map(|piece| piece.side == enemy_side)
        .unwrap_or(false)
}

fn has_adjacent_ally(state: &SearchState, row: usize, col: usize, side: Side) -> bool {
    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = row as isize + dr;
            let nc = col as isize + dc;
            if !in_bounds(nr, nc) {
                continue;
            }
            let idx = nr as usize * 9 + nc as usize;
            if state.board[idx]
                .map(|piece| piece.side == side)
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

fn has_same_row_ally(state: &SearchState, row: usize, col: usize, side: Side) -> bool {
    for candidate_col in 0..9 {
        if candidate_col == col {
            continue;
        }
        let idx = row * 9 + candidate_col;
        let Some(piece) = state.board[idx] else {
            continue;
        };
        if piece.side == side {
            return true;
        }
    }
    false
}

fn decrement_first_hand_piece(hand: &mut [u8; 7]) -> bool {
    for count in hand.iter_mut() {
        if *count > 0 {
            *count -= 1;
            return true;
        }
    }
    false
}

fn effect_duration_turns(effect: &SkillEffect) -> u8 {
    effect
        .params
        .get("durationTurns")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .clamp(1, u8::MAX as u64) as u8
}

fn has_matching_ally_piece(
    state: &SearchState,
    side: Side,
    piece_code_filter: Option<&str>,
) -> bool {
    let piece_kind_filter = piece_code_filter.and_then(piece_kind_from_code);
    state.board.iter().flatten().any(|piece| {
        piece.side == side
            && piece_kind_filter
                .map(|kind| piece.kind == kind)
                .unwrap_or(true)
    })
}

fn has_transformable_adjacent_enemy(
    state: &SearchState,
    row: usize,
    col: usize,
    enemy_side: Side,
) -> bool {
    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = row as isize + dr;
            let nc = col as isize + dc;
            if !in_bounds(nr, nc) {
                continue;
            }
            let idx = nr as usize * 9 + nc as usize;
            if state.board[idx]
                .map(|piece| piece.side == enemy_side && piece.kind != PieceKind::King)
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

fn evaluate_state_for_side(
    state: &SearchState,
    perspective: Side,
    cfg: &EngineConfig,
    rules: &RuntimeRules,
) -> i32 {
    let mut oriented = state.clone();
    oriented.side_to_move = perspective;
    evaluate_state(&oriented, cfg, rules)
}

fn skill_trace_tactical_bonus(trace: &SkillExecutionTrace) -> i32 {
    let forced_move_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "forced_move")
        .count() as i32
        * 40;
    let modify_movement_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "modify_movement")
        .count() as i32
        * 12;
    let apply_status_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "apply_status")
        .count() as i32
        * 14;
    let board_hazard_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "board_hazard")
        .count() as i32
        * 10;
    let copy_ability_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "copy_ability")
        .count() as i32
        * 12;
    let linked_action_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "linked_action")
        .count() as i32
        * 14;
    let disable_piece_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "disable_piece")
        .count() as i32
        * 12;
    let capture_constraint_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "capture_constraint")
        .count() as i32
        * 10;
    let multi_capture_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "multi_capture")
        .count() as i32
        * 24;
    let seal_skill_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "seal_skill")
        .count() as i32
        * 12;
    let defense_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "defense_or_immunity")
        .count() as i32
        * 12;
    let remove_piece_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "remove_piece")
        .count() as i32
        * 22;
    let destroy_hand_piece_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "destroy_hand_piece")
        .count() as i32
        * 18;
    let send_to_hand_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "send_to_hand")
        .count() as i32
        * 18;
    let return_to_hand_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "return_to_hand")
        .count() as i32
        * 16;
    let extra_action_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "extra_action")
        .count() as i32
        * 28;
    let summon_piece_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "summon_piece")
        .count() as i32
        * 18;
    let transform_piece_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "transform_piece")
        .count() as i32
        * 16;
    let gain_piece_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "gain_piece")
        .count() as i32
        * 16;
    let revive_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "revive")
        .count() as i32
        * 14;
    let capture_with_leap_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "capture_with_leap")
        .count() as i32
        * 16;
    let substitute_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "substitute")
        .count() as i32
        * 14;
    let inherit_ability_bonus = trace
        .applied_effects
        .iter()
        .filter(|effect| effect.as_str() == "inherit_ability")
        .count() as i32
        * 16;
    let script_hook_bonus = trace.applied_hooks.len() as i32 * 25;
    forced_move_bonus
        + modify_movement_bonus
        + apply_status_bonus
        + board_hazard_bonus
        + copy_ability_bonus
        + linked_action_bonus
        + disable_piece_bonus
        + capture_constraint_bonus
        + multi_capture_bonus
        + seal_skill_bonus
        + defense_bonus
        + remove_piece_bonus
        + destroy_hand_piece_bonus
        + send_to_hand_bonus
        + return_to_hand_bonus
        + extra_action_bonus
        + summon_piece_bonus
        + transform_piece_bonus
        + gain_piece_bonus
        + revive_bonus
        + capture_with_leap_bonus
        + substitute_bonus
        + inherit_ability_bonus
        + script_hook_bonus
}

fn matches_piece_code(definition: &SkillDefinition, move_piece_code: &str) -> bool {
    use crate::engine::piece_mapping::kanji_to_code;
    definition.piece_chars.iter().any(|piece| {
        piece == move_piece_code
            || piece.eq_ignore_ascii_case(move_piece_code)
            // catalog の pieceChars は漢字 (砲, 竜, …) を使用するため、
            // kanji_to_code で displayCode (HOU, RYU, …) に変換して照合する
            || kanji_to_code(piece)
                .map_or(false, |code| code.eq_ignore_ascii_case(move_piece_code))
    })
}

fn in_bounds(row: isize, col: isize) -> bool {
    (0..=8).contains(&row) && (0..=8).contains(&col)
}

fn is_orthogonal_step(mv: &EngineMove, distance: i32) -> bool {
    let (Some(from_row), Some(from_col)) = (mv.from_row, mv.from_col) else {
        return false;
    };
    let dr = (mv.to_row - from_row).abs();
    let dc = (mv.to_col - from_col).abs();
    (dr == distance && dc == 0) || (dr == 0 && dc == distance)
}

fn is_diagonal_step(mv: &EngineMove, distance: i32) -> bool {
    let (Some(from_row), Some(from_col)) = (mv.from_row, mv.from_col) else {
        return false;
    };
    let dr = (mv.to_row - from_row).abs();
    let dc = (mv.to_col - from_col).abs();
    dr == distance && dc == distance
}

fn is_backward_step(mv: &EngineMove, mover_side: Side, distance: i32) -> bool {
    let (Some(from_row), Some(from_col)) = (mv.from_row, mv.from_col) else {
        return false;
    };
    let dr = mv.to_row - from_row;
    let dc = (mv.to_col - from_col).abs();
    match mover_side {
        Side::Black => dr == distance && dc == 0,
        Side::White => dr == -distance && dc == 0,
    }
}

fn reaches_board_edge(mv: &EngineMove) -> bool {
    mv.to_row == 0 || mv.to_row == 8 || mv.to_col == 0 || mv.to_col == 8
}
