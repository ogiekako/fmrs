pub fn highest_one_bit(i: u128) -> u128 {
    1 << (127 & !i.leading_zeros())
}

pub fn lowest_one_bit(i: u128) -> u128 {
    i & !(i.wrapping_add(1))
}
