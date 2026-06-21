//! `sigil-cluster` — roles, transport, sharding, catalog (DESIGN §5).
//!
//! The platform is a **modular monolith**: one binary that runs one or more
//! *roles* (`ingest`, `index`, `query`, `coordinator`). In monolith mode all
//! roles run in-process and communicate over an in-process [`Transport`]; in
//! scale-out mode each node runs a subset of roles and the transport is a real
//! bus (Kafka/Redpanda). This crate provides the role model, the transport
//! abstraction, the [`ShardMap`] (time + hash sharding with replication), and
//! the cluster [`catalog`] (membership + registries).
//!
//! Provided and tested here: roles, the in-process transport, sharding, and the
//! catalog data model. **Deferred** (need multi-node infra): Raft replication of
//! the catalog via `openraft`, and the Kafka/Redpanda transport implementation.
#![allow(dead_code)]

pub mod catalog;
pub mod role;
pub mod shard;
pub mod transport;
pub mod wire;

pub use catalog::{ClusterCatalog, Member, Membership, NodeId};
pub use role::{Role, RoleSet};
pub use shard::{ShardId, ShardMap, TimeBucket};
pub use transport::{build_transport, InProcTransport, Transport};
