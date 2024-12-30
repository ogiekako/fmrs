use super::{mlist_t, move_::Move_};

pub struct Mlist(*const mlist_t);

impl Mlist {
    pub(super) fn new(inner: *const mlist_t) -> Self {
        Self(inner)
    }
}

impl Iterator for Mlist {
    type Item = Move_;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_null() {
            return None;
        }
        let item = Move_::new((unsafe { *self.0 }).move_);

        self.0 = unsafe { (*self.0).next };

        Some(item)
    }
}
