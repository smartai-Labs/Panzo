use crate::time::TimeTick;
use serde::{Deserialize, Serialize};

pub const JOURNAL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JournalOperation {
    ProjectCreated,
    EpochEstablished,
    StateTransition,
    Checkpoint,
    FinalizeStarted,
    FinalizeCompleted,
    RecoveryStarted,
    RecoveryCompleted,
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JournalResult {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub schema_version: u32,
    pub seq: u64,
    pub time_tick: TimeTick,
    pub operation: JournalOperation,
    pub result: JournalResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl JournalEntry {
    pub fn new(
        seq: u64,
        time_tick: TimeTick,
        operation: JournalOperation,
        result: JournalResult,
        detail: Option<String>,
    ) -> Self {
        Self {
            schema_version: JOURNAL_SCHEMA_VERSION,
            seq,
            time_tick,
            operation,
            result,
            detail,
        }
    }
}
