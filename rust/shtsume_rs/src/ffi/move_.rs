use super::{komainf::Komainf, komainf_t, move_t};

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Move_(pub(super) move_t);

impl Move_ {
    pub(super) fn new(move_: move_t) -> Self {
        Self(move_)
    }

    pub fn prev_pos(&self) -> u8 {
        // PREV_POS
        self.0.prev_pos
    }
    pub fn new_pos(&self) -> u8 {
        // NEW_POS
        self.0.new_pos & 0x7f
    }
    pub fn is_promote(&self) -> bool {
        // PROMOTE
        self.0.new_pos & 0x80 != 0
    }
    pub fn is_move(&self) -> bool {
        // MV_MOVE
        self.0.prev_pos < super::N_SQUARE as u8
    }
    pub fn is_drop(&self) -> bool {
        //MV_DROP
        self.0.prev_pos >= super::HAND as u8
    }
    pub fn hand(&self) -> Komainf {
        // MV_HAND
        Komainf::new((self.0.prev_pos - super::HAND as u8) as komainf_t)
    }
}
