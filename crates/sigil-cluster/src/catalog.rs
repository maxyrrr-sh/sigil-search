//! Cluster catalog: membership + registries (DESIGN §5.3).
//!
//! This is the data model the coordinator owns. In scale-out it is replicated
//! across nodes by Raft (`openraft`) so every node sees a consistent view of
//! membership, the shard map, and the schema/index registry. **Raft replication
//! is deferred**; today this is a single-node, in-memory/JSON catalog with the
//! shapes the consensus layer will carry.

use serde::{Deserialize, Serialize};

use crate::shard::ShardMap;

/// Stable identifier of a node in the cluster.
pub type NodeId = String;

/// One cluster member and the roles it runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub node_id: NodeId,
    pub roles: Vec<String>,
    #[serde(default)]
    pub addr: Option<String>,
}

/// The set of nodes currently in the cluster.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Membership {
    pub members: Vec<Member>,
}

impl Membership {
    pub fn single(member: Member) -> Self {
        Membership {
            members: vec![member],
        }
    }

    pub fn upsert(&mut self, member: Member) {
        match self.members.iter_mut().find(|m| m.node_id == member.node_id) {
            Some(existing) => *existing = member,
            None => self.members.push(member),
        }
    }

    /// Node ids that run a given role.
    pub fn nodes_with_role(&self, role: &str) -> Vec<&NodeId> {
        self.members
            .iter()
            .filter(|m| m.roles.iter().any(|r| r == role))
            .map(|m| &m.node_id)
            .collect()
    }
}

/// The coordinator's view of the cluster.
#[derive(Debug, Clone)]
pub struct ClusterCatalog {
    pub membership: Membership,
    pub shard_map: ShardMap,
    /// Registered index/dataset names (would be Raft-replicated in scale-out).
    pub indexes: Vec<String>,
}

impl ClusterCatalog {
    pub fn new(self_member: Member, shard_map: ShardMap) -> Self {
        ClusterCatalog {
            membership: Membership::single(self_member),
            shard_map,
            indexes: Vec::new(),
        }
    }

    pub fn register_index(&mut self, name: impl Into<String>) {
        let name = name.into();
        if !self.indexes.contains(&name) {
            self.indexes.push(name);
        }
    }

    /// A JSON snapshot of the cluster state (for `cluster info` / debugging).
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "members": self.membership.members,
            "shards": self.shard_map.shards(),
            "replicas": self.shard_map.replicas(),
            "indexes": self.indexes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, roles: &[&str]) -> Member {
        Member {
            node_id: id.into(),
            roles: roles.iter().map(|s| s.to_string()).collect(),
            addr: None,
        }
    }

    #[test]
    fn membership_upsert_and_lookup() {
        let mut m = Membership::default();
        m.upsert(member("n1", &["ingest", "index"]));
        m.upsert(member("n2", &["query"]));
        m.upsert(member("n1", &["ingest"])); // update n1
        assert_eq!(m.members.len(), 2);
        assert_eq!(m.nodes_with_role("query"), vec![&"n2".to_string()]);
        assert_eq!(m.nodes_with_role("index").len(), 0);
    }

    #[test]
    fn catalog_snapshot() {
        let mut cat = ClusterCatalog::new(member("n1", &["index"]), ShardMap::new(4, 2));
        cat.register_index("logs");
        cat.register_index("logs"); // idempotent
        let snap = cat.snapshot();
        assert_eq!(snap["shards"], 4);
        assert_eq!(snap["indexes"], serde_json::json!(["logs"]));
    }
}
