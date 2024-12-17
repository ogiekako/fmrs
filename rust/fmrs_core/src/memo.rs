use std::collections::hash_map::Entry;

use crate::nohash::NoHashMap;

#[derive(Debug, Clone)]
pub struct Memo {
    steps: NoHashMap<u32>,
}

impl Default for Memo {
    fn default() -> Self {
        let steps = NoHashMap::default();
        Memo { steps }
    }
}

impl Memo {
    #[inline]
    pub fn contains_key(&self, digest: &u64) -> bool {
        self.steps.contains_key(digest)
    }

    #[inline]
    pub fn insert(&mut self, digest: u64, step: u32) {
        self.steps.insert(digest, step);
    }

    #[inline]
    pub fn get(&self, digest: &u64) -> Option<&u32> {
        self.steps.get(digest)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    #[inline]
    pub fn entry(&mut self, digest: u64) -> Entry<'_, u64, u32> {
        self.steps.entry(digest)
    }
}
