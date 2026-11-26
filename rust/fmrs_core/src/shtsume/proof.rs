//! Proof/disproof piece computation and branchless 64-bit sort keys (ported from shtsume's `nbase.c`).

use std::cmp::Ordering;

use crate::shtsume::backend::PositionBackend;
use crate::shtsume::types::{IS_SKOMA, MKey, MKeyCmp, MvItem, PROMOTED, FU, GI, HI, KA, KE, KI, KY};

/// Branchless 64-bit sort key for OR nodes (attacker / 詰方), encoding all 13 tie-breaking
/// levels of `proof_number_comp` into a single `u64` integer comparison.
#[inline(always)]
pub fn or_sort_key(a: &MvItem) -> u64 {
    let pn = u64::from(a.tdata.pn.min(0x7fff));
    let dn_raw = a.tdata.dn.min(0x7fff);
    // Break `pn` ties among `unknown` moves (`0 < pn < INFINATE`) by descending `dn`
    // so forcing checks (hard for defender to refute) stay ahead of easily refuted siblings.
    let dn = if a.tdata.pn > 0 && a.tdata.pn < 0x7fff {
        u64::from(0x7fff - dn_raw)
    } else {
        u64::from(dn_raw)
    };
    let not_imm = u64::from(!(a.tdata.pn == 0 && a.tdata.sh == 0));
    let not_inc = u64::from(!a.inc());
    let sh = u64::from(a.tdata.sh.min(0x7ff));
    let pr = u64::from(a.pr());
    let cu = u64::from(a.cu());
    let nouse2 = u64::from(a.nouse2.min(63));
    let inv_cap = u64::from(31 - (a.cap_koma & 31));
    let amv = a.head_move();
    let is_board = amv.is_board_move();
    let not_board = u64::from(!is_board);
    let not_prom = u64::from(!amv.is_promote());
    let inv_drop = if amv.is_drop() {
        u64::from(7 - (amv.hand_ktype() & 7))
    } else {
        0
    };
    let length = u64::from(a.length.min(7));

    let tie13 = if a.ml_tie != 0 {
        (u64::from(a.ml_tie) << 5) | (inv_cap & 0x1f)
    } else {
        (inv_cap << 8) | (not_board << 7) | (not_prom << 6) | (inv_drop << 3) | length
    };

    (pn << 49)
        | (dn << 34)
        | (not_imm << 33)
        | (not_inc << 32)
        | (sh << 21)
        | (pr << 20)
        | (cu << 19)
        | (nouse2 << 13)
        | tie13
}

/// Branchless 64-bit sort key for AND nodes (defender / 玉方), encoding all 11 tie-breaking
/// levels of `disproof_number_comp` into a single `u64` integer comparison.
#[inline(always)]
pub fn and_sort_key(a: &MvItem) -> u64 {
    let dn = u64::from(a.tdata.dn.min(0x7fff));
    let pn = u64::from(a.tdata.pn.min(0x7fff));
    let inc = u64::from(a.inc());
    let nouse2 = u64::from(a.nouse2.min(63));
    let sh_clamped = a.tdata.sh.min(0x7ff);
    let sh_ord = u64::from(if a.tdata.pn == 0 {
        0x7ff - sh_clamped
    } else {
        sh_clamped
    });
    let amv = a.head_move();
    let is_board = amv.is_board_move();
    let not_board = u64::from(!is_board);
    let inv_cap = u64::from(31 - (a.cap_koma & 31));
    let src_ord = if is_board {
        u64::from(a.src_koma & 31)
    } else {
        0
    };
    let not_prom = u64::from(!amv.is_promote());
    let drop_ord = if amv.is_drop() {
        u64::from(amv.hand_ktype() & 7)
    } else {
        0
    };

    let tie15 = if a.ml_tie != 0 {
        (not_board << 14) | (u64::from(a.ml_tie.min(127)) << 7) | (inv_cap << 2) | not_prom
    } else {
        (not_board << 14) | (inv_cap << 9) | (src_ord << 4) | (not_prom << 3) | drop_ord
    };

    (dn << 48)
        | (pn << 33)
        | (inc << 32)
        | (nouse2 << 26)
        | (sh_ord << 15)
        | tie15
}

#[inline(always)]
fn small_insertion_sort(list: &mut [MvItem]) {
    let len = list.len();
    for i in 1..len {
        let key = list[i].sort_key;
        if key < list[i - 1].sort_key {
            let mut dst = i - 1;
            while dst > 0 && key < list[dst - 1].sort_key {
                dst -= 1;
            }
            unsafe {
                let tmp = std::ptr::read(list.as_ptr().add(i));
                std::ptr::copy(list.as_ptr().add(dst), list.as_mut_ptr().add(dst + 1), i - dst);
                std::ptr::write(list.as_mut_ptr().add(dst), tmp);
            }
        }
    }
}

/// Precompute `sort_key` once per item (`O(N)`) and stably sort OR-node moves by a single `u64`.
#[inline]
pub fn sort_or(list: &mut [MvItem]) {
    let len = list.len();
    if len <= 1 {
        return;
    }
    for item in list.iter_mut() {
        item.sort_key = or_sort_key(item);
    }
    if len == 2 {
        if list[0].sort_key > list[1].sort_key {
            list.swap(0, 1);
        }
    } else if len <= 24 {
        small_insertion_sort(list);
    } else {
        list.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    }
}

#[inline(always)]
fn shift_head_to(list: &mut [MvItem], key: u64) {
    let len = list.len();
    if len <= 1 || key <= list[1].sort_key {
        return;
    }
    let mut dst = 1;
    while dst + 1 < len && key > list[dst + 1].sort_key {
        dst += 1;
    }
    // Shift `list[1..=dst]` left by 1 slot in a single `memmove` instead of `dst` 40B swaps.
    unsafe {
        let head = std::ptr::read(list.as_ptr());
        std::ptr::copy(list.as_ptr().add(1), list.as_mut_ptr(), dst);
        std::ptr::write(list.as_mut_ptr().add(dst), head);
    }
}

/// Refresh `list[0].sort_key` and insert `list[0]` into sorted position in `O(N)` `u64` comparisons.
#[inline]
pub fn reorder_first_or(list: &mut [MvItem]) {
    if list.len() <= 1 {
        return;
    }
    let key = or_sort_key(&list[0]);
    list[0].sort_key = key;
    debug_assert!(list[1..].iter().all(|x| x.sort_key == or_sort_key(x)));
    shift_head_to(list, key);
}

/// Precompute `sort_key` once per item (`O(N)`) and stably sort AND-node moves by a single `u64`.
#[inline]
pub fn sort_and(list: &mut [MvItem]) {
    let len = list.len();
    if len <= 1 {
        return;
    }
    for item in list.iter_mut() {
        item.sort_key = and_sort_key(item);
    }
    if len == 2 {
        if list[0].sort_key > list[1].sort_key {
            list.swap(0, 1);
        }
    } else if len <= 24 {
        small_insertion_sort(list);
    } else {
        list.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    }
}

/// Refresh `list[0].sort_key` and insert `list[0]` into sorted position in `O(N)` `u64` comparisons.
#[inline]
pub fn reorder_first_and(list: &mut [MvItem]) {
    if list.len() <= 1 {
        return;
    }
    let key = and_sort_key(&list[0]);
    list[0].sort_key = key;
    debug_assert!(list[1..].iter().all(|x| x.sort_key == and_sort_key(x)));
    shift_head_to(list, key);
}

/// Move ordering comparator for OR nodes (attacker / 詰方), matching `proof_number_comp`.
#[inline(always)]
pub fn proof_number_comp(a: &MvItem, b: &MvItem) -> Ordering {
    or_sort_key(a).cmp(&or_sort_key(b))
}

/// Move ordering comparator for AND nodes (defender / 玉方), matching `disproof_number_comp`.
#[inline(always)]
pub fn disproof_number_comp(a: &MvItem, b: &MvItem) -> Ordering {
    and_sort_key(a).cmp(&and_sort_key(b))
}

/// Proof pieces at a checkmate leaf (AND node with no evasions).
pub fn proof_koma<P: PositionBackend>(pos: &P) -> MKey {
    if pos.n_oute() == 2 {
        return MKey::EMPTY;
    }
    let atk_koma = pos.board_at(pos.attack_sq());
    if IS_SKOMA[atk_koma as usize] {
        return MKey::EMPTY;
    }
    let attacker = pos.turn().flip();
    let defender = pos.turn();
    let mut mkey = pos.mkey(attacker);
    let def_mkey = pos.mkey(defender);
    if def_mkey.hi() != 0 {
        mkey.set_hi(0);
    }
    if def_mkey.ka() != 0 {
        mkey.set_ka(0);
    }
    if def_mkey.ki() != 0 {
        mkey.set_ki(0);
    }
    if def_mkey.gi() != 0 {
        mkey.set_gi(0);
    }
    if def_mkey.ke() != 0 {
        mkey.set_ke(0);
    }
    if def_mkey.ky() != 0 {
        mkey.set_ky(0);
    }
    if def_mkey.fu() != 0 {
        mkey.set_fu(0);
    }
    mkey
}

/// Simplified OR sort key for `simple_mate` mode (omits `hinc` / `nouse2`; prefers `equal_tsumi` over `super_tsumi`, `sh` up to 8191).
#[inline(always)]
pub fn or_sort_key_simple(a: &MvItem) -> u64 {
    let pn = u64::from(a.tdata.pn.min(0x7fff));
    let dn_raw = a.tdata.dn.min(0x7fff);
    let dn = if a.tdata.pn > 0 && a.tdata.pn < 0x7fff {
        u64::from(0x7fff - dn_raw)
    } else {
        u64::from(dn_raw)
    };
    let not_imm = u64::from(!(a.tdata.pn == 0 && a.tdata.sh == 0));
    let cu = u64::from(a.cu());
    let is_super = u64::from(a.inc());
    let sh = u64::from(a.tdata.sh.min(0x1fff));
    let pr = u64::from(a.pr());
    let inv_cap = u64::from(31 - (a.cap_koma & 31));
    let amv = a.head_move();
    let is_board = amv.is_board_move();
    let not_board = u64::from(!is_board);
    let not_prom = u64::from(!amv.is_promote());
    let inv_drop = if amv.is_drop() {
        u64::from(7 - (amv.hand_ktype() & 7))
    } else {
        0
    };
    let length = u64::from(a.length.min(7));
    let tie13 = if a.ml_tie != 0 {
        (u64::from(a.ml_tie) << 5) | (inv_cap & 0x1f)
    } else {
        (inv_cap << 8) | (not_board << 7) | (not_prom << 6) | (inv_drop << 3) | length
    };
    (pn << 49)
        | (dn << 34)
        | (not_imm << 33)
        | (cu << 32)
        | (sh << 19)
        | (is_super << 18)
        | (pr << 17)
        | tie13
}

/// Simplified AND sort key for `simple_mate` mode (omits `inc`, `nouse2`; orders `pn == 0` by max `sh` up to 8191).
#[inline(always)]
pub fn and_sort_key_simple(a: &MvItem) -> u64 {
    let dn = u64::from(a.tdata.dn.min(0x7fff));
    let pn = u64::from(a.tdata.pn.min(0x7fff));
    let sh_clamped = a.tdata.sh.min(0x1fff);
    let sh_ord = u64::from(if a.tdata.pn == 0 {
        0x1fff - sh_clamped
    } else {
        sh_clamped
    });
    let amv = a.head_move();
    let is_board = amv.is_board_move();
    let not_board = u64::from(!is_board);
    let inv_cap = u64::from(31 - (a.cap_koma & 31));
    let src_ord = if is_board {
        u64::from(a.src_koma & 31)
    } else {
        0
    };
    let not_prom = u64::from(!amv.is_promote());
    let drop_ord = if amv.is_drop() {
        u64::from(amv.hand_ktype() & 7)
    } else {
        0
    };
    let tie15 = if a.tdata.pn != 0 && a.ml_tie != 0 {
        (not_board << 14) | (u64::from(a.ml_tie.min(127)) << 7) | (inv_cap << 2) | not_prom
    } else {
        (not_board << 14) | (inv_cap << 9) | (src_ord << 4) | (not_prom << 3) | drop_ord
    };
    (dn << 48) | (pn << 33) | (sh_ord << 18) | tie15
}

#[inline]
pub fn sort_or_simple(list: &mut [MvItem]) {
    let len = list.len();
    if len <= 1 {
        return;
    }
    for item in list.iter_mut() {
        item.sort_key = or_sort_key_simple(item);
    }
    if len == 2 {
        if list[0].sort_key > list[1].sort_key {
            list.swap(0, 1);
        }
    } else if len <= 24 {
        small_insertion_sort(list);
    } else {
        list.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    }
}

#[inline]
pub fn reorder_first_or_simple(list: &mut [MvItem]) {
    if list.len() <= 1 {
        return;
    }
    let key = or_sort_key_simple(&list[0]);
    list[0].sort_key = key;
    shift_head_to(list, key);
}

#[inline]
pub fn sort_and_simple(list: &mut [MvItem]) {
    let len = list.len();
    if len <= 1 {
        return;
    }
    for item in list.iter_mut() {
        item.sort_key = and_sort_key_simple(item);
    }
    if len == 2 {
        if list[0].sort_key > list[1].sort_key {
            list.swap(0, 1);
        }
    } else if len <= 24 {
        small_insertion_sort(list);
    } else {
        list.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    }
}

#[inline]
pub fn reorder_first_and_simple(list: &mut [MvItem]) {
    if list.len() <= 1 {
        return;
    }
    let key = and_sort_key_simple(&list[0]);
    list[0].sort_key = key;
    shift_head_to(list, key);
}

/// Minimal proof-hand propagation at an OR node for `simple_mate` mode (omits `hinc`, `inc`, `nouse`).
#[inline]
pub fn proof_koma_or_simple(mvlist: &mut MvItem, best_child: &MvItem) {
    mvlist.mkey = best_child.mkey;
    let mv = best_child.head_move();
    if mv.is_drop() {
        mvlist.mkey.inc(mv.hand_ktype());
    } else if best_child.cap_koma != 0 {
        let ktype = best_child.cap_koma % PROMOTED;
        if mvlist.mkey.get(ktype) > 0 {
            mvlist.mkey.dec(ktype);
        }
    }
}

/// Minimal proof-hand propagation at an AND node for `simple_mate` mode (omits `hinc`, `inc`, `nouse`).
#[inline]
pub fn proof_koma_and_simple<P: PositionBackend>(pos: &P, mvlist: &mut MvItem, children: &[MvItem]) {
    let mut mkey = MKey::EMPTY;
    for child in children {
        mkey = mkey.max(child.mkey);
    }
    let self_mkey = pos.mkey(pos.turn());
    let enemy_mkey = pos.mkey(pos.turn().flip());
    if self_mkey.hi() == 0 && mkey.hi() != enemy_mkey.hi() {
        mkey.set_hi(enemy_mkey.hi());
    }
    if self_mkey.ka() == 0 && mkey.ka() != enemy_mkey.ka() {
        mkey.set_ka(enemy_mkey.ka());
    }
    if self_mkey.ki() == 0 && mkey.ki() != enemy_mkey.ki() {
        mkey.set_ki(enemy_mkey.ki());
    }
    if self_mkey.gi() == 0 && mkey.gi() != enemy_mkey.gi() {
        mkey.set_gi(enemy_mkey.gi());
    }
    if self_mkey.ke() == 0 && mkey.ke() != enemy_mkey.ke() {
        mkey.set_ke(enemy_mkey.ke());
    }
    if self_mkey.ky() == 0 && mkey.ky() != enemy_mkey.ky() {
        mkey.set_ky(enemy_mkey.ky());
    }
    if self_mkey.fu() == 0 && mkey.fu() != enemy_mkey.fu() {
        mkey.set_fu(enemy_mkey.fu());
    }
    mvlist.mkey = mkey;
}

/// Propagate proof pieces upward at an OR node (`proof_koma_or`).
pub fn proof_koma_or<P: PositionBackend>(pos: &P, mvlist: &mut MvItem, best_child: &MvItem) {
    mvlist.mkey = best_child.mkey;
    let mut hinc = best_child.hinc();
    mvlist.nouse = best_child.nouse;

    let mv = best_child.head_move();
    if mv.is_drop() {
        mvlist.mkey.inc(mv.hand_ktype());
    } else {
        let cap = best_child.cap_koma;
        if cap != 0 {
            let ktype = cap % PROMOTED;
            if mvlist.mkey.get(ktype) == 0 {
                hinc = true;
                mvlist.nouse += 1;
            } else {
                mvlist.mkey.dec(ktype);
            }
        }
    }
    mvlist.set_hinc(hinc);

    let self_mkey = pos.mkey(pos.turn());
    let cmp_res = self_mkey.compare(mvlist.mkey);
    mvlist.set_inc(if cmp_res == MKeyCmp::Equal { hinc } else { true });
    mvlist.nouse2 = self_mkey
        .total()
        .saturating_sub(mvlist.mkey.total())
        .saturating_add(mvlist.nouse);
}

/// Propagate proof pieces upward at an AND node (`proof_koma_and`).
pub fn proof_koma_and<P: PositionBackend>(pos: &P, mvlist: &mut MvItem, children: &[MvItem]) {
    let mut mkey = MKey::EMPTY;
    for child in children {
        mkey = mkey.max(child.mkey);
    }

    let self_mkey = pos.mkey(pos.turn());
    let enemy_mkey = pos.mkey(pos.turn().flip());
    if self_mkey.hi() == 0 && mkey.hi() != enemy_mkey.hi() {
        mkey.set_hi(enemy_mkey.hi());
    }
    if self_mkey.ka() == 0 && mkey.ka() != enemy_mkey.ka() {
        mkey.set_ka(enemy_mkey.ka());
    }
    if self_mkey.ki() == 0 && mkey.ki() != enemy_mkey.ki() {
        mkey.set_ki(enemy_mkey.ki());
    }
    if self_mkey.gi() == 0 && mkey.gi() != enemy_mkey.gi() {
        mkey.set_gi(enemy_mkey.gi());
    }
    if self_mkey.ke() == 0 && mkey.ke() != enemy_mkey.ke() {
        mkey.set_ke(enemy_mkey.ke());
    }
    if self_mkey.ky() == 0 && mkey.ky() != enemy_mkey.ky() {
        mkey.set_ky(enemy_mkey.ky());
    }
    if self_mkey.fu() == 0 && mkey.fu() != enemy_mkey.fu() {
        mkey.set_fu(enemy_mkey.fu());
    }

    mvlist.mkey = mkey;
    let head = &children[0];
    mvlist.nouse = head
        .nouse
        .saturating_add(mkey.total().saturating_sub(head.mkey.total()));
    mvlist.set_hinc(mvlist.nouse != 0);
    mvlist.nouse2 = mvlist
        .nouse
        .saturating_add(enemy_mkey.total().saturating_sub(mkey.total()));
    mvlist.set_inc(mvlist.nouse2 != 0);
}

/// Disproof pieces at an OR node with no legal checks (`disproof_koma`).
pub fn disproof_koma<P: PositionBackend>(pos: &P) -> MKey {
    let self_mkey = pos.mkey(pos.turn());
    let enemy_mkey = pos.mkey(pos.turn().flip());
    let mut mkey = self_mkey;
    if mkey.hi() != 0 || (enemy_mkey.hi() != 0 && !pos.drop_check(HI)) {
        mkey.set_hi(mkey.hi() + enemy_mkey.hi());
    }
    if mkey.ka() != 0 || (enemy_mkey.ka() != 0 && !pos.drop_check(KA)) {
        mkey.set_ka(mkey.ka() + enemy_mkey.ka());
    }
    if mkey.ki() != 0 || (enemy_mkey.ki() != 0 && !pos.drop_check(KI)) {
        mkey.set_ki(mkey.ki() + enemy_mkey.ki());
    }
    if mkey.gi() != 0 || (enemy_mkey.gi() != 0 && !pos.drop_check(GI)) {
        mkey.set_gi(mkey.gi() + enemy_mkey.gi());
    }
    if mkey.ke() != 0 || (enemy_mkey.ke() != 0 && !pos.drop_check(KE)) {
        mkey.set_ke(mkey.ke() + enemy_mkey.ke());
    }
    if mkey.ky() != 0 || (enemy_mkey.ky() != 0 && !pos.drop_check(KY)) {
        mkey.set_ky(mkey.ky() + enemy_mkey.ky());
    }
    if mkey.fu() != 0 || (enemy_mkey.fu() != 0 && !pos.drop_check(FU)) {
        mkey.set_fu(mkey.fu() + enemy_mkey.fu());
    }
    mkey
}

#[inline]
fn disproof_koma_unit(child: &MvItem) -> MKey {
    let mut mkey = child.mkey;
    let mv = child.head_move();
    if mv.is_drop() {
        mkey.inc(mv.hand_ktype());
    } else if child.cap_koma != 0 {
        let ktype = child.cap_koma % PROMOTED;
        if mkey.get(ktype) > 0 {
            mkey.dec(ktype);
        }
    }
    mkey
}

/// Propagate disproof pieces upward at an OR node (`disproof_koma_or`).
pub fn disproof_koma_or<P: PositionBackend>(pos: &P, mvlist: &mut MvItem, children: &[MvItem]) {
    let mut mkey = disproof_koma_unit(&children[0]);
    for child in &children[1..] {
        mkey = mkey.min(disproof_koma_unit(child));
    }
    let self_mkey = pos.mkey(pos.turn());
    if self_mkey.hi() == 0 {
        mkey.set_hi(0);
    }
    if self_mkey.ka() == 0 {
        mkey.set_ka(0);
    }
    if self_mkey.ki() == 0 {
        mkey.set_ki(0);
    }
    if self_mkey.gi() == 0 {
        mkey.set_gi(0);
    }
    if self_mkey.ke() == 0 {
        mkey.set_ke(0);
    }
    if self_mkey.ky() == 0 {
        mkey.set_ky(0);
    }
    if self_mkey.fu() == 0 {
        mkey.set_fu(0);
    }
    mvlist.mkey = mkey;
    mvlist.set_inc(self_mkey.compare(mkey) == MKeyCmp::Equal);
}

/// Propagate disproof pieces upward at an AND node (`disproof_koma_and`).
pub fn disproof_koma_and<P: PositionBackend>(pos: &P, mvlist: &mut MvItem, best_child: &MvItem) {
    mvlist.mkey = best_child.mkey;
    let mv = best_child.head_move();
    let dmkey = pos.mkey(pos.turn());
    let tmkey = pos.mkey(pos.turn().flip());
    let child_mkey = best_child.mkey;

    if mv.is_drop() {
        let ktype = mv.hand_ktype();
        if dmkey.get(ktype) + tmkey.get(ktype) == child_mkey.get(ktype)
            && mvlist.mkey.get(ktype) > 0
        {
            mvlist.mkey.dec(ktype);
        }
    } else if best_child.cap_koma != 0 {
        let ktype = best_child.cap_koma % PROMOTED;
        if dmkey.get(ktype) + tmkey.get(ktype) + 1 == child_mkey.get(ktype)
            && mvlist.mkey.get(ktype) > 0
        {
            mvlist.mkey.dec(ktype);
        }
    }

    mvlist.set_inc(tmkey.compare(mvlist.mkey) == MKeyCmp::Equal);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shtsume::types::{Move, TData, GOU, INFINATE, SOU};
    use rand::{rngs::StdRng, Rng, SeedableRng};

    fn ref_or_comp(a: &MvItem, b: &MvItem) -> Ordering {
        if a.tdata.pn != b.tdata.pn {
            return a.tdata.pn.cmp(&b.tdata.pn);
        }
        if a.tdata.dn != b.tdata.dn {
            if a.tdata.pn > 0 && a.tdata.pn < INFINATE {
                return b.tdata.dn.cmp(&a.tdata.dn);
            }
            return a.tdata.dn.cmp(&b.tdata.dn);
        }
        let a_imm = a.tdata.pn == 0 && a.tdata.sh == 0;
        let b_imm = b.tdata.pn == 0 && b.tdata.sh == 0;
        if a_imm != b_imm {
            return if a_imm {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        if a.inc() != b.inc() {
            return b.inc().cmp(&a.inc());
        }
        if a.tdata.sh != b.tdata.sh {
            return a.tdata.sh.cmp(&b.tdata.sh);
        }
        if a.pr() != b.pr() {
            return a.pr().cmp(&b.pr());
        }
        if a.cu() != b.cu() {
            return a.cu().cmp(&b.cu());
        }
        if a.nouse2 != b.nouse2 {
            return a.nouse2.cmp(&b.nouse2);
        }
        if a.cap_koma != b.cap_koma {
            return b.cap_koma.cmp(&a.cap_koma);
        }
        let amv = a.head_move();
        let bmv = b.head_move();
        let a_move = amv.is_board_move();
        let b_move = bmv.is_board_move();
        if a_move != b_move {
            return if a_move {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        if amv.is_promote() != bmv.is_promote() {
            return bmv.is_promote().cmp(&amv.is_promote());
        }
        if amv.is_drop() && bmv.is_drop() && amv.hand_ktype() != bmv.hand_ktype() {
            return bmv.hand_ktype().cmp(&amv.hand_ktype());
        }
        a.length.cmp(&b.length)
    }

    fn ref_and_comp(a: &MvItem, b: &MvItem) -> Ordering {
        if a.tdata.dn != b.tdata.dn {
            return a.tdata.dn.cmp(&b.tdata.dn);
        }
        if a.tdata.pn != b.tdata.pn {
            return a.tdata.pn.cmp(&b.tdata.pn);
        }
        if a.inc() != b.inc() {
            return a.inc().cmp(&b.inc());
        }
        if a.nouse2 != b.nouse2 {
            return a.nouse2.cmp(&b.nouse2);
        }
        if a.tdata.pn == 0 && b.tdata.pn == 0 {
            if a.tdata.sh != b.tdata.sh {
                return b.tdata.sh.cmp(&a.tdata.sh);
            }
        } else if a.tdata.sh != b.tdata.sh {
            return a.tdata.sh.cmp(&b.tdata.sh);
        }
        let amv = a.head_move();
        let bmv = b.head_move();
        let a_move = amv.is_board_move();
        let b_move = bmv.is_board_move();
        if a_move != b_move {
            return if a_move {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        if a.cap_koma != b.cap_koma {
            return b.cap_koma.cmp(&a.cap_koma);
        }
        if a_move && b_move {
            if a.src_koma != b.src_koma {
                return a.src_koma.cmp(&b.src_koma);
            }
            let a_ou = a.src_koma == SOU || a.src_koma == GOU;
            let b_ou = b.src_koma == SOU || b.src_koma == GOU;
            if a_ou != b_ou {
                return if a_ou {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
        }
        if amv.is_promote() != bmv.is_promote() {
            return bmv.is_promote().cmp(&amv.is_promote());
        }
        if amv.is_drop() && bmv.is_drop() {
            return amv.hand_ktype().cmp(&bmv.hand_ktype());
        }
        Ordering::Equal
    }

    fn rand_item(rng: &mut StdRng) -> MvItem {
        let mv = match rng.gen_range(0..3) {
            0 => Move::normal(rng.gen_range(0..81), rng.gen_range(0..81)),
            1 => Move::promote(rng.gen_range(0..81), rng.gen_range(0..81)),
            _ => Move::drop(rng.gen_range(1..=7), rng.gen_range(0..81)),
        };
        let src_koma = if mv.is_board_move() {
            rng.gen_range(1..32)
        } else {
            0
        };
        let cap_koma = if mv.is_board_move() && rng.gen_bool(0.5) {
            rng.gen_range(1..32)
        } else {
            0
        };
        let mut item = MvItem::single_with_hints(mv, src_koma, cap_koma);
        let pns = [0u16, 1, 2, 5, 100, INFINATE];
        let shs = [0u16, 1, 2, 30, 545, 2000];
        item.tdata = TData {
            pn: pns[rng.gen_range(0..pns.len())],
            dn: pns[rng.gen_range(0..pns.len())],
            sh: shs[rng.gen_range(0..shs.len())],
        };
        item.set_inc(rng.gen_bool(0.5));
        item.set_pr(rng.gen_bool(0.5));
        item.set_cu(rng.gen_bool(0.5));
        item.nouse2 = rng.gen_range(0..=38);
        item.length = rng.gen_range(0..=7);
        item
    }

    #[test]
    fn test_branchless_sort_keys_match_reference() {
        let mut rng = StdRng::seed_from_u64(42);
        let items: Vec<MvItem> = (0..250).map(|_| rand_item(&mut rng)).collect();
        for a in &items {
            for b in &items {
                assert_eq!(proof_number_comp(a, b), ref_or_comp(a, b));
                assert_eq!(disproof_number_comp(a, b), ref_and_comp(a, b));
            }
        }
    }
}
