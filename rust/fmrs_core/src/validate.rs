//! Shared problem-position validation for the solver front ends (wasm / server).
//!
//! Error messages are Japanese and shown to the user as-is.

use crate::{
    piece::{Color, Kind},
    position::{position::PositionAux, Square},
    sfen,
};

/// Decodes a problem SFEN and rejects positions the solver cannot handle.
///
/// The king count is checked before anything else touches the board: the solver
/// assumes white has exactly one king (see `PositionAux::white_king_pos`), and
/// even `checked_slow` panics without it.
pub fn decode_and_validate_position(problem_sfen: &str) -> Result<PositionAux, String> {
    let mut position = sfen::decode_position(problem_sfen)
        .map_err(|_| "局面の読み込みに失敗しました。".to_string())?;

    if let Some(reason) = king_count_error(&position) {
        return Err(format!("初形が不正です: {}。", reason));
    }

    let black_checked = position.checked_slow(Color::BLACK);
    let white_checked = position.checked_slow(Color::WHITE);
    if black_checked && white_checked {
        return Err("両方の玉に王手がかかっています。".to_string());
    }
    if white_checked {
        position.set_turn(Color::WHITE);
    }

    let mut reasons = vec![];
    if has_double_pawns(&position) {
        reasons.push("二歩があります");
    }
    if has_unmovable_pieces(&position) {
        reasons.push("行きどころのない駒があります");
    }
    if !reasons.is_empty() {
        return Err(format!("初形が不正です: {}。", reasons.join("、")));
    }

    Ok(position)
}

fn king_count_error(position: &PositionAux) -> Option<&'static str> {
    match position.bitboard(Color::WHITE, Kind::King).count_ones() {
        1 => {}
        0 => return Some("受方玉がありません"),
        _ => return Some("受方玉が2枚以上あります"),
    }
    if position.bitboard(Color::BLACK, Kind::King).count_ones() > 1 {
        return Some("攻方玉が2枚以上あります");
    }
    None
}

fn has_double_pawns(position: &PositionAux) -> bool {
    for color in [Color::BLACK, Color::WHITE] {
        let pawns = position.bitboard(color, Kind::Pawn).u128();
        for col in 0..9 {
            if (pawns >> (col * 9) & 0x1FF).count_ones() > 1 {
                return true;
            }
        }
    }
    false
}

fn has_unmovable_pieces(position: &PositionAux) -> bool {
    for color in [Color::BLACK, Color::WHITE] {
        for kind in [Kind::Pawn, Kind::Lance, Kind::Knight] {
            for pos in position.bitboard(color, kind) {
                if is_unmovable_square(pos, color, kind) {
                    return true;
                }
            }
        }
    }
    false
}

fn is_unmovable_square(pos: Square, color: Color, kind: Kind) -> bool {
    match (color, kind) {
        (Color::BLACK, Kind::Pawn | Kind::Lance) => pos.row() == 0,
        (Color::WHITE, Kind::Pawn | Kind::Lance) => pos.row() == 8,
        (Color::BLACK, Kind::Knight) => pos.row() <= 1,
        (Color::WHITE, Kind::Knight) => pos.row() >= 7,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::decode_and_validate_position;

    #[test]
    fn rejects_bad_king_counts() {
        for (sfen, want) in [
            (
                // 受方玉が2枚 (攻方玉の色を間違えた場合など)
                "3k1k3/9/9/9/9/9/9/9/9 b RB4G4S4N4L18P 1",
                "初形が不正です: 受方玉が2枚以上あります。",
            ),
            (
                "9/9/9/9/9/9/9/9/9 b RB4G4S4N4L18P 1",
                "初形が不正です: 受方玉がありません。",
            ),
            (
                "4k4/9/9/9/9/9/4K4/4K4/9 b RB4G4S4N4L16P 1",
                "初形が不正です: 攻方玉が2枚以上あります。",
            ),
        ] {
            assert_eq!(
                decode_and_validate_position(sfen).unwrap_err(),
                want,
                "{}",
                sfen
            );
        }
    }

    #[test]
    fn accepts_valid_positions() {
        // 攻方玉なしは通常の詰将棋なので許容する。
        for sfen in [
            "3+pks3/9/4+P4/9/9/8B/9/9/9 b S2rb4g2s4n4l16p 1",
            "4k4/9/9/9/9/9/4K4/9/9 b RB4G4S4N4L18P 1",
        ] {
            assert!(decode_and_validate_position(sfen).is_ok(), "{}", sfen);
        }
    }

    #[test]
    fn keeps_existing_validations() {
        assert_eq!(
            decode_and_validate_position("3k4P/9/9/9/9/9/9/9/9 b RB4G4S4N4L17P 1").unwrap_err(),
            "初形が不正です: 行きどころのない駒があります。"
        );
    }
}
