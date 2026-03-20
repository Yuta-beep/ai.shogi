use crate::engine::types::PieceKind;

type MappingEntry = (&'static str, char, bool);

// DB(master.m_piece_mapping) を反映した Shogi-AI 側の単一ソース。
const MAPPINGS: &[MappingEntry] = &[
    ("FU", 'P', false),
    ("KY", 'L', false),
    ("KE", 'N', false),
    ("GI", 'S', false),
    ("KI", 'G', false),
    ("KA", 'B', false),
    ("HI", 'R', false),
    ("OU", 'K', false),
    ("TO", 'P', true),
    ("NY", 'L', true),
    ("NK", 'N', true),
    ("NG", 'S', true),
    ("UM", 'B', true),
    ("RY", 'R', true),
    ("NIN", 'C', false),
    ("KAG", 'D', false),
    ("HOU", 'E', false),
    ("RYU", 'F', false),
    ("HOO", 'H', false),
    ("ENN", 'I', false),
    ("FIR", 'J', false),
    ("SUI", 'M', false),
    ("NAM", 'Q', false),
    ("MOK", 'T', false),
    ("HAA", 'U', false),
    ("HIK", 'V', false),
    ("HOS", 'W', false),
    ("YAM", 'X', false),
    ("MAK", 'Y', false),
];

pub fn piece_kind_from_sfen_char(ch: char, promoted: bool) -> Option<PieceKind> {
    let upper = ch.to_ascii_uppercase();
    if promoted {
        return match upper {
            'P' => Some(PieceKind::Pawn),
            'L' => Some(PieceKind::Lance),
            'N' => Some(PieceKind::Knight),
            'S' => Some(PieceKind::Silver),
            'B' => Some(PieceKind::Bishop),
            'R' => Some(PieceKind::Rook),
            _ => None,
        };
    }
    let (code, _, _) = MAPPINGS
        .iter()
        .find(|(_, sfen, is_promoted)| *sfen == upper && *is_promoted == promoted)?;
    Some(piece_kind_from_code(code)?)
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
        "TO" | "NY" | "NK" | "NG" | "UM" | "RY" => None,
        other => MAPPINGS
            .iter()
            .find(|(mapped, _, _)| mapped.eq_ignore_ascii_case(other))
            .map(|(mapped, _, _)| PieceKind::Custom(mapped)),
    }
}

pub fn piece_code(kind: &PieceKind) -> &str {
    match kind {
        PieceKind::Pawn => "FU",
        PieceKind::Lance => "KY",
        PieceKind::Knight => "KE",
        PieceKind::Silver => "GI",
        PieceKind::Gold => "KI",
        PieceKind::Bishop => "KA",
        PieceKind::Rook => "HI",
        PieceKind::King => "OU",
        PieceKind::Custom(code) => code,
    }
}

pub fn sfen_char_from_piece_kind(kind: &PieceKind) -> Option<char> {
    let code = piece_code(kind);
    MAPPINGS
        .iter()
        .find(|(mapped, _, is_promoted)| mapped.eq_ignore_ascii_case(code) && !*is_promoted)
        .map(|(_, sfen, _)| *sfen)
}

// DB(master.m_piece_mapping) の kanji ↔ displayCode 対応テーブル。
// skill_definition_v2_catalog.json の pieceChars は漢字を使用しているため、
// skill_executor の matches_piece_code で displayCode との照合に使う。
const KANJI_TO_CODE: &[(&str, &str)] = &[
    ("忍", "NIN"),
    ("影", "KAG"),
    ("砲", "HOU"),
    ("竜", "RYU"),
    ("鳳", "HOO"),
    ("炎", "ENN"),
    ("火", "FIR"),
    ("水", "SUI"),
    ("波", "NAM"),
    ("木", "MOK"),
    ("葉", "HAA"),
    ("光", "HIK"),
    ("星", "HOS"),
    ("闇", "YAM"),
    ("魔", "MAK"),
];

/// catalog の pieceChars に含まれる漢字から displayCode (NIN/KAG/…) を返す。
/// 漢字でない場合は None。
pub fn kanji_to_code(kanji: &str) -> Option<&'static str> {
    KANJI_TO_CODE
        .iter()
        .find(|(k, _)| *k == kanji)
        .map(|(_, code)| *code)
}
