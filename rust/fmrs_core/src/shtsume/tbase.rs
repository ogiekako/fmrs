//! Transposition table with hand dominance and GC (ported from shtsume's `ntbase.c`).
//!
//! Optimizations over raw linked-list pools:
//! - Inlines the head `(tdata, dp)` directly inside `MCardNode` so >99% of dominance scans
//!   in `lookup_inner` never chase a pointer into `tpool`.
//! - Uses lazy bump allocation (`Vec::with_capacity` + free-list for GC-recycled slots)
//!   instead of pre-zeroing and linking millions of nodes on `new()` / `clear()`.

use crate::shtsume::backend::{HsOracle, PositionBackend};
use crate::shtsume::proof::sort_and;
use crate::shtsume::types::{
    KeyState, MKey, MKeyCmp, MvItem, TData, Turn, ALL_DEPTH, GC_DELETE_OFFSET, GC_DELETE_RATE,
    GC_TSUMI_RATE, INFINATE, MCARDS_PER_MBYTE,
};

pub const NONE: u32 = u32::MAX;

const MC_HINC: u8 = 1 << 0;
const MC_CURRENT: u8 = 1 << 1;
const MC_PROTECT: u8 = 1 << 2;

#[derive(Copy, Clone, Default)]
struct TListNode {
    next: u32,
    tdata: TData,
    dp: i16,
}

/// 28-byte packed card storing `mkey` + inlined head `(tdata, dp)` + optional overflow `tlist_tail`.
#[derive(Copy, Clone, Default)]
#[repr(C, packed(4))]
struct MCardNode {
    mkey: MKey,        // 8B
    next: u32,         // 4B
    tlist_tail: u32,   // 4B (overflow depths; NONE for >99% of cards)
    head_tdata: TData, // 6B
    head_dp: i16,      // 2B
    cpn: u16,          // 2B
    nouse: u8,         // 1B
    flags: u8,         // 1B
}
const _: () = assert!(std::mem::size_of::<MCardNode>() == 28);

impl MCardNode {
    #[inline(always)]
    const fn hinc(self) -> bool {
        (self.flags & MC_HINC) != 0
    }
    #[inline(always)]
    fn set_hinc(&mut self, v: bool) {
        if v {
            self.flags |= MC_HINC;
        } else {
            self.flags &= !MC_HINC;
        }
    }
    #[inline(always)]
    const fn current(self) -> bool {
        (self.flags & MC_CURRENT) != 0
    }
    #[inline(always)]
    fn set_current(&mut self, v: bool) {
        if v {
            self.flags |= MC_CURRENT;
        } else {
            self.flags &= !MC_CURRENT;
        }
    }
    #[inline(always)]
    const fn protect(self) -> bool {
        (self.flags & MC_PROTECT) != 0
    }
    #[inline(always)]
    fn set_protect(&mut self, v: bool) {
        if v {
            self.flags |= MC_PROTECT;
        } else {
            self.flags &= !MC_PROTECT;
        }
    }
}

#[derive(Copy, Clone, Default)]
struct ZFolderNode {
    zkey: u64,
    next: u32,
    mcard: u32,
}

/// Transposition table (`tbase_t`).
pub struct TBase {
    mask: u64,
    sz_elm: usize,
    pub num: usize,
    pub pr_num: usize,
    pub gc_num: u16,
    pub gc_max_level: u16,
    pub root_pn: u16,
    pub preserve_even_tsumi: bool,
    hs_depth: u8,
    table: Vec<u32>,
    used_buckets: Vec<u32>,
    protected: Vec<u32>,
    zpool: Vec<ZFolderNode>,
    mpool: Vec<MCardNode>,
    tpool: Vec<TListNode>,
    zfree: u32,
    mfree: u32,
    tfree: u32,
}

impl TBase {
    pub fn new(hash_mb: usize) -> Self {
        let sz_elm = (hash_mb.max(1)) * MCARDS_PER_MBYTE;
        let mut sz_tbl: usize = 1 << 14;
        while sz_tbl <= sz_elm && sz_tbl < (1 << 30) {
            sz_tbl <<= 1;
        }
        Self {
            mask: (sz_tbl as u64) - 1,
            sz_elm,
            num: 0,
            pr_num: 0,
            gc_num: 0,
            gc_max_level: 0,
            root_pn: 1,
            preserve_even_tsumi: false,
            hs_depth: 0,
            table: vec![NONE; sz_tbl],
            used_buckets: Vec::with_capacity(sz_elm.min(1 << 20)),
            protected: Vec::with_capacity(4096),
            zpool: Vec::with_capacity(sz_elm.min(1 << 20)),
            mpool: Vec::with_capacity(sz_elm.min(1 << 20)),
            tpool: Vec::with_capacity((sz_elm / 4).min(1 << 18)),
            zfree: NONE,
            mfree: NONE,
            tfree: NONE,
        }
    }

    pub fn clear(&mut self) {
        for b in self.used_buckets.drain(..) {
            self.table[b as usize] = NONE;
        }
        self.protected.clear();
        self.zpool.clear();
        self.mpool.clear();
        self.tpool.clear();
        self.zfree = NONE;
        self.mfree = NONE;
        self.tfree = NONE;
        self.num = 0;
        self.pr_num = 0;
        self.gc_num = 0;
        self.gc_max_level = 0;
        self.root_pn = 1;
    }

    #[inline(always)]
    fn bucket(&self, zkey: u64) -> usize {
        (zkey & self.mask) as usize
    }

    /// Issue an L1 prefetch hint for the hash bucket of `zkey`.
    #[inline(always)]
    pub fn prefetch(&self, zkey: u64) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let ptr = self.table.as_ptr().add(self.bucket(zkey)) as *const i8;
            core::arch::x86_64::_mm_prefetch(ptr, core::arch::x86_64::_MM_HINT_T0);
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = zkey;
        }
    }

    /// Second-stage prefetch: given `zkey` whose `table[bucket]` was prefetched earlier,
    /// prefetch the `ZFolderNode` in `zpool` to hide the second pointer-chase latency.
    #[inline(always)]
    pub fn prefetch_zfolder(&self, zkey: u64) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let zf = *self.table.get_unchecked(self.bucket(zkey));
            if zf != NONE {
                let ptr = self.zpool.as_ptr().add(zf as usize) as *const i8;
                core::arch::x86_64::_mm_prefetch(ptr, core::arch::x86_64::_MM_HINT_T0);
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = zkey;
        }
    }

    /// Third-stage prefetch: prefetch the head `MCardNode` in `mpool` once `zpool[zf]` is warm.
    #[inline(always)]
    pub fn prefetch_mcard(&self, zkey: u64) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let zf = *self.table.get_unchecked(self.bucket(zkey));
            if zf != NONE {
                let zn = self.zpool.get_unchecked(zf as usize);
                if zn.zkey == zkey && zn.mcard != NONE {
                    let ptr = self.mpool.as_ptr().add(zn.mcard as usize) as *const i8;
                    core::arch::x86_64::_mm_prefetch(ptr, core::arch::x86_64::_MM_HINT_T0);
                }
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = zkey;
        }
    }

    #[inline(always)]
    fn insert_bucket_head(&mut self, bucket: usize, head_zf: u32, new_zf: u32) {
        if head_zf == NONE {
            self.used_buckets.push(bucket as u32);
        }
        self.table[bucket] = new_zf;
    }

    #[inline]
    fn maybe_gc(&mut self) {
        if self.num >= self.sz_elm {
            self.gc();
        }
    }

    #[inline]
    fn tlist_alloc(&mut self, next: u32, tdata: TData, dp: i16) -> u32 {
        self.num += 1;
        if self.tfree != NONE {
            let idx = self.tfree;
            self.tfree = self.tpool[idx as usize].next;
            self.tpool[idx as usize] = TListNode { next, tdata, dp };
            idx
        } else {
            let idx = self.tpool.len() as u32;
            self.tpool.push(TListNode { next, tdata, dp });
            idx
        }
    }

    #[inline]
    fn tlist_free_chain(&mut self, mut idx: u32) {
        while idx != NONE {
            let next = self.tpool[idx as usize].next;
            self.tpool[idx as usize].next = self.tfree;
            self.tfree = idx;
            self.num = self.num.saturating_sub(1);
            idx = next;
        }
    }

    #[inline]
    fn mcard_alloc(&mut self, node: MCardNode) -> u32 {
        self.num += 1;
        if self.mfree != NONE {
            let idx = self.mfree;
            self.mfree = self.mpool[idx as usize].next;
            self.mpool[idx as usize] = node;
            idx
        } else {
            let idx = self.mpool.len() as u32;
            self.mpool.push(node);
            idx
        }
    }

    #[inline]
    fn mcard_free(&mut self, mc: u32) {
        let tail = self.mpool[mc as usize].tlist_tail;
        if tail != NONE {
            self.tlist_free_chain(tail);
            self.mpool[mc as usize].tlist_tail = NONE;
        }
        self.mpool[mc as usize].flags = 0;
        self.mpool[mc as usize].next = self.mfree;
        self.mfree = mc;
        self.num = self.num.saturating_sub(1);
    }

    #[inline]
    fn zfolder_alloc(&mut self, zkey: u64, next: u32, mcard: u32) -> u32 {
        if self.zfree != NONE {
            let idx = self.zfree;
            self.zfree = self.zpool[idx as usize].next;
            self.zpool[idx as usize] = ZFolderNode { zkey, next, mcard };
            idx
        } else {
            let idx = self.zpool.len() as u32;
            self.zpool.push(ZFolderNode { zkey, next, mcard });
            idx
        }
    }

    #[inline(always)]
    fn find_zfolder(&self, zkey: u64) -> u32 {
        let mut zf = self.table[self.bucket(zkey)];
        while zf != NONE {
            let node = &self.zpool[zf as usize];
            if node.zkey == zkey {
                return zf;
            }
            zf = node.next;
        }
        NONE
    }

    #[inline(always)]
    pub fn clear_current(&mut self, mcard_idx: u32) {
        if mcard_idx != NONE {
            self.mpool[mcard_idx as usize].set_current(false);
        }
    }

    #[inline]
    pub fn clear_protect(&mut self) {
        for mc in self.protected.drain(..) {
            self.mpool[mc as usize].set_protect(false);
        }
        self.pr_num = 0;
    }

    #[inline]
    fn should_delete_entry(
        tdata: TData,
        dp: i16,
        gc_level: u16,
        tsumi_delete_flag: bool,
        preserve_even_tsumi: bool,
        n_tsumi: &mut usize,
        del_tsumi: &mut usize,
    ) -> bool {
        if tdata.pn == 0 {
            *n_tsumi += 1;
            let del = if tsumi_delete_flag {
                tdata.sh.is_multiple_of(2) || tdata.sh < 2
            } else if preserve_even_tsumi {
                false
            } else {
                tdata.sh.is_multiple_of(2)
            };
            if del {
                *del_tsumi += 1;
            }
            del
        } else if tdata.dn == 0 {
            if gc_level < GC_DELETE_OFFSET {
                tdata.sh.is_multiple_of(2)
            } else {
                tdata.sh.is_multiple_of(2) || tdata.sh < gc_level - 2
            }
        } else {
            let dp_even = (dp & 1) == 0;
            (tdata.pn < gc_level && dp_even) || (tdata.dn < gc_level && dp_even) || !dp_even
        }
    }

    pub fn gc(&mut self) {
        let mut gc_level: u16 = 2u16.max(self.gc_max_level);
        let mut tsumi_delete_flag = false;
        let mut delete_num: usize = 0;
        let gc_target = self.sz_elm * GC_DELETE_RATE / 100;
        let n_tsumi_max = self.sz_elm * GC_TSUMI_RATE / 100;

        loop {
            if gc_level > self.gc_max_level {
                self.gc_max_level = gc_level;
            }
            let mut n_tsumi: usize = 0;
            let mut del_tsumi: usize = 0;

            let mut write_idx: usize = 0;
            for u in 0..self.used_buckets.len() {
                let i = self.used_buckets[u] as usize;
                let mut zf = self.table[i];
                let mut new_zf = NONE;
                while zf != NONE {
                    let tmp = zf;
                    zf = self.zpool[tmp as usize].next;
                    self.zpool[tmp as usize].next = NONE;

                    let mut mc = self.zpool[tmp as usize].mcard;
                    let mut new_mc = NONE;
                    while mc != NONE {
                        let cur_mc = mc;
                        mc = self.mpool[cur_mc as usize].next;
                        self.mpool[cur_mc as usize].next = NONE;

                        let card = self.mpool[cur_mc as usize];
                        if !card.protect() && !card.current() {
                            // Filter overflow tail first
                            let mut tl = card.tlist_tail;
                            let mut new_tl = NONE;
                            while tl != NONE {
                                let cur_tl = tl;
                                tl = self.tpool[cur_tl as usize].next;
                                let node = self.tpool[cur_tl as usize];
                                if Self::should_delete_entry(
                                    node.tdata,
                                    node.dp,
                                    gc_level,
                                    tsumi_delete_flag,
                                    self.preserve_even_tsumi,
                                    &mut n_tsumi,
                                    &mut del_tsumi,
                                ) {
                                    self.tpool[cur_tl as usize].next = self.tfree;
                                    self.tfree = cur_tl;
                                    self.num = self.num.saturating_sub(1);
                                    delete_num += 1;
                                } else {
                                    self.tpool[cur_tl as usize].next = new_tl;
                                    new_tl = cur_tl;
                                }
                            }
                            self.mpool[cur_mc as usize].tlist_tail = new_tl;

                            // Check inlined head
                            let del_head = Self::should_delete_entry(
                                card.head_tdata,
                                card.head_dp,
                                gc_level,
                                tsumi_delete_flag,
                                self.preserve_even_tsumi,
                                &mut n_tsumi,
                                &mut del_tsumi,
                            );
                            if del_head {
                                delete_num += 1;
                                if new_tl != NONE {
                                    // Promote first surviving tail node into head
                                    let promoted = self.tpool[new_tl as usize];
                                    self.mpool[cur_mc as usize].head_tdata = promoted.tdata;
                                    self.mpool[cur_mc as usize].head_dp = promoted.dp;
                                    self.mpool[cur_mc as usize].tlist_tail = promoted.next;
                                    self.tpool[new_tl as usize].next = self.tfree;
                                    self.tfree = new_tl;
                                    self.num = self.num.saturating_sub(1);
                                    self.mpool[cur_mc as usize].next = new_mc;
                                    new_mc = cur_mc;
                                } else {
                                    self.mpool[cur_mc as usize].flags = 0;
                                    self.mpool[cur_mc as usize].next = self.mfree;
                                    self.mfree = cur_mc;
                                    self.num = self.num.saturating_sub(1);
                                }
                            } else {
                                self.mpool[cur_mc as usize].next = new_mc;
                                new_mc = cur_mc;
                            }
                        } else {
                            self.mpool[cur_mc as usize].next = new_mc;
                            new_mc = cur_mc;
                        }
                    }
                    self.zpool[tmp as usize].mcard = new_mc;
                    if new_mc == NONE {
                        self.zpool[tmp as usize].next = self.zfree;
                        self.zfree = tmp;
                    } else {
                        self.zpool[tmp as usize].next = new_zf;
                        new_zf = tmp;
                    }
                }
                self.table[i] = new_zf;
                if new_zf != NONE {
                    self.used_buckets[write_idx] = i as u32;
                    write_idx += 1;
                }
            }
            self.used_buckets.truncate(write_idx);

            if delete_num > gc_target || gc_level >= INFINATE - 1 {
                break;
            }
            if (n_tsumi - del_tsumi > n_tsumi_max) && (gc_level >= self.root_pn / 2) {
                tsumi_delete_flag = true;
            } else {
                gc_level = gc_level.saturating_add(1);
            }
        }

        self.gc_num = self.gc_num.saturating_add(1);
    }

    #[inline(always)]
    pub fn lookup(&mut self, ks: &KeyState, mv: &mut MvItem, tn: Turn) {
        self.lookup_inner(ks, mv, tn, false);
    }

    #[inline(always)]
    pub fn make_tree_lookup(&mut self, ks: &KeyState, mv: &mut MvItem, tn: Turn) {
        self.lookup_inner(ks, mv, tn, true);
    }

    fn lookup_inner(&mut self, ks: &KeyState, mv: &mut MvItem, tn: Turn, flag: bool) {
        let zf = self.find_zfolder(ks.zkey);
        if zf == NONE {
            mv.tdata = TData::INIT;
            return;
        }
        let mkey = ks.mkey_for(tn);
        let mut super_tsumi = NONE;
        let mut infer_fudumi = NONE;
        let mut equal_tsumi = NONE;
        let mut equal_fudumi = NONE;
        let mut equal_unknown = NONE;
        let mut tmp_pn: u16 = 1;
        let mut tmp_dn: u16 = 1;
        let mut dom_cur_cpn: u16 = 0;

        let mut mc = self.zpool[zf as usize].mcard;
        while mc != NONE {
            let mcard = self.mpool[mc as usize];
            let head_td = mcard.head_tdata;
            if mkey.0 == mcard.mkey.0 {
                if head_td.pn == 0 {
                    if equal_tsumi == NONE {
                        equal_tsumi = mc;
                    }
                } else if head_td.dn == 0 {
                    if equal_fudumi == NONE {
                        equal_fudumi = mc;
                    }
                    break;
                } else if equal_unknown == NONE {
                    equal_unknown = mc;
                }
            } else if mkey.ge_all(mcard.mkey) {
                if head_td.pn == 0 {
                    super_tsumi = mc;
                    break;
                } else if head_td.dn != 0 {
                    if tmp_pn > head_td.pn {
                        tmp_pn = head_td.pn;
                    }
                    let mut tl = mcard.tlist_tail;
                    while tl != NONE {
                        let node = self.tpool[tl as usize];
                        if tmp_pn > node.tdata.pn {
                            tmp_pn = node.tdata.pn;
                        }
                        tl = node.next;
                    }
                }
            } else if mcard.mkey.ge_all(mkey) {
                if head_td.pn != 0 && head_td.dn == 0 {
                    infer_fudumi = mc;
                    break;
                } else if head_td.pn != 0 {
                    if head_td.pn > tmp_pn {
                        tmp_pn = head_td.pn;
                    }
                    let mut tl = mcard.tlist_tail;
                    while tl != NONE {
                        let node = self.tpool[tl as usize];
                        if node.tdata.pn > tmp_pn {
                            tmp_pn = node.tdata.pn;
                        }
                        tl = node.next;
                    }
                    if mcard.current() && mcard.cpn > tmp_pn {
                        tmp_pn = mcard.cpn;
                    }
                    if mcard.current() && mcard.cpn > dom_cur_cpn {
                        dom_cur_cpn = mcard.cpn.max(1);
                    }
                }
            }
            mc = mcard.next;
        }

        if super_tsumi != NONE {
            let mcard = self.mpool[super_tsumi as usize];
            mv.tdata = mcard.head_tdata;
            mv.mkey = mcard.mkey;
            mv.set_search(true);
            mv.set_hinc(mcard.hinc());
            mv.set_inc(true);
            mv.nouse = mcard.nouse;
            mv.nouse2 = mkey
                .total()
                .saturating_sub(mcard.mkey.total())
                .saturating_add(mv.nouse);
            if flag {
                mv.set_cu(false);
                mv.set_pr(mcard.protect());
            }
            return;
        }
        if infer_fudumi != NONE {
            let mcard = self.mpool[infer_fudumi as usize];
            mv.tdata = mcard.head_tdata;
            mv.mkey = mcard.mkey;
            mv.set_search(true);
            mv.set_hinc(false);
            mv.set_inc(false);
            return;
        }
        if equal_tsumi != NONE {
            let mcard = self.mpool[equal_tsumi as usize];
            mv.tdata = mcard.head_tdata;
            mv.mkey = mcard.mkey;
            mv.set_search(true);
            mv.set_hinc(mcard.hinc());
            mv.set_inc(mcard.hinc());
            mv.nouse = mcard.nouse;
            mv.nouse2 = mcard.nouse;
            if flag {
                mv.set_cu(mcard.current());
                mv.set_pr(mcard.protect());
            }
            return;
        }
        if equal_fudumi != NONE {
            let mcard = self.mpool[equal_fudumi as usize];
            mv.tdata = mcard.head_tdata;
            mv.mkey = mcard.mkey;
            mv.set_search(true);
            mv.set_hinc(false);
            mv.set_inc(false);
            return;
        }
        if equal_unknown != NONE {
            let mcard = self.mpool[equal_unknown as usize];
            if mcard.current() {
                mv.tdata = TData {
                    pn: 1u16.max(mcard.cpn).max(dom_cur_cpn),
                    dn: 1,
                    sh: 0,
                };
                mv.set_search(true);
                return;
            }
            let target_dp = ks.count as i16;
            let mut hit = false;
            if mcard.head_dp == target_dp {
                mv.tdata = mcard.head_tdata;
                hit = true;
            }
            tmp_pn = tmp_pn.max(mcard.head_tdata.pn);
            tmp_dn = tmp_dn.max(mcard.head_tdata.dn);

            let mut tl = mcard.tlist_tail;
            while tl != NONE {
                let node = self.tpool[tl as usize];
                if node.dp == target_dp {
                    mv.tdata = node.tdata;
                    hit = true;
                }
                tmp_pn = tmp_pn.max(node.tdata.pn);
                tmp_dn = tmp_dn.max(node.tdata.dn);
                tl = node.next;
            }
            if hit {
                if dom_cur_cpn > mv.tdata.pn {
                    mv.tdata.pn = dom_cur_cpn;
                }
                mv.set_search(true);
            } else {
                // Push current head to tail and install new depth as head (using path-independent tmp_pn)
                let old_tail = mcard.tlist_tail;
                let pushed = self.tlist_alloc(old_tail, mcard.head_tdata, mcard.head_dp);
                let new_td = TData {
                    pn: tmp_pn,
                    dn: tmp_dn,
                    sh: 0,
                };
                let slot = &mut self.mpool[equal_unknown as usize];
                slot.head_tdata = new_td;
                slot.head_dp = target_dp;
                slot.tlist_tail = pushed;
                mv.tdata = TData {
                    pn: tmp_pn.max(dom_cur_cpn),
                    dn: tmp_dn,
                    sh: 0,
                };
            }
            return;
        }
        mv.tdata = TData {
            pn: tmp_pn.max(dom_cur_cpn),
            dn: tmp_dn,
            sh: 0,
        };
    }

    fn hs_lookup_raw(&self, ks: &KeyState, tn: Turn) -> i8 {
        let zf = self.find_zfolder(ks.zkey);
        if zf == NONE {
            return -1;
        }
        let mkey = ks.mkey_for(tn);
        let mut mc = self.zpool[zf as usize].mcard;
        while mc != NONE {
            let mcard = self.mpool[mc as usize];
            let head_td = mcard.head_tdata;
            if mkey.ge_all(mcard.mkey) && head_td.pn == 0 {
                return 1;
            }
            if mcard.mkey.ge_all(mkey) && head_td.dn == 0 {
                return 0;
            }
            if mkey.0 == mcard.mkey.0 && head_td.pn != 0 {
                return 0;
            }
            mc = mcard.next;
        }
        -1
    }

    /// Used by `invalid_drops` to check whether a position after capturing an interposition is already proven mate.
    pub fn hs_lookup<P: PositionBackend>(&mut self, pos: &P, tn: Turn) -> bool {
        if pos.n_oute() == 0 {
            return false;
        }
        let ks = pos.key_state();
        let res = self.hs_lookup_raw(&ks, tn);
        if res >= 0 {
            return res != 0;
        }
        if self.gc_num == 0 || self.hs_depth >= 2 {
            return false;
        }
        self.hs_depth += 1;
        let (mut list, _, _) = pos.generate_evasion(self);
        if list.is_empty() {
            self.hs_depth -= 1;
            return true;
        }
        let mut i = 0;
        while i < list.len() {
            let child_ks = pos.key_forward_item(&list[i]);
            self.make_tree_lookup(&child_ks, &mut list[i], tn);
            if let Some(tail) = list[i].take_tail() {
                let hm = tail[0];
                let src_k = if hm.is_board_move() {
                    pos.board_at(hm.prev_pos as usize)
                } else {
                    0
                };
                let cap_k = pos.board_at(hm.dest());
                list.insert(i + 1, MvItem::from_moves_with_hints(tail, src_k, cap_k));
            }
            i += 1;
        }
        self.hs_depth -= 1;
        sort_and(&mut list);
        list[0].tdata.pn == 0
    }

    #[inline(always)]
    pub fn update(&mut self, ks: &KeyState, mv: &mut MvItem, tn: Turn) {
        self.update_inner(ks, mv, tn, false, false);
    }

    #[inline(always)]
    pub fn make_tree_update(&mut self, ks: &KeyState, mv: &mut MvItem, tn: Turn) {
        self.update_inner(ks, mv, tn, true, false);
    }

    #[inline(always)]
    pub fn make_tree_simple_update(&mut self, ks: &KeyState, mv: &mut MvItem, tn: Turn) {
        self.update_inner(ks, mv, tn, true, true);
    }

    fn update_inner(
        &mut self,
        ks: &KeyState,
        mv: &mut MvItem,
        tn: Turn,
        flag: bool,
        force_sh: bool,
    ) {
        if mv.tdata.pn > INFINATE.saturating_sub(self.root_pn) && mv.tdata.dn != 0 {
            return;
        }
        self.maybe_gc();
        if mv.tdata.pn == 0 {
            self.tsumi_update(ks, mv, tn, flag, force_sh);
        } else if mv.tdata.dn == 0 {
            self.fudumi_update(ks, mv, tn);
        } else {
            self.fumei_update(ks, mv, tn);
        }
    }

    fn tsumi_update(&mut self, ks: &KeyState, mv: &MvItem, tn: Turn, flag: bool, force_sh: bool) {
        let bucket = self.bucket(ks.zkey);
        let zf = self.find_zfolder(ks.zkey);
        let mut init_flags = if mv.hinc() { MC_HINC } else { 0 };
        if flag {
            init_flags |= MC_PROTECT;
        }

        if zf == NONE {
            let new_mc = self.mcard_alloc(MCardNode {
                mkey: mv.mkey,
                next: NONE,
                tlist_tail: NONE,
                head_tdata: mv.tdata,
                head_dp: ALL_DEPTH,
                cpn: 0,
                nouse: mv.nouse,
                flags: init_flags,
            });
            if flag {
                self.pr_num += 1;
                self.protected.push(new_mc);
            }
            let head_zf = self.table[bucket];
            let new_zf = self.zfolder_alloc(ks.zkey, head_zf, new_mc);
            self.insert_bucket_head(bucket, head_zf, new_zf);
            return;
        }

        // Remove exact match of current position's mkey if present
        let cur_mkey = ks.mkey_for(tn);
        let mut mc = self.zpool[zf as usize].mcard;
        let mut prev = NONE;
        while mc != NONE {
            if self.mpool[mc as usize].mkey.0 == cur_mkey.0 {
                let next = self.mpool[mc as usize].next;
                if prev == NONE {
                    self.zpool[zf as usize].mcard = next;
                } else {
                    self.mpool[prev as usize].next = next;
                }
                if self.mpool[mc as usize].protect() {
                    self.pr_num = self.pr_num.saturating_sub(1);
                }
                self.mcard_free(mc);
                break;
            }
            prev = mc;
            mc = self.mpool[mc as usize].next;
        }

        let proof_mkey = mv.mkey;
        let mut mc = self.zpool[zf as usize].mcard;
        let mut prev = NONE;
        let mut updated = false;
        while mc != NONE {
            let next = self.mpool[mc as usize].next;
            match proof_mkey.compare(self.mpool[mc as usize].mkey) {
                MKeyCmp::Super | MKeyCmp::Other => {
                    prev = mc;
                    mc = next;
                }
                MKeyCmp::Infer => {
                    if prev == NONE {
                        self.zpool[zf as usize].mcard = next;
                    } else {
                        self.mpool[prev as usize].next = next;
                    }
                    if self.mpool[mc as usize].protect() {
                        self.pr_num = self.pr_num.saturating_sub(1);
                    }
                    self.mcard_free(mc);
                    mc = next;
                }
                MKeyCmp::Equal => {
                    let td = self.mpool[mc as usize].head_tdata;
                    if td.pn == 0 {
                        if (force_sh && !self.mpool[mc as usize].protect())
                            || td.sh > mv.tdata.sh
                            || self.mpool[mc as usize].hinc() != mv.hinc()
                        {
                            self.mpool[mc as usize].head_tdata.sh = mv.tdata.sh;
                            self.mpool[mc as usize].set_hinc(mv.hinc());
                            self.mpool[mc as usize].nouse = mv.nouse;
                        }
                        if flag && !self.mpool[mc as usize].protect() {
                            self.mpool[mc as usize].set_protect(true);
                            self.pr_num += 1;
                            self.protected.push(mc);
                        }
                        updated = true;
                    } else if td.dn != 0 {
                        let tail = self.mpool[mc as usize].tlist_tail;
                        if tail != NONE {
                            self.tlist_free_chain(tail);
                            self.mpool[mc as usize].tlist_tail = NONE;
                        }
                        self.mpool[mc as usize].head_dp = ALL_DEPTH;
                        self.mpool[mc as usize].head_tdata = mv.tdata;
                        self.mpool[mc as usize].set_hinc(mv.hinc());
                        self.mpool[mc as usize].nouse = mv.nouse;
                        if flag && !self.mpool[mc as usize].protect() {
                            self.mpool[mc as usize].set_protect(true);
                            self.pr_num += 1;
                            self.protected.push(mc);
                        }
                        updated = true;
                    }
                    prev = mc;
                    mc = next;
                }
            }
        }

        if !updated {
            let head_mc = self.zpool[zf as usize].mcard;
            let new_mc = self.mcard_alloc(MCardNode {
                mkey: mv.mkey,
                next: head_mc,
                tlist_tail: NONE,
                head_tdata: mv.tdata,
                head_dp: ALL_DEPTH,
                cpn: 0,
                nouse: mv.nouse,
                flags: init_flags,
            });
            if flag {
                self.pr_num += 1;
                self.protected.push(new_mc);
            }
            self.zpool[zf as usize].mcard = new_mc;
        }
    }

    fn fudumi_update(&mut self, ks: &KeyState, mv: &MvItem, tn: Turn) {
        let bucket = self.bucket(ks.zkey);
        let zf = self.find_zfolder(ks.zkey);
        if zf == NONE {
            let new_mc = self.mcard_alloc(MCardNode {
                mkey: mv.mkey,
                next: NONE,
                tlist_tail: NONE,
                head_tdata: mv.tdata,
                head_dp: ALL_DEPTH,
                ..Default::default()
            });
            let head_zf = self.table[bucket];
            let new_zf = self.zfolder_alloc(ks.zkey, head_zf, new_mc);
            self.insert_bucket_head(bucket, head_zf, new_zf);
            return;
        }

        let cur_mkey = ks.mkey_for(tn);
        let mut mc = self.zpool[zf as usize].mcard;
        let mut prev = NONE;
        while mc != NONE {
            if self.mpool[mc as usize].mkey.0 == cur_mkey.0 {
                let next = self.mpool[mc as usize].next;
                if prev == NONE {
                    self.zpool[zf as usize].mcard = next;
                } else {
                    self.mpool[prev as usize].next = next;
                }
                if self.mpool[mc as usize].protect() {
                    self.pr_num = self.pr_num.saturating_sub(1);
                }
                self.mcard_free(mc);
                break;
            }
            prev = mc;
            mc = self.mpool[mc as usize].next;
        }

        let dp_mkey = mv.mkey;
        let mut mc = self.zpool[zf as usize].mcard;
        let mut prev = NONE;
        let mut updated = false;
        while mc != NONE {
            let next = self.mpool[mc as usize].next;
            match dp_mkey.compare(self.mpool[mc as usize].mkey) {
                MKeyCmp::Super => {
                    if prev == NONE {
                        self.zpool[zf as usize].mcard = next;
                    } else {
                        self.mpool[prev as usize].next = next;
                    }
                    if self.mpool[mc as usize].protect() {
                        self.pr_num = self.pr_num.saturating_sub(1);
                    }
                    self.mcard_free(mc);
                    mc = next;
                }
                MKeyCmp::Infer | MKeyCmp::Other => {
                    prev = mc;
                    mc = next;
                }
                MKeyCmp::Equal => {
                    let td = self.mpool[mc as usize].head_tdata;
                    if td.dn == 0 {
                        updated = true;
                    } else if td.pn != 0 {
                        let tail = self.mpool[mc as usize].tlist_tail;
                        if tail != NONE {
                            self.tlist_free_chain(tail);
                            self.mpool[mc as usize].tlist_tail = NONE;
                        }
                        self.mpool[mc as usize].head_dp = ALL_DEPTH;
                        self.mpool[mc as usize].head_tdata = mv.tdata;
                        updated = true;
                    }
                    prev = mc;
                    mc = next;
                }
            }
        }

        if !updated {
            let head_mc = self.zpool[zf as usize].mcard;
            let new_mc = self.mcard_alloc(MCardNode {
                mkey: mv.mkey,
                next: head_mc,
                tlist_tail: NONE,
                head_tdata: mv.tdata,
                head_dp: ALL_DEPTH,
                ..Default::default()
            });
            self.zpool[zf as usize].mcard = new_mc;
        }
    }

    fn fumei_update(&mut self, ks: &KeyState, mv: &mut MvItem, tn: Turn) {
        let bucket = self.bucket(ks.zkey);
        let zf = self.find_zfolder(ks.zkey);
        let mkey = ks.mkey_for(tn);
        if zf == NONE {
            let new_mc = self.mcard_alloc(MCardNode {
                mkey,
                next: NONE,
                tlist_tail: NONE,
                head_tdata: mv.tdata,
                head_dp: ks.count as i16,
                ..Default::default()
            });
            let head_zf = self.table[bucket];
            let new_zf = self.zfolder_alloc(ks.zkey, head_zf, new_mc);
            self.insert_bucket_head(bucket, head_zf, new_zf);
            return;
        }

        let mut super_tsumi = NONE;
        let mut infer_fudumi = NONE;
        let mut equal_tsumi = NONE;
        let mut equal_fudumi = NONE;
        let mut equal_unknown = NONE;

        let mut mc = self.zpool[zf as usize].mcard;
        while mc != NONE {
            let mcard = self.mpool[mc as usize];
            let head_td = mcard.head_tdata;
            if mkey.0 == mcard.mkey.0 {
                if head_td.pn == 0 {
                    if equal_tsumi == NONE {
                        equal_tsumi = mc;
                    }
                } else if head_td.dn == 0 {
                    if equal_fudumi == NONE {
                        equal_fudumi = mc;
                    }
                } else if equal_unknown == NONE {
                    equal_unknown = mc;
                }
            } else if mkey.ge_all(mcard.mkey) {
                if head_td.pn == 0 {
                    if super_tsumi == NONE
                        || self.mpool[super_tsumi as usize].mkey.ge_all(mcard.mkey)
                    {
                        super_tsumi = mc;
                    }
                } else if head_td.dn != 0 && !mcard.current() {
                    if mv.tdata.pn > head_td.pn {
                        self.mpool[mc as usize].head_tdata.pn = mv.tdata.pn;
                    }
                    let mut tl = mcard.tlist_tail;
                    while tl != NONE {
                        if mv.tdata.pn > self.tpool[tl as usize].tdata.pn {
                            self.tpool[tl as usize].tdata.pn = mv.tdata.pn;
                        }
                        tl = self.tpool[tl as usize].next;
                    }
                }
            } else if mcard.mkey.ge_all(mkey) {
                if head_td.pn != 0 && head_td.dn == 0 {
                    if infer_fudumi == NONE
                        || mcard.mkey.ge_all(self.mpool[infer_fudumi as usize].mkey)
                    {
                        infer_fudumi = mc;
                    }
                } else if head_td.pn != 0 && !mcard.current() {
                    if mv.tdata.pn < head_td.pn {
                        self.mpool[mc as usize].head_tdata.pn = mv.tdata.pn;
                    }
                    let mut tl = mcard.tlist_tail;
                    while tl != NONE {
                        if mv.tdata.pn < self.tpool[tl as usize].tdata.pn {
                            self.tpool[tl as usize].tdata.pn = mv.tdata.pn;
                        }
                        tl = self.tpool[tl as usize].next;
                    }
                }
            }
            mc = mcard.next;
        }

        for found in [super_tsumi, infer_fudumi, equal_tsumi, equal_fudumi] {
            if found != NONE {
                let mcard = self.mpool[found as usize];
                mv.tdata = mcard.head_tdata;
                mv.mkey = mcard.mkey;
                return;
            }
        }

        if equal_unknown != NONE {
            let target_dp = ks.count as i16;
            let mcard = self.mpool[equal_unknown as usize];
            let mut hit_head = false;
            if mcard.head_dp == target_dp {
                hit_head = true;
            } else {
                self.mpool[equal_unknown as usize].head_tdata.pn = mv.tdata.pn;
            }

            let mut same_tl = NONE;
            let mut tl = mcard.tlist_tail;
            while tl != NONE {
                if self.tpool[tl as usize].dp == target_dp {
                    same_tl = tl;
                } else {
                    self.tpool[tl as usize].tdata.pn = mv.tdata.pn;
                }
                tl = self.tpool[tl as usize].next;
            }

            if hit_head {
                self.mpool[equal_unknown as usize].head_tdata = mv.tdata;
            } else if same_tl != NONE {
                self.tpool[same_tl as usize].tdata = mv.tdata;
            } else {
                let old_head_td = self.mpool[equal_unknown as usize].head_tdata;
                let old_head_dp = self.mpool[equal_unknown as usize].head_dp;
                let old_tail = self.mpool[equal_unknown as usize].tlist_tail;
                let pushed = self.tlist_alloc(old_tail, old_head_td, old_head_dp);
                let slot = &mut self.mpool[equal_unknown as usize];
                slot.head_tdata = mv.tdata;
                slot.head_dp = target_dp;
                slot.tlist_tail = pushed;
            }
        } else {
            let head_mc = self.zpool[zf as usize].mcard;
            let new_mc = self.mcard_alloc(MCardNode {
                mkey,
                next: head_mc,
                tlist_tail: NONE,
                head_tdata: mv.tdata,
                head_dp: ks.count as i16,
                ..Default::default()
            });
            self.zpool[zf as usize].mcard = new_mc;
        }
    }

    pub fn set_current(&mut self, ks: &KeyState, mv: &MvItem, tn: Turn) -> u32 {
        let bucket = self.bucket(ks.zkey);
        let zf = self.find_zfolder(ks.zkey);
        let mkey = ks.mkey_for(tn);
        if zf == NONE {
            let new_mc = self.mcard_alloc(MCardNode {
                mkey,
                next: NONE,
                tlist_tail: NONE,
                head_tdata: TData::INIT,
                head_dp: ks.count as i16,
                cpn: mv.tdata.pn,
                nouse: 0,
                flags: MC_CURRENT,
            });
            let head_zf = self.table[bucket];
            let new_zf = self.zfolder_alloc(ks.zkey, head_zf, new_mc);
            self.insert_bucket_head(bucket, head_zf, new_zf);
            return new_mc;
        }

        let mut mc = self.zpool[zf as usize].mcard;
        while mc != NONE {
            if self.mpool[mc as usize].mkey.0 == mkey.0 {
                self.mpool[mc as usize].set_current(true);
                self.mpool[mc as usize].cpn = mv.tdata.pn;
                return mc;
            }
            mc = self.mpool[mc as usize].next;
        }

        let head_mc = self.zpool[zf as usize].mcard;
        let new_mc = self.mcard_alloc(MCardNode {
            mkey,
            next: head_mc,
            tlist_tail: NONE,
            head_tdata: TData::INIT,
            head_dp: ks.count as i16,
            cpn: mv.tdata.pn,
            nouse: 0,
            flags: MC_CURRENT,
        });
        self.zpool[zf as usize].mcard = new_mc;
        new_mc
    }

    pub fn make_tree_set_item(&mut self, ks: &KeyState, tn: Turn, protect: bool) -> u32 {
        let zf = self.find_zfolder(ks.zkey);
        if zf == NONE {
            return NONE;
        }
        let mkey = ks.mkey_for(tn);
        let mut super_tsumi = NONE;
        let mut equal_tsumi = NONE;
        let mut mc = self.zpool[zf as usize].mcard;
        while mc != NONE {
            let mcard = self.mpool[mc as usize];
            if mcard.head_tdata.pn == 0 {
                if mkey.0 == mcard.mkey.0 {
                    equal_tsumi = mc;
                } else if mkey.ge_all(mcard.mkey) {
                    super_tsumi = mc;
                    break;
                }
            }
            mc = mcard.next;
        }
        let target = if super_tsumi != NONE {
            super_tsumi
        } else {
            equal_tsumi
        };
        if target != NONE {
            if protect {
                if !self.mpool[target as usize].protect() {
                    self.mpool[target as usize].set_protect(true);
                    self.pr_num += 1;
                    self.protected.push(target);
                }
            } else {
                self.mpool[target as usize].set_current(true);
            }
        }
        target
    }
}

impl HsOracle for TBase {
    #[inline(always)]
    fn hs_lookup<P: PositionBackend>(&mut self, pos: &P, tn: Turn) -> bool {
        TBase::hs_lookup(self, pos, tn)
    }
}
