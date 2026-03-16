use crate::engine::skills::SkillRuntimeRules;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineMove {
    pub from_row: Option<i32>,
    pub from_col: Option<i32>,
    pub to_row: i32,
    pub to_col: i32,
    pub piece_code: String,
    pub promote: bool,
    pub drop_piece_code: Option<String>,
    pub captured_piece_code: Option<String>,
    pub notation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Black,
    White,
}

impl Side {
    pub fn opposite(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }

    pub fn from_position_side(value: &str) -> Option<Self> {
        match value {
            "player" | "black" => Some(Self::Black),
            "enemy" | "white" => Some(Self::White),
            _ => None,
        }
    }

    pub fn as_position_side(self) -> &'static str {
        match self {
            Self::Black => "player",
            Self::White => "enemy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceKind {
    Pawn,
    Lance,
    Knight,
    Silver,
    Gold,
    Bishop,
    Rook,
    King,
}

#[derive(Debug, Clone, Copy)]
pub struct Piece {
    pub side: Side,
    pub kind: PieceKind,
    pub promoted: bool,
}

#[derive(Debug, Clone)]
pub struct PieceStatusState {
    pub row: usize,
    pub col: usize,
    pub side: Side,
    pub status_type: String,
    pub remaining_turns: u8,
}

#[derive(Debug, Clone)]
pub struct BoardHazardState {
    pub row: usize,
    pub col: usize,
    pub affects_side: Side,
    pub hazard_type: String,
    pub remaining_turns: u8,
}

#[derive(Debug, Clone)]
pub struct MovementModifierState {
    pub row: usize,
    pub col: usize,
    pub side: Side,
    pub movement_rule: String,
    pub remaining_turns: u8,
}

#[derive(Debug, Clone)]
pub struct PieceDefenseState {
    pub row: usize,
    pub col: usize,
    pub side: Side,
    pub mode: String,
    pub remaining_turns: u8,
}

#[derive(Debug, Clone)]
pub struct TurnStartRuleState {
    pub row: usize,
    pub col: usize,
    pub side: Side,
    pub rule_type: String,
    pub phase: u8,
}

#[derive(Debug, Clone, Default)]
pub struct SkillState {
    pub piece_statuses: Vec<PieceStatusState>,
    pub board_hazards: Vec<BoardHazardState>,
    pub movement_modifiers: Vec<MovementModifierState>,
    pub piece_defenses: Vec<PieceDefenseState>,
    pub turn_start_rules: Vec<TurnStartRuleState>,
}

#[derive(Debug, Clone)]
pub struct SearchState {
    pub board: [Option<Piece>; 81],
    pub side_to_move: Side,
    pub hands: [[u8; 7]; 2],
    pub skill_state: SkillState,
}

#[derive(Debug, Clone)]
pub struct GenMove {
    pub from: Option<(usize, usize)>,
    pub to: (usize, usize),
    pub piece: Piece,
    pub promote: bool,
    pub capture: Option<Piece>,
    pub drop: Option<PieceKind>,
}

#[derive(Debug, Clone, Copy)]
pub struct VectorRule {
    pub dr: i32,
    pub dc: i32,
    pub slide: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeRules {
    pub extra_vectors_by_piece: HashMap<String, Vec<VectorRule>>,
    pub eval_bonus_by_piece: HashMap<String, i32>,
    pub skill_runtime: SkillRuntimeRules,
}

impl SearchState {
    pub fn from_sfen(sfen: &str) -> Result<Self, String> {
        let mut parts = sfen.split_whitespace();
        let board_part = parts.next().ok_or("missing board")?;
        let side_part = parts.next().ok_or("missing side")?;
        let hands_part = parts.next().unwrap_or("-");

        let mut board: [Option<Piece>; 81] = [None; 81];
        for (row, rank) in board_part.split('/').enumerate() {
            if row >= 9 {
                return Err("too many ranks".to_string());
            }
            let mut col = 0usize;
            let mut chars = rank.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch.is_ascii_digit() {
                    col += ch.to_digit(10).ok_or("invalid digit")? as usize;
                    continue;
                }
                let promoted = if ch == '+' { true } else { false };
                let pch = if promoted {
                    chars.next().ok_or("invalid promoted piece")?
                } else {
                    ch
                };
                if col >= 9 {
                    return Err("column overflow".to_string());
                }
                let side = if pch.is_ascii_uppercase() {
                    Side::Black
                } else {
                    Side::White
                };
                let piece = Piece {
                    side,
                    kind: piece_kind_from_char(pch).ok_or("invalid piece")?,
                    promoted,
                };
                board[row * 9 + col] = Some(piece);
                col += 1;
            }
            if col != 9 {
                return Err("rank width mismatch".to_string());
            }
        }

        let side_to_move = if side_part == "b" {
            Side::Black
        } else {
            Side::White
        };
        let hands = parse_sfen_hands(hands_part)?;

        Ok(Self {
            board,
            side_to_move,
            hands,
            skill_state: SkillState::default(),
        })
    }

    pub fn hydrate_skill_state_from_board_state(&mut self, board_state: &serde_json::Value) {
        let Some(skill_state) = board_state
            .get("skill_state")
            .and_then(|value| value.as_object())
        else {
            return;
        };

        self.skill_state = SkillState::default();

        if let Some(items) = skill_state
            .get("piece_statuses")
            .and_then(|value| value.as_array())
        {
            for item in items {
                let Some((row, col, side)) = parse_row_col_side(item, "side") else {
                    continue;
                };
                let Some(status_type) = item.get("status_type").and_then(|value| value.as_str())
                else {
                    continue;
                };
                let remaining_turns = parse_u8(item.get("remaining_turns")).unwrap_or(0);
                self.add_piece_status(row, col, side, status_type, remaining_turns);
            }
        }

        if let Some(items) = skill_state
            .get("board_hazards")
            .and_then(|value| value.as_array())
        {
            for item in items {
                let Some((row, col, side)) = parse_row_col_side(item, "affects_side") else {
                    continue;
                };
                let Some(hazard_type) = item.get("hazard_type").and_then(|value| value.as_str())
                else {
                    continue;
                };
                let remaining_turns = parse_u8(item.get("remaining_turns")).unwrap_or(0);
                self.add_board_hazard(row, col, side, hazard_type, remaining_turns);
            }
        }

        if let Some(items) = skill_state
            .get("movement_modifiers")
            .and_then(|value| value.as_array())
        {
            for item in items {
                let Some((row, col, side)) = parse_row_col_side(item, "side") else {
                    continue;
                };
                let Some(movement_rule) =
                    item.get("movement_rule").and_then(|value| value.as_str())
                else {
                    continue;
                };
                let remaining_turns = parse_u8(item.get("remaining_turns")).unwrap_or(0);
                self.add_movement_modifier(row, col, side, movement_rule, remaining_turns);
            }
        }

        if let Some(items) = skill_state
            .get("piece_defenses")
            .and_then(|value| value.as_array())
        {
            for item in items {
                let Some((row, col, side)) = parse_row_col_side(item, "side") else {
                    continue;
                };
                let Some(mode) = item.get("mode").and_then(|value| value.as_str()) else {
                    continue;
                };
                let remaining_turns = parse_u8(item.get("remaining_turns")).unwrap_or(0);
                self.add_piece_defense(row, col, side, mode, remaining_turns);
            }
        }

        if let Some(items) = skill_state
            .get("turn_start_rules")
            .and_then(|value| value.as_array())
        {
            for item in items {
                let Some((row, col, side)) = parse_row_col_side(item, "side") else {
                    continue;
                };
                let Some(rule_type) = item.get("rule_type").and_then(|value| value.as_str()) else {
                    continue;
                };
                let phase = parse_u8(item.get("phase")).unwrap_or(0);
                self.add_turn_start_rule(row, col, side, rule_type);
                if let Some(rule) = self.skill_state.turn_start_rules.iter_mut().find(|rule| {
                    rule.row == row
                        && rule.col == col
                        && rule.side == side
                        && rule.rule_type == rule_type
                }) {
                    rule.phase = phase;
                }
            }
        }

        self.prune_skill_state();
    }

    pub fn to_sfen(&self, move_number: u32) -> String {
        let board = (0..9)
            .map(|row| {
                let mut chunk = String::new();
                let mut empty = 0usize;
                for col in 0..9 {
                    match self.board[row * 9 + col] {
                        Some(piece) => {
                            if empty > 0 {
                                chunk.push_str(&empty.to_string());
                                empty = 0;
                            }
                            if piece.promoted {
                                chunk.push('+');
                            }
                            let ch = piece_kind_to_sfen_char(piece.kind);
                            if piece.side == Side::Black {
                                chunk.push(ch);
                            } else {
                                chunk.push(ch.to_ascii_lowercase());
                            }
                        }
                        None => empty += 1,
                    }
                }
                if empty > 0 {
                    chunk.push_str(&empty.to_string());
                }
                chunk
            })
            .collect::<Vec<_>>()
            .join("/");
        let side = if self.side_to_move == Side::Black {
            "b"
        } else {
            "w"
        };
        format!(
            "{board} {side} {} {}",
            self.sfen_hands(),
            move_number.max(1)
        )
    }

    pub fn hands_to_json(&self) -> serde_json::Value {
        let player = hands_json_for_side(&self.hands[side_index(Side::Black)]);
        let enemy = hands_json_for_side(&self.hands[side_index(Side::White)]);
        serde_json::json!({
            "player": player,
            "enemy": enemy
        })
    }

    pub fn skill_state_to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "piece_statuses": self.skill_state.piece_statuses.iter().map(|status| serde_json::json!({
                "row": status.row,
                "col": status.col,
                "side": status.side.as_position_side(),
                "status_type": status.status_type,
                "remaining_turns": status.remaining_turns
            })).collect::<Vec<_>>(),
            "board_hazards": self.skill_state.board_hazards.iter().map(|hazard| serde_json::json!({
                "row": hazard.row,
                "col": hazard.col,
                "affects_side": hazard.affects_side.as_position_side(),
                "hazard_type": hazard.hazard_type,
                "remaining_turns": hazard.remaining_turns
            })).collect::<Vec<_>>(),
            "movement_modifiers": self.skill_state.movement_modifiers.iter().map(|modifier| serde_json::json!({
                "row": modifier.row,
                "col": modifier.col,
                "side": modifier.side.as_position_side(),
                "movement_rule": modifier.movement_rule,
                "remaining_turns": modifier.remaining_turns
            })).collect::<Vec<_>>(),
            "piece_defenses": self.skill_state.piece_defenses.iter().map(|defense| serde_json::json!({
                "row": defense.row,
                "col": defense.col,
                "side": defense.side.as_position_side(),
                "mode": defense.mode,
                "remaining_turns": defense.remaining_turns
            })).collect::<Vec<_>>(),
            "turn_start_rules": self.skill_state.turn_start_rules.iter().map(|rule| serde_json::json!({
                "row": rule.row,
                "col": rule.col,
                "side": rule.side.as_position_side(),
                "rule_type": rule.rule_type,
                "phase": rule.phase
            })).collect::<Vec<_>>(),
        })
    }

    fn sfen_hands(&self) -> String {
        let mut chunks = Vec::new();
        for (kind, idx) in [
            (PieceKind::Rook, 6usize),
            (PieceKind::Bishop, 5usize),
            (PieceKind::Gold, 4usize),
            (PieceKind::Silver, 3usize),
            (PieceKind::Knight, 2usize),
            (PieceKind::Lance, 1usize),
            (PieceKind::Pawn, 0usize),
        ] {
            let player_count = self.hands[side_index(Side::Black)][idx];
            let enemy_count = self.hands[side_index(Side::White)][idx];
            let sfen = piece_kind_to_sfen_char(kind);
            if player_count > 0 {
                chunks.push(format!("{}{}", count_prefix(player_count), sfen));
            }
            if enemy_count > 0 {
                chunks.push(format!(
                    "{}{}",
                    count_prefix(enemy_count),
                    sfen.to_ascii_lowercase()
                ));
            }
        }
        if chunks.is_empty() {
            "-".to_string()
        } else {
            chunks.join("")
        }
    }

    pub fn finish_turn_for(&mut self, side: Side) {
        for status in &mut self.skill_state.piece_statuses {
            if status.side == side {
                status.remaining_turns = status.remaining_turns.saturating_sub(1);
            }
        }
        for hazard in &mut self.skill_state.board_hazards {
            if hazard.affects_side == side {
                hazard.remaining_turns = hazard.remaining_turns.saturating_sub(1);
            }
        }
        for modifier in &mut self.skill_state.movement_modifiers {
            if modifier.side == side {
                modifier.remaining_turns = modifier.remaining_turns.saturating_sub(1);
            }
        }
        for defense in &mut self.skill_state.piece_defenses {
            if defense.side == side {
                defense.remaining_turns = defense.remaining_turns.saturating_sub(1);
            }
        }
        self.prune_skill_state();
    }

    pub fn begin_turn_for(&mut self, side: Side) {
        let mut refreshed_modifiers = Vec::new();
        for rule in &mut self.skill_state.turn_start_rules {
            if rule.side != side {
                continue;
            }
            if !matches!(
                self.board[rule.row * 9 + rule.col],
                Some(piece) if piece.side == side
            ) {
                continue;
            }
            if rule.rule_type == "cyclic_pattern_change" {
                let movement_rule = if rule.phase % 2 == 0 {
                    "orthogonal_step_only"
                } else {
                    "diagonal_step_only"
                };
                refreshed_modifiers.push((rule.row, rule.col, side, movement_rule.to_string(), 1));
                rule.phase = (rule.phase + 1) % 2;
            }
        }
        for (row, col, refresh_side, movement_rule, remaining_turns) in refreshed_modifiers {
            self.add_movement_modifier(row, col, refresh_side, movement_rule, remaining_turns);
        }
        self.prune_skill_state();
    }

    pub fn prune_skill_state(&mut self) {
        self.skill_state.piece_statuses.retain(|status| {
            if status.remaining_turns == 0 {
                return false;
            }
            matches!(
                self.board[status.row * 9 + status.col],
                Some(piece) if piece.side == status.side
            )
        });
        self.skill_state
            .board_hazards
            .retain(|hazard| hazard.remaining_turns > 0);
        self.skill_state.movement_modifiers.retain(|modifier| {
            if modifier.remaining_turns == 0 {
                return false;
            }
            matches!(
                self.board[modifier.row * 9 + modifier.col],
                Some(piece) if piece.side == modifier.side
            )
        });
        self.skill_state.piece_defenses.retain(|defense| {
            if defense.remaining_turns == 0 {
                return false;
            }
            matches!(
                self.board[defense.row * 9 + defense.col],
                Some(piece) if piece.side == defense.side
            )
        });
        self.skill_state.turn_start_rules.retain(|rule| {
            matches!(
                self.board[rule.row * 9 + rule.col],
                Some(piece) if piece.side == rule.side
            )
        });
    }

    pub fn move_piece_statuses(&mut self, from: (usize, usize), to: (usize, usize), side: Side) {
        for status in &mut self.skill_state.piece_statuses {
            if status.side == side && (status.row, status.col) == from {
                status.row = to.0;
                status.col = to.1;
            }
        }
        for modifier in &mut self.skill_state.movement_modifiers {
            if modifier.side == side && (modifier.row, modifier.col) == from {
                modifier.row = to.0;
                modifier.col = to.1;
            }
        }
        for defense in &mut self.skill_state.piece_defenses {
            if defense.side == side && (defense.row, defense.col) == from {
                defense.row = to.0;
                defense.col = to.1;
            }
        }
        for rule in &mut self.skill_state.turn_start_rules {
            if rule.side == side && (rule.row, rule.col) == from {
                rule.row = to.0;
                rule.col = to.1;
            }
        }
    }

    pub fn add_piece_status(
        &mut self,
        row: usize,
        col: usize,
        side: Side,
        status_type: impl Into<String>,
        remaining_turns: u8,
    ) {
        if remaining_turns == 0 {
            return;
        }
        let status_type = status_type.into();
        if let Some(existing) = self.skill_state.piece_statuses.iter_mut().find(|status| {
            status.row == row
                && status.col == col
                && status.side == side
                && status.status_type == status_type
        }) {
            existing.remaining_turns = existing.remaining_turns.max(remaining_turns);
            return;
        }
        self.skill_state.piece_statuses.push(PieceStatusState {
            row,
            col,
            side,
            status_type,
            remaining_turns,
        });
    }

    pub fn add_board_hazard(
        &mut self,
        row: usize,
        col: usize,
        affects_side: Side,
        hazard_type: impl Into<String>,
        remaining_turns: u8,
    ) {
        if remaining_turns == 0 {
            return;
        }
        let hazard_type = hazard_type.into();
        if let Some(existing) = self.skill_state.board_hazards.iter_mut().find(|hazard| {
            hazard.row == row
                && hazard.col == col
                && hazard.affects_side == affects_side
                && hazard.hazard_type == hazard_type
        }) {
            existing.remaining_turns = existing.remaining_turns.max(remaining_turns);
            return;
        }
        self.skill_state.board_hazards.push(BoardHazardState {
            row,
            col,
            affects_side,
            hazard_type,
            remaining_turns,
        });
    }

    pub fn add_movement_modifier(
        &mut self,
        row: usize,
        col: usize,
        side: Side,
        movement_rule: impl Into<String>,
        remaining_turns: u8,
    ) {
        if remaining_turns == 0 {
            return;
        }
        let movement_rule = movement_rule.into();
        if let Some(existing) = self
            .skill_state
            .movement_modifiers
            .iter_mut()
            .find(|modifier| {
                modifier.row == row
                    && modifier.col == col
                    && modifier.side == side
                    && modifier.movement_rule == movement_rule
            })
        {
            existing.remaining_turns = existing.remaining_turns.max(remaining_turns);
            return;
        }
        self.skill_state
            .movement_modifiers
            .push(MovementModifierState {
                row,
                col,
                side,
                movement_rule,
                remaining_turns,
            });
    }

    pub fn add_piece_defense(
        &mut self,
        row: usize,
        col: usize,
        side: Side,
        mode: impl Into<String>,
        remaining_turns: u8,
    ) {
        if remaining_turns == 0 {
            return;
        }
        let mode = mode.into();
        if let Some(existing) = self.skill_state.piece_defenses.iter_mut().find(|defense| {
            defense.row == row && defense.col == col && defense.side == side && defense.mode == mode
        }) {
            existing.remaining_turns = existing.remaining_turns.max(remaining_turns);
            return;
        }
        self.skill_state.piece_defenses.push(PieceDefenseState {
            row,
            col,
            side,
            mode,
            remaining_turns,
        });
    }

    pub fn add_turn_start_rule(
        &mut self,
        row: usize,
        col: usize,
        side: Side,
        rule_type: impl Into<String>,
    ) {
        let rule_type = rule_type.into();
        if self.skill_state.turn_start_rules.iter().any(|rule| {
            rule.row == row && rule.col == col && rule.side == side && rule.rule_type == rule_type
        }) {
            return;
        }
        self.skill_state.turn_start_rules.push(TurnStartRuleState {
            row,
            col,
            side,
            rule_type,
            phase: 0,
        });
    }

    pub fn has_piece_status(&self, row: usize, col: usize, side: Side, status_type: &str) -> bool {
        self.skill_state.piece_statuses.iter().any(|status| {
            status.remaining_turns > 0
                && status.side == side
                && status.row == row
                && status.col == col
                && status.status_type == status_type
        })
    }

    pub fn piece_status_penalty(&self, row: usize, col: usize, side: Side) -> i32 {
        self.skill_state
            .piece_statuses
            .iter()
            .filter(|status| {
                status.remaining_turns > 0
                    && status.side == side
                    && status.row == row
                    && status.col == col
            })
            .map(|status| match status.status_type.as_str() {
                "freeze" | "time_stop" | "infected_immobilized" | "stun" => 80,
                "dark_blind" => 28,
                "drown" => 20,
                _ => 16,
            })
            .sum()
    }

    pub fn has_movement_modifier(
        &self,
        row: usize,
        col: usize,
        side: Side,
        movement_rule: &str,
    ) -> bool {
        self.skill_state.movement_modifiers.iter().any(|modifier| {
            modifier.remaining_turns > 0
                && modifier.side == side
                && modifier.row == row
                && modifier.col == col
                && modifier.movement_rule == movement_rule
        })
    }

    pub fn movement_rule_for_piece(
        &self,
        row: usize,
        col: usize,
        side: Side,
    ) -> Option<&'static str> {
        for movement_rule in [
            "vertical_step_only",
            "diagonal_step_only",
            "orthogonal_step_only",
            "backward_step_only",
        ] {
            if self.has_movement_modifier(row, col, side, movement_rule) {
                return Some(movement_rule);
            }
        }
        None
    }

    pub fn movement_modifier_penalty(&self, row: usize, col: usize, side: Side) -> i32 {
        self.skill_state
            .movement_modifiers
            .iter()
            .filter(|modifier| {
                modifier.remaining_turns > 0
                    && modifier.side == side
                    && modifier.row == row
                    && modifier.col == col
            })
            .map(|modifier| match modifier.movement_rule.as_str() {
                "vertical_step_only" => 70,
                "diagonal_step_only" => 48,
                "orthogonal_step_only" => 40,
                "backward_step_only" => 28,
                _ => 18,
            })
            .sum()
    }

    pub fn has_board_hazard(&self, row: usize, col: usize, side: Side) -> bool {
        self.skill_state.board_hazards.iter().any(|hazard| {
            hazard.remaining_turns > 0
                && hazard.affects_side == side
                && hazard.row == row
                && hazard.col == col
        })
    }

    pub fn board_hazard_penalty(&self, row: usize, col: usize, side: Side) -> i32 {
        self.skill_state
            .board_hazards
            .iter()
            .filter(|hazard| {
                hazard.remaining_turns > 0
                    && hazard.affects_side == side
                    && hazard.row == row
                    && hazard.col == col
            })
            .map(|hazard| match hazard.hazard_type.as_str() {
                "poison_pool" | "pitfall" => 36,
                _ => 24,
            })
            .sum()
    }

    pub fn has_piece_defense(&self, row: usize, col: usize, side: Side, mode: &str) -> bool {
        self.skill_state.piece_defenses.iter().any(|defense| {
            defense.remaining_turns > 0
                && defense.side == side
                && defense.row == row
                && defense.col == col
                && defense.mode == mode
        })
    }

    pub fn capture_blocked_by_piece_defense(
        &self,
        row: usize,
        col: usize,
        target_side: Side,
    ) -> bool {
        self.skill_state.piece_defenses.iter().any(|defense| {
            defense.remaining_turns > 0
                && defense.side == target_side
                && defense.row == row
                && defense.col == col
                && matches!(
                    defense.mode.as_str(),
                    "immune_to_capture"
                        | "grant_invulnerability"
                        | "grant_uncapturable"
                        | "uncapturable_if_ally_yin_same_row_or_col"
                )
        })
    }

    pub fn piece_defense_bonus(&self, row: usize, col: usize, side: Side) -> i32 {
        self.skill_state
            .piece_defenses
            .iter()
            .filter(|defense| {
                defense.remaining_turns > 0
                    && defense.side == side
                    && defense.row == row
                    && defense.col == col
            })
            .map(|defense| match defense.mode.as_str() {
                "immune_to_capture" => 88,
                "grant_invulnerability" | "grant_uncapturable" => 80,
                "immune_to_special_effects" => 44,
                "self_guard_aura" | "protect_king_aura" => 36,
                "sidestep_evade" | "evade_enemy_attack" => 32,
                "two_hit_survival" => 28,
                _ => 20,
            })
            .sum()
    }
}

pub fn piece_kind_from_char(ch: char) -> Option<PieceKind> {
    match ch.to_ascii_uppercase() {
        'P' => Some(PieceKind::Pawn),
        'L' => Some(PieceKind::Lance),
        'N' => Some(PieceKind::Knight),
        'S' => Some(PieceKind::Silver),
        'G' => Some(PieceKind::Gold),
        'B' => Some(PieceKind::Bishop),
        'R' => Some(PieceKind::Rook),
        'K' => Some(PieceKind::King),
        _ => None,
    }
}

pub fn piece_kind_from_code(code: &str) -> Option<PieceKind> {
    match code.to_ascii_uppercase().as_str() {
        "FU" | "P" => Some(PieceKind::Pawn),
        "KY" | "L" => Some(PieceKind::Lance),
        "KE" | "N" => Some(PieceKind::Knight),
        "GI" | "S" => Some(PieceKind::Silver),
        "KI" | "G" => Some(PieceKind::Gold),
        "KA" | "B" => Some(PieceKind::Bishop),
        "HI" | "R" => Some(PieceKind::Rook),
        "OU" | "K" => Some(PieceKind::King),
        _ => None,
    }
}

pub fn piece_code(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::Pawn => "FU",
        PieceKind::Lance => "KY",
        PieceKind::Knight => "KE",
        PieceKind::Silver => "GI",
        PieceKind::Gold => "KI",
        PieceKind::Bishop => "KA",
        PieceKind::Rook => "HI",
        PieceKind::King => "OU",
    }
}

pub fn piece_base_value(kind: PieceKind) -> i32 {
    match kind {
        PieceKind::Pawn => 100,
        PieceKind::Lance => 300,
        PieceKind::Knight => 320,
        PieceKind::Silver => 500,
        PieceKind::Gold => 600,
        PieceKind::Bishop => 900,
        PieceKind::Rook => 1000,
        PieceKind::King => 10000,
    }
}

pub fn piece_promotable(kind: PieceKind) -> bool {
    !matches!(kind, PieceKind::Gold | PieceKind::King)
}

pub fn is_promotion_zone(side: Side, row: usize) -> bool {
    match side {
        Side::Black => row <= 2,
        Side::White => row >= 6,
    }
}

pub fn must_promote(piece: Piece, to_row: usize) -> bool {
    match (piece.side, piece.kind) {
        (Side::Black, PieceKind::Pawn | PieceKind::Lance) => to_row == 0,
        (Side::White, PieceKind::Pawn | PieceKind::Lance) => to_row == 8,
        (Side::Black, PieceKind::Knight) => to_row <= 1,
        (Side::White, PieceKind::Knight) => to_row >= 7,
        _ => false,
    }
}

pub fn side_index(side: Side) -> usize {
    match side {
        Side::Black => 0,
        Side::White => 1,
    }
}

pub fn hand_index(kind: PieceKind) -> Option<usize> {
    match kind {
        PieceKind::Pawn => Some(0),
        PieceKind::Lance => Some(1),
        PieceKind::Knight => Some(2),
        PieceKind::Silver => Some(3),
        PieceKind::Gold => Some(4),
        PieceKind::Bishop => Some(5),
        PieceKind::Rook => Some(6),
        PieceKind::King => None,
    }
}

fn parse_sfen_hands(hands: &str) -> Result<[[u8; 7]; 2], String> {
    let mut out = [[0u8; 7]; 2];
    if hands == "-" {
        return Ok(out);
    }

    let mut cnt = 0u32;
    for ch in hands.chars() {
        if ch.is_ascii_digit() {
            cnt = cnt * 10 + ch.to_digit(10).ok_or("invalid hand digit")?;
            continue;
        }
        let n = if cnt == 0 { 1 } else { cnt };
        cnt = 0;

        let side = if ch.is_ascii_uppercase() {
            Side::Black
        } else {
            Side::White
        };
        let kind = piece_kind_from_char(ch).ok_or("invalid hand piece")?;
        if kind == PieceKind::King {
            return Err("king cannot be in hand".to_string());
        }

        let idx = hand_index(kind).ok_or("invalid hand kind")?;
        out[side_index(side)][idx] = out[side_index(side)][idx].saturating_add(n as u8);
    }

    Ok(out)
}

fn parse_row_col_side(item: &serde_json::Value, side_key: &str) -> Option<(usize, usize, Side)> {
    let row = item.get("row").and_then(|value| value.as_u64())? as usize;
    let col = item.get("col").and_then(|value| value.as_u64())? as usize;
    if row > 8 || col > 8 {
        return None;
    }
    let side = item
        .get(side_key)
        .and_then(|value| value.as_str())
        .and_then(Side::from_position_side)?;
    Some((row, col, side))
}

fn parse_u8(value: Option<&serde_json::Value>) -> Option<u8> {
    value
        .and_then(|value| value.as_u64())
        .map(|value| value as u8)
}

fn piece_kind_to_sfen_char(kind: PieceKind) -> char {
    match kind {
        PieceKind::Pawn => 'P',
        PieceKind::Lance => 'L',
        PieceKind::Knight => 'N',
        PieceKind::Silver => 'S',
        PieceKind::Gold => 'G',
        PieceKind::Bishop => 'B',
        PieceKind::Rook => 'R',
        PieceKind::King => 'K',
    }
}

fn hands_json_for_side(counts: &[u8; 7]) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for (code, idx) in [
        ("FU", 0usize),
        ("KY", 1usize),
        ("KE", 2usize),
        ("GI", 3usize),
        ("KI", 4usize),
        ("KA", 5usize),
        ("HI", 6usize),
    ] {
        let count = counts[idx];
        if count > 0 {
            out.insert(code.to_string(), serde_json::json!(count));
        }
    }
    serde_json::Value::Object(out)
}

fn count_prefix(count: u8) -> String {
    if count > 1 {
        count.to_string()
    } else {
        String::new()
    }
}
