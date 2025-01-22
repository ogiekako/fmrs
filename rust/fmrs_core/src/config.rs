pub struct Config {
    pub use_bishop_magic: bool,
    pub use_rook_magic: bool,
    pub pinned_v: usize,
}

pub const CONFIG: Config = Config {
    use_bishop_magic: true,
    use_rook_magic: true,
    pinned_v: 0,
};
