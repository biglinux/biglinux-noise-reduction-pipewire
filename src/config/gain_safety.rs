//! Sample-peak protection, not a true-peak or mastering limiter.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GainSafety {
    #[default]
    Automatic,
    Unrestricted,
}
