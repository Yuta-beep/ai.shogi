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
pub struct SearchState {
    pub board: [Option<Piece>; 81],
    pub side_to_move: Side,
    pub hands: [[u8; 7]; 2],
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
        })
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
