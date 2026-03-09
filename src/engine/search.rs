use crate::engine::config::EngineConfig;
use crate::engine::types::EngineMove;
use crate::engine::types::{
    hand_index, is_promotion_zone, must_promote, piece_base_value, piece_code, piece_promotable,
    side_index, GenMove, Piece, PieceKind, RuntimeRules, SearchState, Side,
};
use std::time::Instant;

pub fn search_with_iterative_deepening(
    state: &SearchState,
    cfg: &EngineConfig,
    rules: &RuntimeRules,
    start: Instant,
) -> (Vec<EngineMove>, Vec<(usize, i32)>, u64, u32) {
    let mut nodes = 0u64;
    let root = generate_legal_moves(state, rules, true);
    let root_inputs: Vec<EngineMove> = root.iter().map(to_move_input).collect();
    if root.is_empty() {
        return (Vec::new(), Vec::new(), 0, 1);
    }

    let mut last_scores: Vec<i32> = root
        .iter()
        .map(|mv| {
            let next = apply_move(state, mv);
            -evaluate_state(&next, cfg, rules)
        })
        .collect();
    let mut reached_depth = 1;

    for depth in 2..=cfg.max_depth {
        if nodes >= cfg.max_nodes as u64 || start.elapsed().as_millis() as u32 >= cfg.time_limit_ms
        {
            break;
        }
        let mut depth_scores = Vec::with_capacity(root.len());
        for mv in &root {
            if nodes >= cfg.max_nodes as u64
                || start.elapsed().as_millis() as u32 >= cfg.time_limit_ms
            {
                break;
            }
            let next = apply_move(state, mv);
            let score = -negamax(
                &next,
                depth - 1,
                -30000,
                30000,
                cfg,
                rules,
                start,
                &mut nodes,
            );
            depth_scores.push(score);
        }
        if depth_scores.len() == root.len() {
            last_scores = depth_scores;
            reached_depth = depth;
        } else {
            break;
        }
    }

    let scored = last_scores.into_iter().enumerate().collect();
    (
        root_inputs,
        scored,
        nodes.max(root.len() as u64),
        reached_depth,
    )
}

fn negamax(
    state: &SearchState,
    depth: u32,
    mut alpha: i32,
    beta: i32,
    cfg: &EngineConfig,
    rules: &RuntimeRules,
    start: Instant,
    nodes: &mut u64,
) -> i32 {
    if depth == 0
        || *nodes >= cfg.max_nodes as u64
        || start.elapsed().as_millis() as u32 >= cfg.time_limit_ms
    {
        return evaluate_state(state, cfg, rules);
    }
    *nodes += 1;

    let moves = generate_legal_moves(state, rules, true);
    if moves.is_empty() {
        return -29000 + depth as i32;
    }

    let mut best = -30000;
    for mv in &moves {
        let next = apply_move(state, mv);
        let score = -negamax(&next, depth - 1, -beta, -alpha, cfg, rules, start, nodes);
        if score > best {
            best = score;
        }
        if score > alpha {
            alpha = score;
        }
        if alpha >= beta {
            break;
        }
    }
    best
}

fn evaluate_state(state: &SearchState, cfg: &EngineConfig, rules: &RuntimeRules) -> i32 {
    let mut material = 0.0;
    let mut mobility = 0.0;
    let mut center = 0.0;

    for row in 0..9 {
        for col in 0..9 {
            if let Some(p) = state.board[row * 9 + col] {
                let v = piece_base_value(p.kind) as f64 + if p.promoted { 80.0 } else { 0.0 };
                let s = if p.side == state.side_to_move {
                    1.0
                } else {
                    -1.0
                };
                material += v * s;
                let bonus = rules
                    .eval_bonus_by_piece
                    .get(piece_code(p.kind))
                    .copied()
                    .unwrap_or(0) as f64;
                material += bonus * s;
                center += (8.0 - ((row as i32 - 4).abs() + (col as i32 - 4).abs()) as f64) * s;
            }
        }
    }
    mobility += generate_legal_moves(state, rules, false).len() as f64;

    (material * cfg.eval_material_weight
        + center * cfg.eval_position_weight
        + mobility * cfg.eval_mobility_weight) as i32
}

pub(crate) fn generate_legal_moves(
    state: &SearchState,
    rules: &RuntimeRules,
    enforce_uchifuzume: bool,
) -> Vec<GenMove> {
    let pseudo = generate_pseudo_moves(state, rules);
    let mut legal = Vec::new();
    for mv in pseudo {
        let next = apply_move(state, &mv);
        if is_in_check(&next, state.side_to_move, rules) {
            continue;
        }
        if enforce_uchifuzume && is_uchi_fuzume(state, &mv, rules) {
            continue;
        }
        legal.push(mv);
    }
    legal
}

fn generate_pseudo_moves(state: &SearchState, rules: &RuntimeRules) -> Vec<GenMove> {
    let mut moves = Vec::new();
    for row in 0..9 {
        for col in 0..9 {
            let idx = row * 9 + col;
            let Some(piece) = state.board[idx] else {
                continue;
            };
            if piece.side != state.side_to_move {
                continue;
            }
            gen_piece_moves(state, row, col, piece, rules, &mut moves);
        }
    }
    gen_drop_moves(state, &mut moves);
    moves
}

fn gen_piece_moves(
    state: &SearchState,
    row: usize,
    col: usize,
    piece: Piece,
    rules: &RuntimeRules,
    out: &mut Vec<GenMove>,
) {
    let fwd = if piece.side == Side::Black { -1 } else { 1 };
    let gold_dirs = [(fwd, 0), (fwd, -1), (fwd, 1), (0, -1), (0, 1), (-fwd, 0)];
    let king_dirs = [
        (-1, -1),
        (-1, 0),
        (-1, 1),
        (0, -1),
        (0, 1),
        (1, -1),
        (1, 0),
        (1, 1),
    ];
    let silver_dirs = [(fwd, -1), (fwd, 0), (fwd, 1), (-fwd, -1), (-fwd, 1)];

    let mut push_step = |dr: i32, dc: i32, slide: bool| {
        let mut r = row as i32 + dr;
        let mut c = col as i32 + dc;
        while (0..=8).contains(&r) && (0..=8).contains(&c) {
            let tidx = r as usize * 9 + c as usize;
            if let Some(tp) = state.board[tidx] {
                if tp.side == piece.side {
                    break;
                }
                push_promote_variants(
                    out,
                    make_gen_move((row, col), (r as usize, c as usize), piece, Some(tp)),
                );
                break;
            }
            push_promote_variants(
                out,
                make_gen_move((row, col), (r as usize, c as usize), piece, None),
            );
            if !slide {
                break;
            }
            r += dr;
            c += dc;
        }
    };

    if piece.promoted
        && matches!(
            piece.kind,
            PieceKind::Pawn | PieceKind::Lance | PieceKind::Knight | PieceKind::Silver
        )
    {
        for (dr, dc) in gold_dirs {
            push_step(dr, dc, false);
        }
        return;
    }

    match piece.kind {
        PieceKind::Pawn => push_step(fwd, 0, false),
        PieceKind::Lance => push_step(fwd, 0, true),
        PieceKind::Knight => {
            push_step(fwd * 2, -1, false);
            push_step(fwd * 2, 1, false);
        }
        PieceKind::Silver => {
            for (dr, dc) in silver_dirs {
                push_step(dr, dc, false);
            }
        }
        PieceKind::Gold => {
            for (dr, dc) in gold_dirs {
                push_step(dr, dc, false);
            }
        }
        PieceKind::Bishop => {
            for (dr, dc) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
                push_step(dr, dc, true);
            }
            if piece.promoted {
                for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    push_step(dr, dc, false);
                }
            }
        }
        PieceKind::Rook => {
            for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                push_step(dr, dc, true);
            }
            if piece.promoted {
                for (dr, dc) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
                    push_step(dr, dc, false);
                }
            }
        }
        PieceKind::King => {
            for (dr, dc) in king_dirs {
                push_step(dr, dc, false);
            }
        }
    }

    if let Some(extra) = rules.extra_vectors_by_piece.get(piece_code(piece.kind)) {
        for v in extra {
            push_step(v.dr, v.dc, v.slide);
        }
    }
}

fn make_gen_move(
    from: (usize, usize),
    to: (usize, usize),
    piece: Piece,
    capture: Option<Piece>,
) -> GenMove {
    let can_promote = piece_promotable(piece.kind)
        && !piece.promoted
        && (is_promotion_zone(piece.side, from.0) || is_promotion_zone(piece.side, to.0));
    GenMove {
        from: Some(from),
        to,
        piece,
        promote: can_promote && must_promote(piece, to.0),
        capture,
        drop: None,
    }
}

pub(crate) fn apply_move(state: &SearchState, mv: &GenMove) -> SearchState {
    let mut next = state.clone();
    if let Some((fr, fc)) = mv.from {
        let from_idx = fr * 9 + fc;
        let to_idx = mv.to.0 * 9 + mv.to.1;
        let mut piece = next.board[from_idx].expect("piece must exist");
        next.board[from_idx] = None;
        if let Some(cap) = next.board[to_idx] {
            if cap.kind != PieceKind::King {
                if let Some(hidx) = hand_index(cap.kind) {
                    next.hands[side_index(piece.side)][hidx] =
                        next.hands[side_index(piece.side)][hidx].saturating_add(1);
                }
            }
        }
        if mv.promote {
            piece.promoted = true;
        }
        next.board[to_idx] = Some(piece);
    } else if let Some(kind) = mv.drop {
        let to_idx = mv.to.0 * 9 + mv.to.1;
        next.board[to_idx] = Some(Piece {
            side: state.side_to_move,
            kind,
            promoted: false,
        });
        if let Some(hidx) = hand_index(kind) {
            next.hands[side_index(state.side_to_move)][hidx] =
                next.hands[side_index(state.side_to_move)][hidx].saturating_sub(1);
        }
    }
    next.side_to_move = state.side_to_move.opposite();
    next
}

fn to_move_input(mv: &GenMove) -> EngineMove {
    EngineMove {
        from_row: mv.from.map(|(r, _)| r as i32),
        from_col: mv.from.map(|(_, c)| c as i32),
        to_row: mv.to.0 as i32,
        to_col: mv.to.1 as i32,
        piece_code: piece_code(mv.piece.kind).to_string(),
        promote: mv.promote,
        drop_piece_code: mv.drop.map(|k| piece_code(k).to_string()),
        captured_piece_code: mv.capture.map(|p| piece_code(p.kind).to_string()),
        notation: None,
    }
}

fn push_promote_variants(out: &mut Vec<GenMove>, base: GenMove) {
    let mut pushed = false;
    if base.from.is_some() && piece_promotable(base.piece.kind) && !base.piece.promoted {
        if let Some((fr, _)) = base.from {
            if is_promotion_zone(base.piece.side, fr)
                || is_promotion_zone(base.piece.side, base.to.0)
            {
                let mut m = base.clone();
                m.promote = true;
                out.push(m);
                pushed = true;
                if must_promote(base.piece, base.to.0) {
                    return;
                }
            }
        }
    }
    if !pushed || !must_promote(base.piece, base.to.0) {
        let mut m = base;
        m.promote = false;
        out.push(m);
    }
}

fn gen_drop_moves(state: &SearchState, out: &mut Vec<GenMove>) {
    let h = &state.hands[side_index(state.side_to_move)];
    for kind in [
        PieceKind::Pawn,
        PieceKind::Lance,
        PieceKind::Knight,
        PieceKind::Silver,
        PieceKind::Gold,
        PieceKind::Bishop,
        PieceKind::Rook,
    ] {
        let Some(hidx) = hand_index(kind) else {
            continue;
        };
        if h[hidx] == 0 {
            continue;
        }
        for row in 0..9 {
            for col in 0..9 {
                let idx = row * 9 + col;
                if state.board[idx].is_some() {
                    continue;
                }
                if !drop_allowed(state, kind, row, col) {
                    continue;
                }
                out.push(GenMove {
                    from: None,
                    to: (row, col),
                    piece: Piece {
                        side: state.side_to_move,
                        kind,
                        promoted: false,
                    },
                    promote: false,
                    capture: None,
                    drop: Some(kind),
                });
            }
        }
    }
}

fn drop_allowed(state: &SearchState, kind: PieceKind, row: usize, col: usize) -> bool {
    match (state.side_to_move, kind) {
        (Side::Black, PieceKind::Pawn | PieceKind::Lance) if row == 0 => return false,
        (Side::White, PieceKind::Pawn | PieceKind::Lance) if row == 8 => return false,
        (Side::Black, PieceKind::Knight) if row <= 1 => return false,
        (Side::White, PieceKind::Knight) if row >= 7 => return false,
        _ => {}
    }
    if kind == PieceKind::Pawn && has_pawn_on_file(state, state.side_to_move, col) {
        return false;
    }
    true
}

fn has_pawn_on_file(state: &SearchState, side: Side, col: usize) -> bool {
    for row in 0..9 {
        if let Some(p) = state.board[row * 9 + col] {
            if p.side == side && p.kind == PieceKind::Pawn && !p.promoted {
                return true;
            }
        }
    }
    false
}

pub(crate) fn is_in_check(state: &SearchState, side: Side, rules: &RuntimeRules) -> bool {
    let king_pos = state.board.iter().enumerate().find_map(|(idx, p)| match p {
        Some(pc) if pc.side == side && pc.kind == PieceKind::King => Some((idx / 9, idx % 9)),
        _ => None,
    });
    let Some((kr, kc)) = king_pos else {
        return false;
    };
    attacks_square(state, side.opposite(), kr, kc, rules)
}

fn attacks_square(
    state: &SearchState,
    attacker: Side,
    tr: usize,
    tc: usize,
    rules: &RuntimeRules,
) -> bool {
    for row in 0..9 {
        for col in 0..9 {
            let Some(piece) = state.board[row * 9 + col] else {
                continue;
            };
            if piece.side != attacker {
                continue;
            }
            let mut list = Vec::new();
            gen_piece_moves(state, row, col, piece, rules, &mut list);
            if list.into_iter().any(|m| m.to == (tr, tc)) {
                return true;
            }
        }
    }
    false
}

fn is_uchi_fuzume(state: &SearchState, mv: &GenMove, rules: &RuntimeRules) -> bool {
    if mv.drop != Some(PieceKind::Pawn) {
        return false;
    }
    let next = apply_move(state, mv);
    if !is_in_check(&next, next.side_to_move, rules) {
        return false;
    }
    let replies = generate_legal_moves(&next, rules, false);
    replies.is_empty()
}
