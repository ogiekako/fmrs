use fmrs_core::position::bitboard::magics_generator::gen_magic as gen_magic_impl;
use rand::rngs::SmallRng;

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum MagicAttribute {
    Reachable,
    Pinned,
}

pub fn gen_magic(attr: MagicAttribute) -> anyhow::Result<()> {
    match attr {
        MagicAttribute::Reachable => gen_magic_impl::<SmallRng>(),
        MagicAttribute::Pinned => {
            todo!()
        }
    }
}
