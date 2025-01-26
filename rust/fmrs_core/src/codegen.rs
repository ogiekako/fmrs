use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};

use crate::{
    piece::Color,
    position::{BitBoard, Square},
};

pub(crate) struct DefWriter {
    w: TokenStream,
    visited: HashSet<&'static str>,
}

impl DefWriter {
    pub(crate) fn new() -> Self {
        DefWriter {
            w: TokenStream::new(),
            visited: HashSet::new(),
        }
    }

    pub(crate) fn write(&mut self, id: &'static str, tokens: TokenStream) {
        if self.visited.insert(id) {
            self.w.append_all(tokens);
        }
    }

    pub(crate) fn finish(self) -> TokenStream {
        self.w
    }
}

pub(crate) trait DefTokens {
    fn def_tokens(w: &mut DefWriter);
}

impl DefTokens for Color {
    fn def_tokens(w: &mut DefWriter) {
        w.write(
            "Color",
            quote! {
                use crate::piece::Color;
            },
        );
    }
}

impl DefTokens for Square {
    fn def_tokens(w: &mut DefWriter) {
        w.write(
            "Square",
            quote! {
                use crate::position::Square;
            },
        );
    }
}

impl DefTokens for BitBoard {
    fn def_tokens(w: &mut DefWriter) {
        w.write(
            "BitBoard",
            quote! {
                use crate::position::bitboard::BitBoard;
            },
        );
    }
}

pub(crate) struct ConstVec<'a, T> {
    name: TokenStream,
    ty: TokenStream,
    data: &'a Vec<T>,
}

impl<'a, T> ConstVec<'a, T> {
    pub(crate) fn new(name: TokenStream, ty: TokenStream, data: &'a Vec<T>) -> Self {
        ConstVec { name, ty, data }
    }
}

impl<T: ToTokens> ToTokens for ConstVec<'_, T> {
    fn to_tokens(&self, w: &mut TokenStream) {
        let name = &self.name;
        let ty = &self.ty;
        let len = self.data.len();
        let data = self.data;
        w.append_all(quote! {
            const #name: [#ty; #len] = [#(#data),*];
        });
    }
}
