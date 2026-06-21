//! Index sharding: time bucketing + hash distribution with replication (DESIGN §5.3).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// A shard index in `0..shards`.
pub type ShardId = u32;

/// Time granularity for time-based partitioning of segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeBucket {
    Hour,
    Day,
    Week,
}

impl TimeBucket {
    fn seconds(self) -> i64 {
        match self {
            TimeBucket::Hour => 3_600,
            TimeBucket::Day => 86_400,
            TimeBucket::Week => 604_800,
        }
    }

    /// Bucket number for a microsecond timestamp (e.g. day index since epoch).
    pub fn bucket(self, ts_micros: i64) -> i64 {
        (ts_micros / 1_000_000) / self.seconds()
    }
}

/// Maps routing keys + time to shards, and shards to replica placements.
#[derive(Debug, Clone, Copy)]
pub struct ShardMap {
    shards: u32,
    replicas: u32,
    bucket: TimeBucket,
}

impl ShardMap {
    /// `shards` >= 1; `replicas` is clamped to `1..=shards`.
    pub fn new(shards: u32, replicas: u32) -> Self {
        let shards = shards.max(1);
        let replicas = replicas.clamp(1, shards);
        ShardMap {
            shards,
            replicas,
            bucket: TimeBucket::Day,
        }
    }

    pub fn with_bucket(mut self, bucket: TimeBucket) -> Self {
        self.bucket = bucket;
        self
    }

    pub fn shards(&self) -> u32 {
        self.shards
    }

    pub fn replicas(&self) -> u32 {
        self.replicas
    }

    /// Primary shard for a routing key (hash distribution).
    pub fn shard_for_key(&self, key: &str) -> ShardId {
        let mut h = DefaultHasher::new();
        key.hash(&mut h);
        (h.finish() % self.shards as u64) as ShardId
    }

    /// Primary shard combining the time bucket with the routing key, so data is
    /// partitioned across both time and key space.
    pub fn shard_for(&self, key: &str, ts_micros: i64) -> ShardId {
        let mut h = DefaultHasher::new();
        key.hash(&mut h);
        self.bucket.bucket(ts_micros).hash(&mut h);
        (h.finish() % self.shards as u64) as ShardId
    }

    /// Replica placements for a primary shard: the primary plus the next
    /// `replicas-1` shards (mod `shards`).
    pub fn replica_shards(&self, primary: ShardId) -> Vec<ShardId> {
        (0..self.replicas)
            .map(|i| (primary + i) % self.shards)
            .collect()
    }

    /// All shards a query must fan out to (every shard).
    pub fn all_shards(&self) -> Vec<ShardId> {
        (0..self.shards).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_is_stable_and_bounded() {
        let m = ShardMap::new(4, 2);
        let a = m.shard_for_key("host-1");
        assert_eq!(a, m.shard_for_key("host-1"));
        assert!(a < 4);
    }

    #[test]
    fn time_changes_placement() {
        let m = ShardMap::new(8, 1);
        let day0 = m.shard_for("k", 0);
        let day100 = m.shard_for("k", 100 * 86_400 * 1_000_000);
        // Same key, different day bucket — usually a different shard.
        assert!(day0 < 8 && day100 < 8);
    }

    #[test]
    fn replica_placement_wraps() {
        let m = ShardMap::new(3, 2);
        assert_eq!(m.replica_shards(2), vec![2, 0]);
        assert_eq!(m.replica_shards(0), vec![0, 1]);
    }

    #[test]
    fn replicas_clamped_to_shards() {
        let m = ShardMap::new(2, 5);
        assert_eq!(m.replicas(), 2);
    }
}
