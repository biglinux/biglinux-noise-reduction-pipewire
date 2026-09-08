//! Optional loader resource policies; automatic system scheduling is default.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub prefer_fast_cpus: bool,
    pub reserve_memory: bool,
}
