pub struct Config {
    pub use_lance_magic: bool,
    pub use_bishop_magic: bool,
    pub use_rook_magic: bool,
}

pub const CONFIG: Config = Config {
    use_lance_magic: true,
    use_bishop_magic: true,
    use_rook_magic: true,
};
