use super::move_t;

#[derive(Debug, Clone, Copy)]
pub struct Move_(move_t);

impl Move_ {
    pub(super) fn new(move_: move_t) -> Self {
        Self(move_)
    }

    pub fn prev_pos(&self) -> u8 {
        self.0.prev_pos
    }
    pub fn new_pos(&self) -> u8 {
        self.0.new_pos & 0x7f
    }
    pub fn is_promote(&self) -> bool {
        self.0.new_pos & 0x80 != 0
    }
    pub fn is_move(&self) -> bool {
        self.0.prev_pos < super::N_SQUARE as u8
    }
    pub fn is_drop(&self) -> bool {
        self.0.prev_pos >= super::HAND as u8
    }
    pub fn hand(&self) -> u8 {
        self.0.prev_pos - super::HAND as u8
    }
}
