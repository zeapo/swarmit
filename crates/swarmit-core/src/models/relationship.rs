use serde::{Deserialize, Serialize};
use std::fmt;

use super::id::ItemId;

/// The type of relationship between two items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    Blocks,
    BlockedBy,
    Parent,
    Child,
    RelatesTo,
    Duplicates,
    DuplicatedBy,
}

impl RelationType {
    /// Returns the inverse relationship type.
    pub fn inverse(&self) -> RelationType {
        match self {
            RelationType::Blocks => RelationType::BlockedBy,
            RelationType::BlockedBy => RelationType::Blocks,
            RelationType::Parent => RelationType::Child,
            RelationType::Child => RelationType::Parent,
            RelationType::RelatesTo => RelationType::RelatesTo,
            RelationType::Duplicates => RelationType::DuplicatedBy,
            RelationType::DuplicatedBy => RelationType::Duplicates,
        }
    }

    /// Returns true if this relationship type has a meaningful inverse
    /// that should be automatically created.
    pub fn has_auto_inverse(&self) -> bool {
        !matches!(self, RelationType::RelatesTo)
    }
}

impl fmt::Display for RelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            RelationType::Blocks => "blocks",
            RelationType::BlockedBy => "blocked_by",
            RelationType::Parent => "parent",
            RelationType::Child => "child",
            RelationType::RelatesTo => "relates_to",
            RelationType::Duplicates => "duplicates",
            RelationType::DuplicatedBy => "duplicated_by",
        };
        write!(f, "{}", s)
    }
}

/// A directed relationship between two items.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Relationship {
    pub from: ItemId,
    pub to: ItemId,
    pub rel_type: RelationType,
}

impl Relationship {
    pub fn new(from: ItemId, to: ItemId, rel_type: RelationType) -> Self {
        Relationship { from, to, rel_type }
    }

    /// Returns the inverse of this relationship.
    pub fn inverse(&self) -> Relationship {
        Relationship {
            from: self.to.clone(),
            to: self.from.clone(),
            rel_type: self.rel_type.inverse(),
        }
    }
}

impl fmt::Display for Relationship {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.from, self.rel_type, self.to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> ItemId {
        s.parse().unwrap()
    }

    #[test]
    fn blocks_inverse_is_blocked_by() {
        assert_eq!(RelationType::Blocks.inverse(), RelationType::BlockedBy);
        assert_eq!(RelationType::BlockedBy.inverse(), RelationType::Blocks);
    }

    #[test]
    fn parent_child_inverse() {
        assert_eq!(RelationType::Parent.inverse(), RelationType::Child);
        assert_eq!(RelationType::Child.inverse(), RelationType::Parent);
    }

    #[test]
    fn relates_to_self_inverse() {
        assert_eq!(RelationType::RelatesTo.inverse(), RelationType::RelatesTo);
    }

    #[test]
    fn relationship_inverse() {
        let r = Relationship::new(id("TASK-001"), id("TASK-002"), RelationType::Blocks);
        let inv = r.inverse();
        assert_eq!(inv.from, id("TASK-002"));
        assert_eq!(inv.to, id("TASK-001"));
        assert_eq!(inv.rel_type, RelationType::BlockedBy);
    }

    #[test]
    fn relationship_display() {
        let r = Relationship::new(id("TASK-001"), id("TASK-002"), RelationType::Blocks);
        assert_eq!(r.to_string(), "TASK-001 blocks TASK-002");
    }
}
