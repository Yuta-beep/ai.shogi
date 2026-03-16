use crate::engine::skills::{
    builtin_skill_registry, parse_skill_definition_document_value, parse_skill_registry_value,
    validate_skill_definitions,
};
use crate::engine::types::{RuntimeRules, VectorRule};

pub fn parse_runtime_rules(board_state: &serde_json::Value) -> Result<RuntimeRules, String> {
    let mut rules = RuntimeRules::default();

    if let Some(m) = board_state
        .get("eval_bonus_by_piece")
        .and_then(|v| v.as_object())
    {
        for (k, v) in m {
            if let Some(cp) = v.as_i64() {
                rules
                    .eval_bonus_by_piece
                    .insert(k.to_ascii_uppercase(), cp as i32);
            }
        }
    }

    if let Some(m) = board_state
        .get("custom_move_vectors")
        .and_then(|v| v.as_object())
    {
        for (piece, arr) in m {
            let mut vecs = Vec::new();
            if let Some(items) = arr.as_array() {
                for item in items {
                    let dr = item.get("dr").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
                    let dc = item.get("dc").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
                    let slide = item.get("slide").and_then(|x| x.as_bool()).unwrap_or(false);
                    if dr != 0 || dc != 0 {
                        vecs.push(VectorRule { dr, dc, slide });
                    }
                }
            }
            if !vecs.is_empty() {
                rules
                    .extra_vectors_by_piece
                    .insert(piece.to_ascii_uppercase(), vecs);
            }
        }
    }

    if let Some(legacy_effects) = board_state.get("skill_effects").and_then(|v| v.as_array()) {
        rules.skill_runtime.legacy_skill_effects = legacy_effects.clone();
    }

    let registry = if let Some(registry_value) = board_state.get("skill_registry_v2") {
        parse_skill_registry_value(registry_value.clone()).map_err(|e| e.to_string())?
    } else {
        builtin_skill_registry().clone()
    };

    if let Some(definitions_value) = board_state.get("skill_definitions_v2") {
        let doc = parse_skill_definition_document_value(definitions_value.clone())
            .map_err(|e| e.to_string())?;
        validate_skill_definitions(&registry, &doc.definitions).map_err(|e| e.to_string())?;
        rules.skill_runtime.definitions = doc.definitions;
    }

    if board_state.get("skill_registry_v2").is_some()
        || board_state.get("skill_definitions_v2").is_some()
    {
        rules.skill_runtime.registry = Some(registry);
    }

    Ok(rules)
}
