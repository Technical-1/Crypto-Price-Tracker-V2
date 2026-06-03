//! Rebalancing. `Strategy` defined here now; logic added in Task 11.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strategy {
    Band,
    Full,
}
