use crate::piece::{Color, Kind};
use crate::position::position::PositionAux;
use crate::position::zobrist::zobrist;
use crate::position::Hands;

/// Pre-sum a batch of white hand-count changes (+Gold, +Lance, +Knight, +Silver,
/// −Pawn) into a single delta for the white count word. The same per-kind
/// `bit_of` shifts that `add_n`/`remove_n` would apply are summed once and added
/// to `h[1]` in one go.
#[inline]
fn white_word_delta(
    n_gold: u64,
    n_lance: u64,
    n_knight: u64,
    n_silver: u64,
    n_pawn_sub: u64,
) -> u64 {
    let adds = [
        (Kind::Gold, n_gold),
        (Kind::Lance, n_lance),
        (Kind::Knight, n_knight),
        (Kind::Silver, n_silver),
    ];
    let mut word = 0u64;
    for (kind, n) in adds {
        word = word.wrapping_add(Hands::bit_of(kind).wrapping_mul(n));
    }
    word.wrapping_sub(Hands::bit_of(Kind::Pawn).wrapping_mul(n_pawn_sub))
}

/// Smoke 用の正規化: 黒の goldish (Gold/ProPawn/ProLance/ProKnight/ProSilver) のうち
/// ProPawn 以外を順に「白持駒に戻して、白持駒から優先順 [Pawn, Lance, Knight, Silver, Gold]
/// で取り直し、ProPawn/ProLance/ProKnight/ProSilver/Gold として置き直す」操作。
///
/// 想定: 白手駒には常に Pawn を優先して使い、ProPawn 化されることが多い。
/// 駒種情報は (盤面 → 白持駒) に対称に移るため、総駒数が固定 (Gold=Silver=Knight=Lance=4,
/// Pawn=18) の制約下で、同じ goldish 占有マス集合 + 同じ非 goldish 盤面 + 同じ黒手駒を
/// 持つ局面はすべて同一 canonical に潰れる。
///
/// 注意: 黒手駒や白手駒の Bishop/Rook、turn、stone、pawn_drop flag には触れない。
///
/// 実装方針:
/// - 空マスを 0..81 全走査せず、goldish (≠ ProPawn) の bitboard を直接反復する。
/// - Fast path (`K ≤ 白手駒 Pawn`) では全マスが ProPawn になり、square 順は結果に
///   無関係。kind 別 bb を per-kind 反復し、discriminator (4-way contains) を省略。
///   さらに白手駒の更新を `add_n` / `remove_n` でバッチ化する。
/// - Slow path (Phase B; Pawn 在庫切れで fallback が必要な稀ケース) は square 順を
///   保つために combined bb を反復する従来パスにフォールバック。
pub fn canonicalize_attacker_goldish(position: &mut PositionAux) {
    let bb_gold = position.bitboard(Color::BLACK, Kind::Gold);
    let bb_prolance = position.bitboard(Color::BLACK, Kind::ProLance);
    let bb_proknight = position.bitboard(Color::BLACK, Kind::ProKnight);
    let bb_prosilver = position.bitboard(Color::BLACK, Kind::ProSilver);
    let combined = bb_gold | bb_prolance | bb_proknight | bb_prosilver;
    if combined.is_empty() {
        return;
    }

    let k = combined.count_ones() as usize;
    let pawn_avail = position.hands().count(Color::WHITE, Kind::Pawn);

    if k <= pawn_avail {
        // Fast path: 全 K 個が Phase A。kind 別に独立処理。
        let n_gold = bb_gold.count_ones() as usize;
        let n_prolance = bb_prolance.count_ones() as usize;
        let n_proknight = bb_proknight.count_ones() as usize;
        let n_prosilver = bb_prosilver.count_ones() as usize;

        for sq in bb_gold {
            position.change_kind(sq, Color::BLACK, Kind::Gold, Kind::ProPawn);
        }
        for sq in bb_prolance {
            position.change_kind(sq, Color::BLACK, Kind::ProLance, Kind::ProPawn);
        }
        for sq in bb_proknight {
            position.change_kind(sq, Color::BLACK, Kind::ProKnight, Kind::ProPawn);
        }
        for sq in bb_prosilver {
            position.change_kind(sq, Color::BLACK, Kind::ProSilver, Kind::ProPawn);
        }

        // 白手駒の駒種別カウント更新を白カウントワードへの加算 1 回にバッチ。
        // 各駒種は色ワード内で固有の bit_of(kind) シフトを持ち、それを係数倍で
        // 合算してから一括加算する (ここは全て白)。
        let word_delta = white_word_delta(
            n_gold as u64,
            n_prolance as u64,
            n_proknight as u64,
            n_prosilver as u64,
            k as u64,
        );
        let w = position.hands_mut().white_word_mut();
        *w = w.wrapping_add(word_delta);
        return;
    }

    // Slow path: Pawn 不足、square 順に Phase A → Phase B 切替。
    canonicalize_slow(
        position,
        combined,
        bb_gold,
        bb_prolance,
        bb_proknight,
        pawn_avail,
        k,
    );
}

#[cold]
fn canonicalize_slow(
    position: &mut PositionAux,
    combined: crate::position::BitBoard,
    bb_gold: crate::position::BitBoard,
    bb_prolance: crate::position::BitBoard,
    bb_proknight: crate::position::BitBoard,
    pawn_avail: usize,
    k: usize,
) {
    let phase_a_count = pawn_avail; // == pawn_avail.min(k) で k > pawn_avail 前提
    debug_assert!(phase_a_count < k);
    let mut processed = 0usize;
    for sq in combined {
        let (kind, base) = if bb_gold.contains(sq) {
            (Kind::Gold, Kind::Gold)
        } else if bb_prolance.contains(sq) {
            (Kind::ProLance, Kind::Lance)
        } else if bb_proknight.contains(sq) {
            (Kind::ProKnight, Kind::Knight)
        } else {
            (Kind::ProSilver, Kind::Silver)
        };

        if processed < phase_a_count {
            position.change_kind(sq, Color::BLACK, kind, Kind::ProPawn);
            position.hands_mut().add(Color::WHITE, base);
        } else {
            position.hands_mut().add(Color::WHITE, base);
            let target_base = pick_non_pawn_priority(position);
            if target_base == base {
                position.hands_mut().remove(Color::WHITE, base);
            } else {
                position.hands_mut().remove(Color::WHITE, target_base);
                let new_kind = match target_base {
                    Kind::Lance => Kind::ProLance,
                    Kind::Knight => Kind::ProKnight,
                    Kind::Silver => Kind::ProSilver,
                    Kind::Gold => Kind::Gold,
                    _ => unreachable!(),
                };
                position.change_kind(sq, Color::BLACK, kind, new_kind);
            }
        }
        processed += 1;
    }

    if phase_a_count > 0 {
        position
            .hands_mut()
            .remove_n(Color::WHITE, Kind::Pawn, phase_a_count);
    }
}

/// Mutation を行わず、[`canonicalize_attacker_goldish`] 適用後の digest を直接計算する。
///
/// canonical 後の盤面・白手駒は (黒 goldish bb, 非黒goldish 盤面, 黒手駒, turn, stone)
/// から決定論的に決まるので、その関数として digest を「差分」で計算できる。
///
/// 式:
///   canonical_digest = orig_digest
///                    ⊕ board_zobrist_diff                         (Σ over 黒非ProPawn goldish sq)
///                    ⊕ orig_hands.x ⊕ (orig_hands.x + hand_delta) (XOR で旧 hand を消し canonical hand を入れる)
///
/// Fast path (`K ≤ 白持駒 Pawn`): 全マスが ProPawn 化、hand_delta が定数式で求まる。
/// Slow path (Phase B; 稀): 仕様の per-square fallback simulation が必要なので
/// fallback として実 mutation 版を呼び出す。
#[inline]
pub fn canonical_digest_for_smoke(position: &PositionAux) -> u64 {
    let bb_gold = position.bitboard(Color::BLACK, Kind::Gold);
    let bb_prolance = position.bitboard(Color::BLACK, Kind::ProLance);
    let bb_proknight = position.bitboard(Color::BLACK, Kind::ProKnight);
    let bb_prosilver = position.bitboard(Color::BLACK, Kind::ProSilver);

    // 非 ProPawn goldish が一つもなければ canonical は orig 自身。
    if (bb_gold | bb_prolance | bb_proknight | bb_prosilver).is_empty() {
        return position.digest();
    }

    let n_gold = bb_gold.count_ones() as u64;
    let n_prolance = bb_prolance.count_ones() as u64;
    let n_proknight = bb_proknight.count_ones() as u64;
    let n_prosilver = bb_prosilver.count_ones() as u64;
    let k = n_gold + n_prolance + n_proknight + n_prosilver;
    let pawn_avail = position.hands().count(Color::WHITE, Kind::Pawn) as u64;

    if k > pawn_avail {
        // Phase B (稀): 実装簡単化のため実 mutation で digest を取る。
        let mut clone = position.clone();
        canonicalize_attacker_goldish(&mut clone);
        return clone.digest();
    }

    // 盤面 zobrist 差分: 各 黒 goldish (≠ ProPawn) sq について kind→ProPawn の XOR。
    let mut board_diff: u64 = 0;
    for sq in bb_gold {
        board_diff ^=
            zobrist(Color::BLACK, sq, Kind::Gold) ^ zobrist(Color::BLACK, sq, Kind::ProPawn);
    }
    for sq in bb_prolance {
        board_diff ^=
            zobrist(Color::BLACK, sq, Kind::ProLance) ^ zobrist(Color::BLACK, sq, Kind::ProPawn);
    }
    for sq in bb_proknight {
        board_diff ^=
            zobrist(Color::BLACK, sq, Kind::ProKnight) ^ zobrist(Color::BLACK, sq, Kind::ProPawn);
    }
    for sq in bb_prosilver {
        board_diff ^=
            zobrist(Color::BLACK, sq, Kind::ProSilver) ^ zobrist(Color::BLACK, sq, Kind::ProPawn);
    }

    // 白手駒 delta: +n_gold Gold, +n_prolance Lance, +n_proknight Knight,
    //               +n_prosilver Silver, -K Pawn (Phase A 全マス Pawn 取り)。
    let word_delta = white_word_delta(
        n_gold as u64,
        n_prolance as u64,
        n_proknight as u64,
        n_prosilver as u64,
        k as u64,
    );

    // canon == orig with the white count word advanced by the delta.
    let orig = position.hands();
    let mut canon = orig;
    let w = canon.white_word_mut();
    *w = w.wrapping_add(word_delta);

    position.digest() ^ board_diff ^ orig.fold() ^ canon.fold()
}

#[inline]
fn pick_non_pawn_priority(position: &PositionAux) -> Kind {
    // base ∈ {Gold, Lance, Knight, Silver} を直前に return しているので、
    // 必ず最低 1 種は ≥ 1 → 末尾 Gold で必ず止まる。
    let h = position.hands();
    if h.count(Color::WHITE, Kind::Lance) > 0 {
        Kind::Lance
    } else if h.count(Color::WHITE, Kind::Knight) > 0 {
        Kind::Knight
    } else if h.count(Color::WHITE, Kind::Silver) > 0 {
        Kind::Silver
    } else {
        Kind::Gold
    }
}

#[cfg(test)]
mod tests {
    use super::canonicalize_attacker_goldish;
    use crate::position::position::PositionAux;

    fn canon_sfen(sfen: &str) -> String {
        let mut p = PositionAux::from_sfen(sfen).unwrap();
        canonicalize_attacker_goldish(&mut p);
        p.sfen()
    }

    #[test]
    fn gold_at_sq_becomes_propawn_with_pawn_swap() {
        // 黒 Gold 1 枚、白持駒に Pawn あり。Gold → ProPawn、白持駒 +Gold -Pawn。
        let before = "8k/9/9/9/9/9/9/9/G8 b 4r4b3g4s4n4l17p 1";
        let after = canon_sfen(before);
        let p = PositionAux::from_sfen(&after).unwrap();
        // sq A1 (col 0, row 8) = "G8" の G は黒の Gold → 9 段 1 筋。
        assert_eq!(
            p.get(crate::position::Square::S99),
            Some((crate::piece::Color::BLACK, crate::piece::Kind::ProPawn))
        );
        // 白手駒: Gold 4, Pawn 16 (17 - 1)。
        assert_eq!(
            p.hands()
                .count(crate::piece::Color::WHITE, crate::piece::Kind::Gold),
            4
        );
        assert_eq!(
            p.hands()
                .count(crate::piece::Color::WHITE, crate::piece::Kind::Pawn),
            16
        );
    }

    #[test]
    fn propawn_unchanged() {
        let before = "8k/9/9/9/9/9/9/9/+P8 b 4r4b4g4s4n4l17p 1";
        let after = canon_sfen(before);
        // 既に ProPawn なので no-op。
        assert_eq!(before, after);
    }

    #[test]
    fn no_pawn_falls_back_to_lance() {
        // 白持駒 Pawn 0、Lance 4。Gold → ProLance、白持駒 +Gold -Lance。
        let before = "8k/9/9/9/9/9/9/9/G8 b 4r4b3g4s4n4l 1";
        let mut p = PositionAux::from_sfen(before).unwrap();
        canonicalize_attacker_goldish(&mut p);
        assert_eq!(
            p.get(crate::position::Square::S99),
            Some((crate::piece::Color::BLACK, crate::piece::Kind::ProLance))
        );
        assert_eq!(
            p.hands()
                .count(crate::piece::Color::WHITE, crate::piece::Kind::Gold),
            4
        );
        assert_eq!(
            p.hands()
                .count(crate::piece::Color::WHITE, crate::piece::Kind::Lance),
            3
        );
    }

    #[test]
    fn collapse_same_set_different_kinds() {
        // 同 goldish 占有マス、異種別 → canonical 一致。
        let a = "8k/9/9/9/9/9/9/9/G+S+N6 b 4r4b3g3s3n4l16p 1";
        let b = "8k/9/9/9/9/9/9/9/+S+NG6 b 4r4b3g3s3n4l16p 1";
        let c = "8k/9/9/9/9/9/9/9/+N+SG6 b 4r4b3g3s3n4l16p 1";
        let ca = canon_sfen(a);
        let cb = canon_sfen(b);
        let cc = canon_sfen(c);
        assert_eq!(ca, cb, "a={a}\nca={ca}\ncb={cb}");
        assert_eq!(ca, cc);
    }

    #[test]
    fn collapse_different_multiset_same_propawn_count_after() {
        // 異種・異 multiset でも、canonical board / white hand 一致を確認。
        // A: 2 Gold + 1 ProSilver、B: 1 Gold + 2 ProSilver (どちらも 3 駒)
        let a = "8k/9/9/9/9/9/9/9/GG+S6 b 4r4b2g3s4n4l16p 1";
        let b = "8k/9/9/9/9/9/9/9/G+S+S6 b 4r4b3g2s4n4l16p 1";
        assert_eq!(canon_sfen(a), canon_sfen(b));
    }

    #[test]
    fn idempotent() {
        let s = "8k/9/9/9/9/9/9/9/GG+S6 b 4r4b2g3s4n4l16p 1";
        let once = canon_sfen(s);
        let twice = canon_sfen(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn no_goldish_is_noop() {
        let s = "8k/9/9/9/9/9/9/9/B8 b 4r3b4g4s4n4l18p 1";
        assert_eq!(canon_sfen(s), s);
    }

    #[test]
    fn white_goldish_untouched() {
        // 白の goldish には触れない (canonicalize は黒のみ)。
        let s = "g7k/9/9/9/9/9/9/9/9 b 4r4b3g4s4n4l18p 1";
        assert_eq!(canon_sfen(s), s);
    }

    #[test]
    fn black_hand_preserved() {
        // 黒手駒に Gold/Silver があっても canonicalize は触れない。
        use crate::piece::{Color, Kind};
        let mut p = PositionAux::from_sfen("8k/9/9/9/9/9/9/9/G8 b 4r4b3g4s4n4l17p 2g 1").unwrap();
        let before_black_gold = p.hands().count(Color::BLACK, Kind::Gold);
        let before_black_silver = p.hands().count(Color::BLACK, Kind::Silver);
        canonicalize_attacker_goldish(&mut p);
        assert_eq!(p.hands().count(Color::BLACK, Kind::Gold), before_black_gold);
        assert_eq!(
            p.hands().count(Color::BLACK, Kind::Silver),
            before_black_silver
        );
    }

    #[test]
    fn turn_and_pawn_drop_preserved() {
        // turn = w で、SFEN field 4 にステップ番号代わりのフィールドは pawn_drop に対応しないため
        // 通常の SFEN で turn のみ確認。
        let s = "8k/9/9/9/9/9/9/9/G8 w 4r4b3g4s4n4l17p 1";
        let mut p = PositionAux::from_sfen(s).unwrap();
        canonicalize_attacker_goldish(&mut p);
        assert_eq!(p.turn(), crate::piece::Color::WHITE);
    }

    #[test]
    fn total_piece_count_invariant() {
        // canonicalize 前後で Gold/Silver/Knight/Lance/Pawn の総数 (盤面+両手駒) が一致する。
        use crate::piece::{Color, Kind};
        let s = "9/9/9/9/9/9/6+L+L+S/5L2S/6kSL b 2r2b4gs4n18p 1";
        let mut p = PositionAux::from_sfen(s).unwrap();
        let totals_before: Vec<usize> = [
            Kind::Pawn,
            Kind::Lance,
            Kind::Knight,
            Kind::Silver,
            Kind::Gold,
        ]
        .iter()
        .map(|&k| total_kind_count(&p, k))
        .collect();
        canonicalize_attacker_goldish(&mut p);
        let totals_after: Vec<usize> = [
            Kind::Pawn,
            Kind::Lance,
            Kind::Knight,
            Kind::Silver,
            Kind::Gold,
        ]
        .iter()
        .map(|&k| total_kind_count(&p, k))
        .collect();
        assert_eq!(totals_before, totals_after);
        // 黒の goldish は ProPawn のみ (canonicalize 後)。
        for sq in crate::position::Square::iter() {
            if let Some((Color::BLACK, kind)) = p.get(sq) {
                if matches!(
                    kind,
                    Kind::Gold | Kind::ProLance | Kind::ProKnight | Kind::ProSilver
                ) {
                    panic!("黒の非 ProPawn goldish が残っている: {:?} @ {:?}", kind, sq);
                }
            }
        }
    }

    fn total_kind_count(p: &PositionAux, k: crate::piece::Kind) -> usize {
        use crate::piece::{Color, Kind};
        let on_board = |target: Kind| -> usize {
            crate::position::Square::iter()
                .filter(|&sq| p.get(sq).map(|(_, kind)| kind == target).unwrap_or(false))
                .count()
        };
        match k {
            Kind::Pawn => {
                on_board(Kind::Pawn)
                    + on_board(Kind::ProPawn)
                    + p.hands().count(Color::BLACK, Kind::Pawn)
                    + p.hands().count(Color::WHITE, Kind::Pawn)
            }
            Kind::Lance => {
                on_board(Kind::Lance)
                    + on_board(Kind::ProLance)
                    + p.hands().count(Color::BLACK, Kind::Lance)
                    + p.hands().count(Color::WHITE, Kind::Lance)
            }
            Kind::Knight => {
                on_board(Kind::Knight)
                    + on_board(Kind::ProKnight)
                    + p.hands().count(Color::BLACK, Kind::Knight)
                    + p.hands().count(Color::WHITE, Kind::Knight)
            }
            Kind::Silver => {
                on_board(Kind::Silver)
                    + on_board(Kind::ProSilver)
                    + p.hands().count(Color::BLACK, Kind::Silver)
                    + p.hands().count(Color::WHITE, Kind::Silver)
            }
            Kind::Gold => {
                on_board(Kind::Gold)
                    + p.hands().count(Color::BLACK, Kind::Gold)
                    + p.hands().count(Color::WHITE, Kind::Gold)
            }
            _ => 0,
        }
    }

    #[test]
    fn many_goldish_collapse() {
        // 複雑な多 goldish 局面の collapse 確認。
        // 4 マス全部異種別 vs 4 マス全 ProPawn — total Pawn count が異なるため
        // canonical も異なる (ProPawn 元数で Pawn 配分が変わる)。
        let mixed = "9/9/9/9/9/9/9/8k/G+S+N+L5 b 4r4b3g3s3n3l17p 1";
        let all_propawn_diff_count = "9/9/9/9/9/9/9/8k/+P+P+P+P5 b 4r4b4g4s4n4l14p 1";
        // mixed canonical: ProPawn が 4 マス、白手駒 Gold=4,Silver=4,Knight=4,Lance=4,Pawn=17-4=13。
        // all_propawn canonical (no-op): 同じく ProPawn 4 マス、白手駒 ...,Pawn=14。
        // Pawn 数が異なる → canonical 異なる。
        assert_ne!(canon_sfen(mixed), canon_sfen(all_propawn_diff_count));
    }

    #[test]
    fn canonical_digest_matches_canonicalize_then_digest() {
        let cases = [
            "8k/9/9/9/9/9/9/9/B8 b 4r3b4g4s4n4l18p 1",
            "8k/9/9/9/9/9/9/9/+P+P+P6 b 4r4b4g4s4n4l15p 1",
            "8k/9/9/9/9/9/9/9/G+S+N6 b 4r4b3g3s3n4l16p 1",
            "8k/9/9/9/9/9/G+S+N+L+SG3/+N+L+SG5/9 b 4r4b1gs2n2l13p 1",
            "8k/9/9/9/9/9/9/9/G8 b 4r4b3g4s4n4l 1", // Phase B (no pawn)
            "8k/9/9/9/9/9/9/9/GG+S6 b 4r4b2g3s4n4l16p 1",
            "8k/9/9/9/9/9/9/9/+S+NG6 b 4r4b3g3s3n4l16p 1",
            "9/9/9/9/9/9/6+L+L+S/5L2S/6kSL b 2r2b4gs4n18p 1",
        ];
        for sfen in cases {
            let p = PositionAux::from_sfen(sfen).unwrap();
            let direct = super::canonical_digest_for_smoke(&p);
            let mut clone = p.clone();
            super::canonicalize_attacker_goldish(&mut clone);
            assert_eq!(direct, clone.digest(), "case: {}", sfen);
        }
    }

    #[test]
    fn canonical_digest_collapse_matches() {
        // 等値類の局面 → 同 canonical_digest
        let a = "8k/9/9/9/9/9/9/9/G+S+N6 b 4r4b3g3s3n4l16p 1";
        let b = "8k/9/9/9/9/9/9/9/+S+NG6 b 4r4b3g3s3n4l16p 1";
        let c = "8k/9/9/9/9/9/9/9/+N+SG6 b 4r4b3g3s3n4l16p 1";
        let pa = PositionAux::from_sfen(a).unwrap();
        let pb = PositionAux::from_sfen(b).unwrap();
        let pc = PositionAux::from_sfen(c).unwrap();
        let da = super::canonical_digest_for_smoke(&pa);
        assert_eq!(da, super::canonical_digest_for_smoke(&pb));
        assert_eq!(da, super::canonical_digest_for_smoke(&pc));
    }

    #[test]
    fn many_goldish_collapse_same_pawn_count() {
        // mixed: 4 駒 (G,+S,+N,+L) → canonical: Pawn 17-4=13
        // all_gold: 4 駒 (G,G,G,G) → canonical: Pawn 17-4=13
        // 同 Pawn 数 → collapse する。
        let mixed = "9/9/9/9/9/9/9/8k/G+S+N+L5 b 4r4b3g3s3n3l17p 1";
        let all_gold = "9/9/9/9/9/9/9/8k/GGGG5 b 4r4b4s4n4l17p 1";
        assert_eq!(canon_sfen(mixed), canon_sfen(all_gold));
    }
}
