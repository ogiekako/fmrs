use crate::position::advance::options::AdvanceResult;
use crate::position::bitboard::{king_power, lion_king_power, power, reachable, reachable_sub};
use crate::position::position::PositionAux;

use crate::piece::{Color, Kind};

use crate::position::{
    bitboard::{self, BitBoard},
    Movement,
};

use super::attack_prevent::{attack_preventing_movements, attacker, Attacker};
use super::pinned::{pinned, Pinned};
use super::{common, AdvanceOptions};

pub(super) fn advance(
    position: &mut PositionAux,
    options: &AdvanceOptions,
    res: &mut Vec<Movement>,
) -> AdvanceResult<()> {
    debug_assert_eq!(position.turn(), Color::BLACK);
    let mut ctx = Context::new(position, options, res)?;
    ctx.advance()?;
    Ok(())
}

struct Context<'a> {
    // Immutable fields
    position: &'a mut PositionAux,
    attacker: Option<Attacker>,
    /// Lazy: drops() では未使用、direct_attack/discovered_attack でのみ参照。
    /// max_allowed_branches=0 で drops() が早期 Err 返した場合 pinned 不要。
    pinned: Option<Pinned>,
    pawn_mask: usize,
    options: &'a AdvanceOptions,

    // Mutable fields
    result: &'a mut Vec<Movement>,
    num_branches_without_pawn_drop: usize,
}

impl<'a> Context<'a> {
    fn new(
        position: &'a mut PositionAux,
        options: &'a AdvanceOptions,
        result: &'a mut Vec<Movement>,
    ) -> AdvanceResult<Self> {
        let attacker = position
            .black_king_pos()
            .is_some()
            .then(|| attacker(position, Color::BLACK, false))
            .flatten();

        let pawn_mask = {
            let mut mask = Default::default();
            for pos in position.bitboard(Color::BLACK, Kind::Pawn) {
                mask |= 1 << pos.col()
            }
            mask
        };

        Ok(Self {
            position,
            attacker,
            pinned: None,
            pawn_mask,
            result,
            num_branches_without_pawn_drop: 0,
            options,
        })
    }

    fn pinned(&mut self) -> &Pinned {
        if self.pinned.is_none() {
            self.pinned = Some(if self.position.black_king_pos().is_some() {
                pinned(self.position, Color::BLACK, Color::BLACK)
            } else {
                Pinned::default()
            });
        }
        self.pinned.as_ref().unwrap()
    }

    fn advance(&mut self) -> AdvanceResult<()> {
        if let Some(attacker) = &self.attacker {
            attack_preventing_movements(
                self.position,
                true,
                self.options,
                attacker.clone().into(),
                self.result,
            )?;
            return Ok(());
        }

        self.drops()?;
        self.direct_attack_moves()?;
        self.discovered_attack_moves()?;

        Ok(())
    }

    #[inline(never)]
    fn drops(&mut self) -> AdvanceResult<()> {
        let white_king_pos = self.position.white_king_pos();
        for kind in self.position.hands().kinds(Color::BLACK) {
            if kind == Kind::Pawn && self.pawn_mask >> white_king_pos.col() & 1 != 0 {
                continue;
            }

            let empty_attack_squares =
                reachable_sub(self.position, Color::WHITE, white_king_pos, kind)
                    .and_not(self.position.occupied_bb());

            for pos in empty_attack_squares {
                self.maybe_add_move(Movement::Drop(pos, kind), kind)?;
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn direct_attack_moves(&mut self) -> AdvanceResult<()> {
        self.non_leap_piece_direct_attack()?;
        self.leap_piece_direct_attack()?;

        Ok(())
    }

    #[inline(never)]
    fn non_leap_piece_direct_attack(&mut self) -> AdvanceResult<()> {
        let lion_king_range = lion_king_power(self.position.white_king_pos());
        let king_range = king_power(self.position.white_king_pos())
            .and_not(self.position.color_bb_and_stone(Color::BLACK));

        let attacker_cands =
            self.position.pawn_silver_goldish() & lion_king_range & self.position.black_bb();

        for attacker_pos in attacker_cands {
            // pawn_silver_goldish includes various non-line kinds; lookup needed.
            let attacker_source_kind = self.position.must_get_kind(attacker_pos);

            let attacker_range = self.pinned().pinned_area(attacker_pos).unwrap_or_else(|| {
                bitboard::power(Color::BLACK, attacker_pos, attacker_source_kind)
            }) & king_range;
            if attacker_range.is_empty() {
                continue;
            }

            for promote in [false, true] {
                if promote && attacker_source_kind.promote().is_none() {
                    continue;
                }

                let attacker_dest_kind = if promote {
                    attacker_source_kind.promote().unwrap()
                } else {
                    attacker_source_kind
                };

                let mut attack_squares = power(
                    Color::WHITE,
                    self.position.white_king_pos(),
                    attacker_dest_kind,
                );
                if promote && !BitBoard::BLACK_PROMOTABLE.contains(attacker_pos) {
                    attack_squares &= BitBoard::BLACK_PROMOTABLE;
                }

                for dest in attacker_range & attack_squares {
                    let capture_kind = self.position.get_kind(dest);
                    self.maybe_add_move(
                        Movement::move_with_hint(
                            attacker_pos,
                            attacker_source_kind,
                            dest,
                            promote,
                            capture_kind,
                        ),
                        attacker_source_kind,
                    )?;
                }
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn leap_piece_direct_attack(&mut self) -> AdvanceResult<()> {
        let white_king_pos = self.position.white_king_pos();

        for attacker_source_kind in [
            Kind::Lance,
            Kind::Knight,
            Kind::Bishop,
            Kind::Rook,
            Kind::ProBishop,
            Kind::ProRook,
        ] {
            let attackers = self.position.bitboard(Color::BLACK, attacker_source_kind);
            if attackers.is_empty() {
                continue;
            }
            let promoted_kind = attacker_source_kind.promote();
            let no_promotion_dest_cands = reachable(
                self.position,
                Color::WHITE,
                white_king_pos,
                attacker_source_kind,
                true,
            );
            let promotion_dest_cands = promoted_kind
                .map(|k| reachable(self.position, Color::WHITE, white_king_pos, k, true));

            // attackers は bitboard(BLACK, attacker_source_kind) なのでこのループ内では
            // kind は必ず attacker_source_kind。must_get_kind の packed lookup は不要。
            let source_kind = attacker_source_kind;
            let can_promote = source_kind.can_promote();
            for attacker_pos in attackers {
                let attacker_reachable =
                    self.pinned().pinned_area(attacker_pos).unwrap_or_else(|| {
                        bitboard::reachable_sub(
                            self.position,
                            Color::BLACK,
                            attacker_pos,
                            attacker_source_kind,
                        )
                    });

                for dest in attacker_reachable & no_promotion_dest_cands {
                    let capture_kind = self.position.get_kind(dest);
                    self.maybe_add_move(
                        Movement::move_with_hint(
                            attacker_pos,
                            source_kind,
                            dest,
                            false,
                            capture_kind,
                        ),
                        source_kind,
                    )?;
                }
                if let Some(mut dest_cands) = promotion_dest_cands {
                    if !can_promote {
                        continue;
                    }
                    if !BitBoard::BLACK_PROMOTABLE.contains(attacker_pos) {
                        dest_cands &= BitBoard::BLACK_PROMOTABLE;
                    }
                    for dest in attacker_reachable & dest_cands {
                        let capture_kind = self.position.get_kind(dest);
                        self.maybe_add_move(
                            Movement::move_with_hint(
                                attacker_pos,
                                source_kind,
                                dest,
                                true,
                                capture_kind,
                            ),
                            source_kind,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    // #[inline(never)]
    fn discovered_attack_moves(&mut self) -> AdvanceResult<()> {
        let blockers = pinned(self.position, Color::WHITE, Color::BLACK);
        if blockers.iter().next().is_none() {
            return Ok(());
        }

        // 直接攻撃手と一致する dest を BitBoard で先に除外して
        // (blocker, dest, promote) 単位の集合的 dedup を行う。
        // 必要十分な理由:
        //  - 直接攻撃手 = blocker_kind が dest から白王を取れる手
        //  - power(WHITE, white_king_pos, kind) (非 line) /
        //    reachable_sub(WHITE, white_king_pos, kind) (line) が
        //    その dest 集合を表す
        //  - 開き王手 = pin 線から外れる手なので、両者の交差が
        //    direct_attack_moves で既に追加された手と一致する
        let white_king_pos = self.position.white_king_pos();
        for &(blocker_pos, blocker_pinned_area) in blockers.iter() {
            let blocker_kind = self.position.must_get_kind(blocker_pos);

            let mut blocker_dest_cands = bitboard::reachable(
                self.position,
                Color::BLACK,
                blocker_pos,
                blocker_kind,
                false,
            )
            .and_not(blocker_pinned_area);

            if let Some(area) = self.pinned().pinned_area(blocker_pos) {
                blocker_dest_cands &= area;
            }

            // 直接攻撃手で生成される dest を除外。
            let direct_unpromoted = bitboard::reachable_sub(
                self.position,
                Color::WHITE,
                white_king_pos,
                blocker_kind,
            );
            let pure_unpromoted = blocker_dest_cands.and_not(direct_unpromoted);

            let promoted_kind = blocker_kind.promote();
            let direct_promoted = promoted_kind
                .map(|k| bitboard::reachable_sub(self.position, Color::WHITE, white_king_pos, k));

            // 不成り合法 mask: Pawn/Lance は最終段、Knight は最終段+次の段に
            // 不成りで動けない。それ以外の駒種は全マス合法。
            let unpromoted_legal = match blocker_kind {
                Kind::Pawn | Kind::Lance => BitBoard::FULL.and_not(BitBoard::ROW1),
                Kind::Knight => BitBoard::FULL.and_not(BitBoard::ROW1 | BitBoard::ROW2),
                _ => BitBoard::FULL,
            };
            let pure_unpromoted = pure_unpromoted & unpromoted_legal;

            for blocker_dest in pure_unpromoted {
                let capture_kind = self.position.get_kind(blocker_dest);
                self.maybe_add_move(
                    Movement::move_with_hint(
                        blocker_pos,
                        blocker_kind,
                        blocker_dest,
                        false,
                        capture_kind,
                    ),
                    blocker_kind,
                )?;
            }

            if let Some(direct_promoted) = direct_promoted {
                // 成り合法 mask: source か dest のどちらかが敵陣にあれば成れる。
                // source が敵陣なら全 dest 合法、そうでなければ dest が敵陣のもののみ。
                let promoted_legal = if BitBoard::BLACK_PROMOTABLE.contains(blocker_pos) {
                    BitBoard::FULL
                } else {
                    BitBoard::BLACK_PROMOTABLE
                };
                let pure_promoted = blocker_dest_cands.and_not(direct_promoted) & promoted_legal;
                for blocker_dest in pure_promoted {
                    let capture_kind = self.position.get_kind(blocker_dest);
                    self.maybe_add_move(
                        Movement::move_with_hint(
                            blocker_pos,
                            blocker_kind,
                            blocker_dest,
                            true,
                            capture_kind,
                        ),
                        blocker_kind,
                    )?;
                }
            }
        }
        Ok(())
    }
}

// Helper
impl Context<'_> {
    fn maybe_add_move(&mut self, movement: Movement, kind: Kind) -> AdvanceResult<()> {
        if kind == Kind::King {
            let mut np = self.position.clone();
            np.do_move(&movement);
            if common::checked(&mut np, Color::BLACK) {
                return Ok(());
            }
        }

        debug_assert!(
            {
                let mut np = self.position.clone();
                np.do_move(&movement);
                !common::checked(&mut np, Color::BLACK)
            },
            "Black king checked: {:?}",
            {
                let mut np = self.position.clone();
                np.do_move(&movement);
                np
            }
        );

        if !movement.is_pawn_drop() {
            self.num_branches_without_pawn_drop += 1;
            self.options
                .check_allowed_branches(self.num_branches_without_pawn_drop)?;
        }

        // Each phase (drops / non_leap_direct / leap_direct / discovered) emits
        // unique (source, dest, promote) tuples internally, and the bitboard
        // filter inside `discovered_attack_moves` removes overlap with direct
        // moves. No `seen`-based dedup is needed.
        self.result.push(movement);

        Ok(())
    }
}
