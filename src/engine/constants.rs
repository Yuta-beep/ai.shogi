// Engine search defaults
pub const DEFAULT_MAX_DEPTH: u32 = 4;
pub const DEFAULT_MAX_NODES: u32 = 50_000;
pub const DEFAULT_TIME_LIMIT_MS: u32 = 500;
pub const DEFAULT_EVAL_MATERIAL_WEIGHT: f64 = 1.0;
pub const DEFAULT_EVAL_POSITION_WEIGHT: f64 = 0.35;
pub const DEFAULT_EVAL_KING_SAFETY_WEIGHT: f64 = 0.25;
pub const DEFAULT_EVAL_MOBILITY_WEIGHT: f64 = 0.2;
pub const DEFAULT_BLUNDER_RATE: f64 = 0.0;
pub const DEFAULT_BLUNDER_MAX_LOSS_CP: u32 = 0;
pub const DEFAULT_RANDOM_TOPK: u32 = 1;
pub const DEFAULT_TEMPERATURE: f64 = 0.0;
pub const DEFAULT_MAX_REPEAT_DRAW_BIAS: f64 = 0.0;

// Search scores
pub const SCORE_INF: i32 = 30_000;
pub const SCORE_CHECKMATE_BASE: i32 = 29_000;

// Evaluation
pub const PROMOTION_BONUS_CP: f64 = 80.0;
pub const CENTER_DIST_MAX: f64 = 8.0;

// Heuristic (no-SFEN fallback)
pub const HEURISTIC_PROMOTE_BONUS_CP: f64 = 60.0;
pub const HEURISTIC_CENTER_WEIGHT: f64 = 3.0;
pub const HEURISTIC_MOBILITY_BASE_CP: f64 = 5.0;
pub const HEURISTIC_KING_SAFETY_BASE_CP: f64 = 2.0;
