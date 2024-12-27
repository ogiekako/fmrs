use crate::nohash::NoHashMap;

pub trait MemoTrait {
    fn contains_key(&self, digest: &u64) -> bool;
    fn contains_or_insert(&mut self, digest: u64, step: u16) -> bool;
    fn get(&self, digest: &u64) -> Option<&u16>;
    fn len(&self) -> usize;
}

#[derive(Debug, Clone)]
pub struct Memo {
    steps: NoHashMap<u16>,
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
    fn get(&self, digest: &u64) -> Option<&u16> {
        self.steps.get(digest)
    }

    #[inline]
    fn len(&self) -> usize {
        self.steps.len()
    }
}
