use std::cell::RefCell;

use dashmap::DashMap;

use crate::nohash::{BuildNoHasher, NoHashMap};

pub trait MemoTrait {
    fn contains_key(&self, digest: &u64) -> bool;
    fn contains_or_insert(&self, digest: u64, step: u16) -> bool;
    fn get(&self, digest: &u64) -> Option<u16>;
    fn len(&self) -> usize;
}

#[derive(Debug, Clone)]
pub struct Memo {
    steps: RefCell<NoHashMap<u16>>,
}

impl Default for Memo {
    fn default() -> Self {
        let steps = NoHashMap::default().into();
        Memo { steps }
    }
}

impl MemoTrait for Memo {
    #[inline]
    fn contains_key(&self, digest: &u64) -> bool {
        self.steps.borrow_mut().contains_key(digest)
    }

    #[inline]
    fn contains_or_insert(&self, digest: u64, step: u16) -> bool {
        let mut contains = true;
        self.steps.borrow_mut().entry(digest).or_insert_with(|| {
            contains = false;
            step
        });
        contains
    }

    #[inline]
    fn get(&self, digest: &u64) -> Option<u16> {
        self.steps.borrow().get(digest).cloned()
    }

    #[inline]
    fn len(&self) -> usize {
        self.steps.borrow().len()
    }
}

#[derive(Debug, Clone)]
pub struct DashMemo {
    steps: DashMap<u64, u16, BuildNoHasher>,
}

impl Default for DashMemo {
    fn default() -> Self {
        let steps = DashMap::default();
        DashMemo { steps }
    }
}

impl MemoTrait for DashMemo {
    #[inline]
    fn contains_key(&self, digest: &u64) -> bool {
        self.steps.contains_key(digest)
    }

    #[inline]
    fn contains_or_insert(&self, digest: u64, step: u16) -> bool {
        if self.steps.contains_key(&digest) {
            return true;
        }
        self.steps.insert(digest, step);
        false
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
