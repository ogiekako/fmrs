use super::komainf_t;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Komainf(komainf_t);

impl From<komainf_t> for Komainf {
    fn from(inner: komainf_t) -> Self {
        Self::new(inner)
    }
}

impl From<Komainf> for komainf_t {
    fn from(komainf: Komainf) -> Self {
        komainf.0
    }
}

impl Komainf {
    pub const FU: Self = Self(super::_KType_FU as komainf_t);
    pub const KY: Self = Self(super::_KType_KY as komainf_t);
    pub const KE: Self = Self(super::_KType_KE as komainf_t);
    pub const GI: Self = Self(super::_KType_GI as komainf_t);
    pub const KI: Self = Self(super::_KType_KI as komainf_t);
    pub const KA: Self = Self(super::_KType_KA as komainf_t);
    pub const HI: Self = Self(super::_KType_HI as komainf_t);

    pub const SPC: Self = Self(super::_Koma_SPC as komainf_t);
    pub const SFU: Self = Self(super::_Koma_SFU as komainf_t);
    pub const SKY: Self = Self(super::_Koma_SKY as komainf_t);
    pub const SKE: Self = Self(super::_Koma_SKE as komainf_t);
    pub const SGI: Self = Self(super::_Koma_SGI as komainf_t);
    pub const SKI: Self = Self(super::_Koma_SKI as komainf_t);
    pub const SKA: Self = Self(super::_Koma_SKA as komainf_t);
    pub const SHI: Self = Self(super::_Koma_SHI as komainf_t);
    pub const SOU: Self = Self(super::_Koma_SOU as komainf_t);
    pub const STO: Self = Self(super::_Koma_STO as komainf_t);
    pub const SNY: Self = Self(super::_Koma_SNY as komainf_t);
    pub const SNK: Self = Self(super::_Koma_SNK as komainf_t);
    pub const SNG: Self = Self(super::_Koma_SNG as komainf_t);
    pub const SUM: Self = Self(super::_Koma_SUM as komainf_t);
    pub const SRY: Self = Self(super::_Koma_SRY as komainf_t);
    pub const GFU: Self = Self(super::_Koma_GFU as komainf_t);
    pub const GKY: Self = Self(super::_Koma_GKY as komainf_t);
    pub const GKE: Self = Self(super::_Koma_GKE as komainf_t);
    pub const GGI: Self = Self(super::_Koma_GGI as komainf_t);
    pub const GKI: Self = Self(super::_Koma_GKI as komainf_t);
    pub const GKA: Self = Self(super::_Koma_GKA as komainf_t);
    pub const GHI: Self = Self(super::_Koma_GHI as komainf_t);
    pub const GOU: Self = Self(super::_Koma_GOU as komainf_t);
    pub const GTO: Self = Self(super::_Koma_GTO as komainf_t);
    pub const GNY: Self = Self(super::_Koma_GNY as komainf_t);
    pub const GNK: Self = Self(super::_Koma_GNK as komainf_t);
    pub const GNG: Self = Self(super::_Koma_GNG as komainf_t);
    pub const GUM: Self = Self(super::_Koma_GUM as komainf_t);
    pub const GRY: Self = Self(super::_Koma_GRY as komainf_t);

    pub(super) fn new(inner: komainf_t) -> Self {
        Self(inner)
    }

    pub fn sente(self) -> bool {
        // SENTE_KOMA
        self.0 != 0 && self.0 < 16
    }

    pub fn gote(self) -> bool {
        // GOTE_KOMA
        self.0 > 16
    }
}
