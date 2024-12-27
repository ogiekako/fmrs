use dashmap::DashMap;

use crate::nohash::{BuildNoHasher, NoHashMap};

pub trait MemoTrait {
    fn contains_key(&self, digest: &u64) -> bool;
    fn contains_or_insert(&mut self, digest: u64, step: u16) -> bool;
    fn get(&self, digest: &u64) -> Option<u16>;
    fn len(&self) -> usize;
}

#[derive(Debug, Clone)]
pub struct Memo {
    steps: NoHashMap<u16>,
}

impl Default for Memo {
    fn default() -> Self {
        let steps = NoHashMap::default().into();
        Memo { steps }
    }
}

impl<'a> MemoTrait for Memo {
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
    inner: &'a DashMemo,
}

impl MemoTrait for DashMemoMut<'_> {
    #[inline]
    fn contains_key(&self, digest: &u64) -> bool {
        self.inner.steps.contains_key(digest)
    }

    #[inline]
    fn contains_or_insert(&mut self, digest: u64, step: u16) -> bool {
        let mut contains = true;
        self.inner.steps.entry(digest).or_insert_with(|| {
            contains = false;
            step
        });
        contains
    }

    #[inline]
    fn get(&self, digest: &u64) -> Option<u16> {
        self.inner.steps.get(digest).map(|v| *v)
    }

    #[inline]
    fn len(&self) -> usize {
        self.inner.steps.len()
    }
}

impl DashMemo {
    pub fn as_mut(&self) -> DashMemoMut {
        DashMemoMut { inner: self }
    }
}
