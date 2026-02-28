use thiserror::Error;

use super::id::ItemId;

#[derive(Debug, Error)]
pub enum SwarmitError {
    #[error("Item not found: {0}")]
    NotFound(ItemId),

    #[error("Invalid item ID format: {0}")]
    InvalidId(String),

    #[error("Invalid agent ID: {0}")]
    InvalidAgentId(String),

    #[error("Invalid status transition from {from} to {to}")]
    InvalidStatusTransition { from: String, to: String },

    #[error("Self-relationship not allowed: {0}")]
    SelfRelationship(ItemId),

    #[error("Relationship already exists: {from} {rel_type} {to}")]
    DuplicateRelationship {
        from: ItemId,
        rel_type: String,
        to: ItemId,
    },

    #[error("Project not initialized in {0}")]
    NotInitialized(String),

    #[error("Project already initialized at {0}")]
    AlreadyInitialized(String),

    #[error("Lock timeout after {0}ms")]
    LockTimeout(u64),

    #[error("Corrupted operations log: {0}")]
    CorruptedLog(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, SwarmitError>;
