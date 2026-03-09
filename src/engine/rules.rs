use crate::engine::types::{RuntimeRules, VectorRule};

pub fn parse_runtime_rules(board_state: &serde_json::Value) -> RuntimeRules {
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

    rules
}
