#[macro_use]
extern crate lazy_static;

pub mod jkf;
pub mod piece;
pub mod position;
pub mod sfen;
pub mod solve;
pub mod converter;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
