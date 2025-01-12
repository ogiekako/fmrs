pub fn powerset(mask: u16) -> impl Iterator<Item = u16> {
    let n = 1 << mask.count_ones();
    let mut cur = mask;
    (0..n).map(move |_| {
        let res = cur;
        cur = mask & (cur.wrapping_sub(1));
        res
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_powerset() {
        let mask = 0b1010;
        let mut iter = powerset(mask);
        assert_eq!(iter.next(), Some(0b1010));
        assert_eq!(iter.next(), Some(0b1000));
        assert_eq!(iter.next(), Some(0b0010));
        assert_eq!(iter.next(), Some(0b0000));
        assert_eq!(iter.next(), None);
    }
}
