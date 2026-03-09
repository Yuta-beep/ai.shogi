pub mod config;
pub mod heuristic;
pub mod rules;
pub mod search;
pub mod types;
pub mod util;

pub use config::{build_engine_config, EngineConfig, EngineConfigPatch};
pub use heuristic::{evaluate_move, is_board_coordinate_valid};
pub use rules::parse_runtime_rules;
pub use search::search_with_iterative_deepening;
pub use types::{EngineMove, SearchState, Side};
pub use util::{make_seed, select_move_index};

#[cfg(test)]
mod tests;
