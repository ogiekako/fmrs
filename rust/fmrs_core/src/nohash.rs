use std::{
    collections::{HashMap, HashSet},
    hash::{BuildHasher, Hasher},
};

#[derive(Debug, Clone)]
pub struct NoHasher {
    hash: u64,
}

impl Default for NoHasher {
    #[inline]
    fn default() -> NoHasher {
        NoHasher { hash: 0 }
    }
}

impl Hasher for NoHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }

    fn write(&mut self, _bytes: &[u8]) {
        unimplemented!()
    }

    #[inline]
    fn write_u64(&mut self, hash: u64) {
        debug_assert_eq!(self.hash, 0);
        self.hash = hash;
    }
}

#[derive(Copy, Default, Debug, Clone)]
pub struct BuildNoHasher;

impl BuildHasher for BuildNoHasher {
    type Hasher = NoHasher;

    fn build_hasher(&self) -> Self::Hasher {
        NoHasher::default()
    }
}

pub type NoHashSet<K> = HashSet<K, BuildNoHasher>;
pub type NoHashMap<K, V> = HashMap<K, V, BuildNoHasher>;
pub type NoHashSet64 = NoHashSet<u64>;
pub type NoHashMap64<V> = NoHashMap<u64, V>;

pub const SHARD_BITS: u32 = 6;
pub const NUM_SHARDS: usize = 1 << SHARD_BITS; // 64

#[inline]
pub fn shard_index_64(key: u64) -> usize {
    (key >> (64 - SHARD_BITS)) as usize
}

pub struct ShardedMap64<V> {
    shards: [NoHashMap64<V>; NUM_SHARDS],
}

impl<V> Default for ShardedMap64<V> {
    fn default() -> Self {
        Self {
            shards: std::array::from_fn(|_| NoHashMap64::default()),
        }
    }
}

impl<V> ShardedMap64<V> {
    #[inline]
    pub fn get(&self, key: &u64) -> Option<&V> {
        self.shards[shard_index_64(*key)].get(key)
    }

    #[inline]
    pub fn contains_key(&self, key: &u64) -> bool {
        self.shards[shard_index_64(*key)].contains_key(key)
    }

    #[inline]
    pub fn insert(&mut self, key: u64, value: V) -> Option<V> {
        self.shards[shard_index_64(key)].insert(key, value)
    }

    #[inline]
    pub fn remove(&mut self, key: &u64) -> Option<V> {
        self.shards[shard_index_64(*key)].remove(key)
    }

    pub fn len(&self) -> usize {
        self.shards.iter().map(|s| s.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.shards.iter().all(|s| s.is_empty())
    }

    pub fn shards_mut(&mut self) -> &mut [NoHashMap64<V>; NUM_SHARDS] {
        &mut self.shards
    }

    pub fn shards(&self) -> &[NoHashMap64<V>; NUM_SHARDS] {
        &self.shards
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u64, &V)> {
        self.shards.iter().flat_map(|s| s.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_hash_map() {
        let mut map = NoHashMap64::default();
        map.insert(1, 2);
        assert_eq!(map.get(&1), Some(&2));
        map.insert(1, 3);
        assert_eq!(map.get(&1), Some(&3));
        assert_eq!(map.get(&2), None);
        map.insert(2, 4);
        assert_eq!(map.get(&2), Some(&4));
    }
}
