use fmrs_core::position::bitboard::magics_generator::gen_magic as gen_magic_impl;

pub fn gen_magic() -> anyhow::Result<()> {
    gen_magic_impl()
}
