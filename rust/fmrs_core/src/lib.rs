#![allow(clippy::needless_range_loop, clippy::module_inception)]
#[macro_use]
extern crate lazy_static;

pub mod converter;
pub mod direction;
pub mod jkf;
pub mod memo;
pub mod nohash;
pub mod piece;
pub mod position;
pub mod search;
pub mod sfen;
pub mod solve;
pub mod config;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
