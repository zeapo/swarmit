use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::error::{Result, SwarmitError};

/// An item ID in PREFIX-NNN format (e.g., TASK-001, EPIC-042).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ItemId(String);

impl ItemId {
    pub fn new(prefix: &str, number: u32) -> Self {
        ItemId(format!("{}-{:03}", prefix.to_uppercase(), number))
    }

    pub fn prefix(&self) -> &str {
        self.0.split('-').next().unwrap_or("")
    }

    pub fn number(&self) -> Option<u32> {
        self.0.split('-').nth(1).and_then(|n| n.parse().ok())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn validate(s: &str) -> Result<()> {
        let parts: Vec<&str> = s.splitn(2, '-').collect();
        if parts.len() != 2 {
            return Err(SwarmitError::InvalidId(s.to_string()));
        }
        let prefix = parts[0];
        let number = parts[1];
        if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(SwarmitError::InvalidId(s.to_string()));
        }
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
            return Err(SwarmitError::InvalidId(s.to_string()));
        }
        Ok(())
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ItemId {
    type Err = SwarmitError;

    fn from_str(s: &str) -> Result<Self> {
        ItemId::validate(s)?;
        Ok(ItemId(s.to_string()))
    }
}

/// A validated agent identifier.
/// Must be non-empty, alphanumeric with hyphens/underscores allowed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(String);

impl AgentId {
    pub fn new(s: &str) -> Result<Self> {
        if s.is_empty() {
            return Err(SwarmitError::InvalidAgentId(s.to_string()));
        }
        if !s
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(SwarmitError::InvalidAgentId(s.to_string()));
        }
        Ok(AgentId(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for AgentId {
    type Err = SwarmitError;

    fn from_str(s: &str) -> Result<Self> {
        AgentId::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_id_display() {
        let id = ItemId::new("TASK", 1);
        assert_eq!(id.to_string(), "TASK-001");
    }

    #[test]
    fn item_id_display_large_number() {
        let id = ItemId::new("EPIC", 42);
        assert_eq!(id.to_string(), "EPIC-042");
    }

    #[test]
    fn item_id_display_very_large() {
        let id = ItemId::new("TASK", 1234);
        assert_eq!(id.to_string(), "TASK-1234");
    }

    #[test]
    fn item_id_prefix() {
        let id = ItemId::new("TASK", 5);
        assert_eq!(id.prefix(), "TASK");
    }

    #[test]
    fn item_id_number() {
        let id = ItemId::new("TASK", 42);
        assert_eq!(id.number(), Some(42));
    }

    #[test]
    fn item_id_from_str_valid() {
        let id: ItemId = "TASK-001".parse().unwrap();
        assert_eq!(id.as_str(), "TASK-001");
    }

    #[test]
    fn item_id_from_str_invalid_no_dash() {
        assert!("TASK001".parse::<ItemId>().is_err());
    }

    #[test]
    fn item_id_from_str_invalid_lowercase_prefix() {
        assert!("task-001".parse::<ItemId>().is_err());
    }

    #[test]
    fn item_id_from_str_invalid_non_numeric() {
        assert!("TASK-abc".parse::<ItemId>().is_err());
    }

    #[test]
    fn agent_id_valid() {
        assert!(AgentId::new("agent-1").is_ok());
        assert!(AgentId::new("claude_agent").is_ok());
        assert!(AgentId::new("me").is_ok());
        assert!(AgentId::new("agent.v2").is_ok());
    }

    #[test]
    fn agent_id_empty() {
        assert!(AgentId::new("").is_err());
    }

    #[test]
    fn agent_id_invalid_chars() {
        assert!(AgentId::new("agent@1").is_err());
        assert!(AgentId::new("agent 1").is_err());
    }

    #[test]
    fn agent_id_display() {
        let id = AgentId::new("my-agent").unwrap();
        assert_eq!(id.to_string(), "my-agent");
    }
}
