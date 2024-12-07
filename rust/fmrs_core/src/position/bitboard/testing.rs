macro_rules! bitboard {
    ($($x:expr,)*) => {
        {
            let v = vec![$($x),*];
            if v.len() != 9 {
                panic!("Exactly 9 elements should be given.");
            }
            let mut res = crate::position::bitboard::BitBoard::default();
            for i in 0..9 {
                if v[i].len() != 9 {
                    panic!("v[{}] = {:?} should contain exactly 9 characters.", i, v[i]);
                }
                for (j, c) in v[i].chars().rev().enumerate() {
                    if c == '*' {
                        res.set(crate::position::bitboard::Square::new(j, i));
                    }
                }
            }
            res
        }
    }
}
pub(super) use bitboard;
