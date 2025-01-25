use fmrs_core::position::advance::pinned::magics_generator::gen_magic as gen_magic_pinned;
use fmrs_core::position::bitboard::magics_generator::gen_magic as gen_magic_reachable;
use rand::rngs::SmallRng;

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum MagicAttribute {
    Reachable,
    Pinned,
}

pub fn gen_magic(attr: MagicAttribute) -> anyhow::Result<()> {
    match attr {
        MagicAttribute::Reachable => gen_magic_reachable::<SmallRng>(),
        MagicAttribute::Pinned => gen_magic_pinned::<SmallRng>(),
    }
}
