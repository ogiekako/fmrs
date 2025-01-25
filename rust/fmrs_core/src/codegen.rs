use quote::{quote, ToTokens, TokenStreamExt};

use crate::position::BitBoard;

pub(super) trait WriteCode {
    fn write_def(_w: &mut impl TokenStreamExt) {}
    fn write_value(&self, w: &mut impl TokenStreamExt);
    fn tokens(&self) -> impl TokenStreamExt + ToTokens {
        let mut w = quote! {};
        self.write_value(&mut w);
        w
    }
}

trait Num: ToTokens {}

impl Num for u8 {}
impl Num for u16 {}
impl Num for u32 {}
impl Num for usize {}

impl<N: Num> WriteCode for N {
    fn write_value(&self, w: &mut impl TokenStreamExt) {
        w.append_all(quote! {
            #self
        });
    }
}

impl WriteCode for BitBoard {
    fn write_def(w: &mut impl TokenStreamExt) {
        w.append_all(quote! {
            use crate::position::bitboard::BitBoard;
            const fn b(x: u128) -> BitBoard {
                BitBoard::from_u128(x)
            }
        });
    }
    fn write_value(&self, w: &mut impl TokenStreamExt) {
        let x = self.u128();
        w.append_all(quote! {
            b(#x)
        });
    }
}

impl<T: WriteCode> WriteCode for Vec<T> {
    fn write_value(&self, w: &mut impl TokenStreamExt) {
        let mut tokens = vec![];
        for x in self {
            tokens.push(x.tokens());
        }
        w.append_all(quote! {
            [#(#tokens),*]
        });
    }
}
