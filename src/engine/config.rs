use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Serialize)]
pub struct EngineConfig {
    pub max_depth: u32,
    pub max_nodes: u32,
    pub time_limit_ms: u32,
    pub quiescence_enabled: bool,
    pub eval_material_weight: f64,
    pub eval_position_weight: f64,
    pub eval_king_safety_weight: f64,
    pub eval_mobility_weight: f64,
    pub blunder_rate: f64,
    pub blunder_max_loss_cp: u32,
    pub random_topk: u32,
    pub temperature: f64,
    pub always_legal_move: bool,
    pub mate_avoidance: bool,
    pub max_repeat_draw_bias: f64,
    pub random_seed: Option<u64>,
}

#[derive(Debug, Default, Clone)]
pub struct EngineConfigPatch {
    pub max_depth: Option<u32>,
    pub max_nodes: Option<u32>,
    pub time_limit_ms: Option<u32>,
    pub quiescence_enabled: Option<bool>,
    pub eval_material_weight: Option<f64>,
    pub eval_position_weight: Option<f64>,
    pub eval_king_safety_weight: Option<f64>,
    pub eval_mobility_weight: Option<f64>,
    pub blunder_rate: Option<f64>,
    pub blunder_max_loss_cp: Option<u32>,
    pub random_topk: Option<u32>,
    pub temperature: Option<f64>,
    pub always_legal_move: Option<bool>,
    pub mate_avoidance: Option<bool>,
    pub max_repeat_draw_bias: Option<f64>,
    pub random_seed: Option<u64>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_nodes: 20_000,
            time_limit_ms: 300,
            quiescence_enabled: true,
            eval_material_weight: 1.0,
            eval_position_weight: 0.35,
            eval_king_safety_weight: 0.25,
            eval_mobility_weight: 0.2,
            blunder_rate: 0.0,
            blunder_max_loss_cp: 0,
            random_topk: 1,
            temperature: 0.0,
            always_legal_move: true,
            mate_avoidance: true,
            max_repeat_draw_bias: 0.0,
            random_seed: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{0}")]
    Invalid(String),
}

pub fn build_engine_config(input: EngineConfigPatch) -> Result<EngineConfig, ConfigError> {
    let mut cfg = EngineConfig::default();

    if let Some(v) = input.max_depth {
        ensure_range_u32("max_depth", v, 1, 12)?;
        cfg.max_depth = v;
    }
    if let Some(v) = input.max_nodes {
        ensure_range_u32("max_nodes", v, 100, 5_000_000)?;
        cfg.max_nodes = v;
    }
    if let Some(v) = input.time_limit_ms {
        ensure_range_u32("time_limit_ms", v, 10, 60_000)?;
        cfg.time_limit_ms = v;
    }
    if let Some(v) = input.quiescence_enabled {
        cfg.quiescence_enabled = v;
    }
    if let Some(v) = input.eval_material_weight {
        ensure_range_f64("eval_material_weight", v, 0.0, 10.0)?;
        cfg.eval_material_weight = v;
    }
    if let Some(v) = input.eval_position_weight {
        ensure_range_f64("eval_position_weight", v, 0.0, 10.0)?;
        cfg.eval_position_weight = v;
    }
    if let Some(v) = input.eval_king_safety_weight {
        ensure_range_f64("eval_king_safety_weight", v, 0.0, 10.0)?;
        cfg.eval_king_safety_weight = v;
    }
    if let Some(v) = input.eval_mobility_weight {
        ensure_range_f64("eval_mobility_weight", v, 0.0, 10.0)?;
        cfg.eval_mobility_weight = v;
    }
    if let Some(v) = input.blunder_rate {
        ensure_range_f64("blunder_rate", v, 0.0, 1.0)?;
        cfg.blunder_rate = v;
    }
    if let Some(v) = input.blunder_max_loss_cp {
        ensure_range_u32("blunder_max_loss_cp", v, 0, 3000)?;
        cfg.blunder_max_loss_cp = v;
    }
    if let Some(v) = input.random_topk {
        ensure_range_u32("random_topk", v, 1, 20)?;
        cfg.random_topk = v;
    }
    if let Some(v) = input.temperature {
        ensure_range_f64("temperature", v, 0.0, 2.0)?;
        cfg.temperature = v;
    }
    if let Some(v) = input.max_repeat_draw_bias {
        ensure_range_f64("max_repeat_draw_bias", v, -1.0, 1.0)?;
        cfg.max_repeat_draw_bias = v;
    }

    cfg.random_seed = input.random_seed;

    if let Some(v) = input.always_legal_move {
        if !v {
            return Err(ConfigError::Invalid(
                "always_legal_move must be true".to_string(),
            ));
        }
    }
    if let Some(v) = input.mate_avoidance {
        if !v {
            return Err(ConfigError::Invalid(
                "mate_avoidance must be true".to_string(),
            ));
        }
    }

    Ok(cfg)
}

fn ensure_range_u32(name: &str, value: u32, min: u32, max: u32) -> Result<(), ConfigError> {
    if value < min || value > max {
        return Err(ConfigError::Invalid(format!(
            "{} must be in {}..={} (got {})",
            name, min, max, value
        )));
    }
    Ok(())
}

fn ensure_range_f64(name: &str, value: f64, min: f64, max: f64) -> Result<(), ConfigError> {
    if !value.is_finite() || value < min || value > max {
        return Err(ConfigError::Invalid(format!(
            "{} must be in {}..={} (got {})",
            name, min, max, value
        )));
    }
    Ok(())
}
