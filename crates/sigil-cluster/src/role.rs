//! Runtime roles a node can take on (DESIGN §5.1).

use std::collections::BTreeSet;

/// A runtime role. A node runs one or more of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// Receive raw events from inputs and run the processing pipeline.
    Ingest,
    /// Consume normalized events and write/serve the index + tiers.
    Index,
    /// Serve the search/SQL/DSL query API.
    Query,
    /// Maintain cluster membership, the shard map, and registries.
    Coordinator,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Ingest => "ingest",
            Role::Index => "index",
            Role::Query => "query",
            Role::Coordinator => "coordinator",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ingest" => Some(Role::Ingest),
            "index" => Some(Role::Index),
            "query" => Some(Role::Query),
            "coordinator" => Some(Role::Coordinator),
            _ => None,
        }
    }

    pub const ALL: [Role; 4] = [Role::Ingest, Role::Index, Role::Query, Role::Coordinator];
}

/// The set of roles active on a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleSet(BTreeSet<Role>);

impl RoleSet {
    /// Every role (monolith mode).
    pub fn all() -> Self {
        RoleSet(Role::ALL.into_iter().collect())
    }

    /// Build a role set from config `targets`. `all` (or empty) means every role.
    pub fn from_targets(targets: &[String]) -> anyhow::Result<Self> {
        if targets.is_empty() || targets.iter().any(|t| t.eq_ignore_ascii_case("all")) {
            return Ok(RoleSet::all());
        }
        let mut set = BTreeSet::new();
        for t in targets {
            let role = Role::parse(t)
                .ok_or_else(|| anyhow::anyhow!("unknown role target '{t}'"))?;
            set.insert(role);
        }
        Ok(RoleSet(set))
    }

    pub fn has(&self, role: Role) -> bool {
        self.0.contains(&role)
    }

    pub fn iter(&self) -> impl Iterator<Item = Role> + '_ {
        self.0.iter().copied()
    }

    pub fn labels(&self) -> Vec<&'static str> {
        self.0.iter().map(|r| r.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_and_explicit() {
        assert_eq!(RoleSet::from_targets(&["all".into()]).unwrap(), RoleSet::all());
        assert_eq!(RoleSet::from_targets(&[]).unwrap(), RoleSet::all());

        let rs = RoleSet::from_targets(&["ingest".into(), "query".into()]).unwrap();
        assert!(rs.has(Role::Ingest));
        assert!(rs.has(Role::Query));
        assert!(!rs.has(Role::Index));
    }

    #[test]
    fn unknown_role_errors() {
        assert!(RoleSet::from_targets(&["frobnicate".into()]).is_err());
    }
}
