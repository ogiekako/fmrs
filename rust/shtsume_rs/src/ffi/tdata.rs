use super::tdata_t;

#[derive(Debug, Clone, Default)]
pub struct Tdata(pub(super) tdata_t);

impl Tdata {
    pub fn pn(&self) -> u16 {
        self.0.pn
    }
    pub fn dn(&self) -> u16 {
        self.0.dn
    }
    pub fn sh(&self) -> u16 {
        self.0.sh
    }
}
