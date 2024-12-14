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
        // https://docs.rs/rustc-hash/2.1.0/src/rustc_hash/lib.rs.html
        self.hash = hash.wrapping_mul(0xf1357aea2e62a9c5).rotate_left(20);
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

pub type NoHashMap<V> = HashMap<u64, V, BuildNoHasher>;
pub type NoHashSet = HashSet<u64, BuildNoHasher>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_hash_map() {
        let mut map = NoHashMap::default();
        map.insert(1, 2);
        assert_eq!(map.get(&1), Some(&2));
        map.insert(1, 3);
        assert_eq!(map.get(&1), Some(&3));
        assert_eq!(map.get(&2), None);
        map.insert(2, 4);
        assert_eq!(map.get(&2), Some(&4));
    }
}
