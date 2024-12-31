use super::mkey_t;

#[repr(C)]
pub struct Mkey(pub(super) mkey_t);

impl From<mkey_t> for Mkey {
    fn from(inner: mkey_t) -> Self {
        Self::new(inner)
    }
}

impl From<Mkey> for mkey_t {
    fn from(mkey: Mkey) -> Self {
        mkey.0
    }
}

impl Mkey {
    pub(super) fn new(inner: mkey_t) -> Self {
        Self(inner)
    }

    pub fn ou(&self) -> u32 {
        self.0.ou()
    }
    pub fn fu(&self) -> u32 {
        self.0.fu()
    }
    pub fn ky(&self) -> u32 {
        self.0.ky()
    }
    pub fn ke(&self) -> u32 {
        self.0.ke()
    }
    pub fn gi(&self) -> u32 {
        self.0.gi()
    }
    pub fn ki(&self) -> u32 {
        self.0.ki()
    }
    pub fn ka(&self) -> u32 {
        self.0.ka()
    }
    pub fn hi(&self) -> u32 {
        self.0.hi()
    }

    pub fn as_u32(&self) -> u32 {
        unsafe { std::mem::transmute(self.0) }
    }
}
