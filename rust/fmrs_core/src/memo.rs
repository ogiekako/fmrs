use dashmap::DashMap;

use crate::nohash::{BuildNoHasher, NoHashMap};

pub trait MemoTrait {
    fn contains_key(&self, digest: &u64) -> bool;
    fn contains_or_insert(&mut self, digest: u64, step: u16) -> bool;
    fn get(&self, digest: &u64) -> Option<u16>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Default)]
pub struct MemoStub;

impl MemoTrait for MemoStub {
    #[inline]
    fn contains_key(&self, _digest: &u64) -> bool {
        unimplemented!()
    }

    #[inline]
    fn contains_or_insert(&mut self, _digest: u64, _step: u16) -> bool {
        unimplemented!()
    }

    #[inline]
    fn get(&self, _digest: &u64) -> Option<u16> {
        unimplemented!()
    }

    #[inline]
    fn len(&self) -> usize {
        unimplemented!()
    }
}

#[derive(Debug, Clone)]
pub struct Memo {
    steps: NoHashMap<u16>,
}

impl Memo {
    pub fn clear(&mut self) {
        self.steps.clear();
    }
}

impl Default for Memo {
    fn default() -> Self {
        let steps = NoHashMap::default();
        Memo { steps }
    }
}

impl MemoTrait for Memo {
    #[inline]
    fn contains_key(&self, digest: &u64) -> bool {
        self.steps.contains_key(digest)
    }

    #[inline]
    fn contains_or_insert(&mut self, digest: u64, step: u16) -> bool {
        let mut contains = true;
        self.steps.entry(digest).or_insert_with(|| {
            contains = false;
            step
        });
        contains
    }

    #[inline]
    fn get(&self, digest: &u64) -> Option<u16> {
        self.steps.get(digest).cloned()
    }

    #[inline]
    fn len(&self) -> usize {
        self.steps.len()
    }
}

#[derive(Debug, Default)]
pub struct DashMemo {
    steps: DashMap<u64, u16, BuildNoHasher>,
}

impl DashMemo {
    pub fn insert(&mut self, digest: u64, step: u16) {
        self.steps.insert(digest, step);
    }
}

pub struct DashMemoMut<'a> {
    steps: &'a DashMap<u64, u16, BuildNoHasher>,
}

impl MemoTrait for DashMemoMut<'_> {
    #[inline]
    fn contains_key(&self, digest: &u64) -> bool {
        self.steps.contains_key(digest)
    }

    #[inline]
    fn contains_or_insert(&mut self, digest: u64, step: u16) -> bool {
        let mut contains = true;
        self.steps.entry(digest).or_insert_with(|| {
            contains = false;
            step
        });
        contains
    }

    #[inline]
    fn get(&self, digest: &u64) -> Option<u16> {
        self.steps.get(digest).map(|v| *v)
    }

    #[inline]
    fn len(&self) -> usize {
        self.steps.len()
    }
}

impl DashMemo {
    pub fn as_mut(&self) -> DashMemoMut {
        DashMemoMut { steps: &self.steps }
    }
}
