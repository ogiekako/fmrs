use anyhow::bail;
use percent_encoding::utf8_percent_encode;
use percent_encoding::NON_ALPHANUMERIC;
use position::PositionAux;
use url::Url;

/// SFEN format is defined in
/// https://web.archive.org/web/20080131070731/http://www.glaurungchess.com/shogi/usi.html
/// Use https://sfenreader.appspot.com/ja/create_board.html to a convert Shogi
/// board to an SFEN and vice versa.
use crate::piece::*;
use crate::position::*;

static PAWN_DROP_STEP: &str = "-1";

fn encode_piece(c: Color, mut k: Kind) -> String {
    let mut res = String::new();
    if let Some(x) = k.unpromote() {
        res.push('+');
        k = x;
    }
    let mut ch = match k {
        Pawn => 'P',
        Lance => 'L',
        Knight => 'N',
        Silver => 'S',
        Gold => 'G',
        Bishop => 'B',
        Rook => 'R',
        King => 'K',
        _ => panic!("Unexpected piece {:?}", k),
    };
    if c == Color::WHITE {
        ch = ch.to_lowercase().next().unwrap();
    }
    res.push(ch);
    res
}

fn decode_hand_kind(ch: char) -> anyhow::Result<(Color, Kind)> {
    for c in Color::iter() {
        for k in Kind::iter() {
            if !k.is_hand_piece() {
                continue;
            }
            if encode_piece(c, k).chars().next().unwrap() == ch {
                return Ok((c, k));
            }
        }
    }
    bail!("Illegal hand kind {}", ch)
}

pub fn encode_position(board: &PositionAux) -> String {
    let mut res = String::new();
    let stone = *board.stone();
    for row in 0..9 {
        let mut count_empty = 0i32;
        for col in (0..9).rev() {
            if let Some(stone) = stone {
                if stone.get(Square::new(col, row)) {
                    if count_empty > 0 {
                        res.push_str(&count_empty.to_string());
                        count_empty = 0;
                    }
                    res.push('O');
                    continue;
                }
            }

            if let Some((c, k)) = board.get(Square::new(col, row)) {
                if count_empty > 0 {
                    res.push_str(&count_empty.to_string());
                }
                count_empty = 0;
                res.push_str(&encode_piece(c, k));
            } else {
                count_empty += 1;
            }
        }
        if count_empty > 0 {
            res.push_str(&count_empty.to_string());
        }
        if row < 8 {
            res.push('/');
        }
    }
    res.push(' ');

    res.push(match board.turn() {
        Color::BLACK => 'b',
        Color::WHITE => 'w',
    });
    res.push(' ');

    let mut has_hand = false;
    // The pieces are always listed in the order rook, bishop, gold, silver, knight, lance, pawn;
    // and with all black pieces before all white pieces.
    for c in [Color::BLACK, Color::WHITE].iter() {
        let c = *c;
        for k in [Rook, Bishop, Gold, Silver, Knight, Lance, Pawn].iter() {
            let k = *k;
            if !k.is_hand_piece() {
                continue;
            }
            let n = board.hands().count(c, k);
            if n > 1 {
                res.push_str(&n.to_string());
            }
            if n > 0 {
                has_hand = true;
                res.push_str(&encode_piece(c, k));
            }
        }
    }
    if !has_hand {
        res.push('-');
    }
    if board.pawn_drop() {
        res.push(' ');
        res.push_str(PAWN_DROP_STEP);
    } else {
        res.push_str(" 1");
    }
    res
}

// Ingore optional move count if any.
pub fn decode_position(sfen: &str) -> anyhow::Result<PositionAux> {
    let v: Vec<&str> = sfen.split(' ').collect();
    if v.len() < 3 {
        bail!("Insufficient number of fields");
    }
    let rows: Vec<&str> = v[0].split('/').collect();
    if rows.len() != 9 {
        bail!("There should be exactly 9 rows");
    }
    let mut board = PositionAux::default();
    let mut stone = BitBoard::empty();

    for row in 0..9 {
        let mut col = 9isize;
        let mut promote = false;
        for ch in rows[row].chars() {
            if ch == 'O' {
                if promote {
                    bail!("stone shouldn't be promoted");
                }
                col -= 1;
                if col < 0 {
                    bail!("Too long row");
                }
                stone.set(Square::new(col as usize, row));
                continue;
            }

            if ch == '+' {
                if promote {
                    bail!("+ shouldn't continue twice");
                }
                promote = true;
                continue;
            }
            if let Some(n) = ch.to_digit(10) {
                if promote {
                    bail!("Illegal occurence of +");
                }
                col -= n as isize;
                continue;
            }
            let mut found = false;
            'outer: for c in Color::iter() {
                for k in Kind::iter() {
                    if k.unpromote().is_some() {
                        continue;
                    }
                    if encode_piece(c, k).chars().next().unwrap() == ch {
                        found = true;
                        col -= 1;
                        if col < 0 {
                            bail!("Too long row");
                        }

                        board.set(
                            Square::new(col as usize, row),
                            c,
                            if promote { k.promote().unwrap() } else { k },
                        );
                        promote = false;
                        break 'outer;
                    }
                }
            }
            if !found {
                bail!("Illegal character {}", ch);
            }
        }
        if col != 0 {
            bail!("Illegal row length");
        }
    }
    if !stone.is_empty() {
        board.set_stone(stone);
    }

    match v[1] {
        "b" => board.set_turn(Color::BLACK),
        "w" => board.set_turn(Color::WHITE),
        _ => bail!("Illegal turn string {}", v[1]),
    }

    if v.get(3) == Some(&PAWN_DROP_STEP) {
        board.set_pawn_drop(true);
    }

    if v[2] == "-" {
        return Ok(board);
    }

    let mut hand_count = 0;
    for ch in v[2].chars() {
        if let Some(n) = ch.to_digit(10) {
            hand_count = hand_count * 10 + n;
            if hand_count >= 100 {
                bail!(&"Hand counts should be less than 100");
            }
            continue;
        }
        if hand_count == 0 {
            hand_count = 1;
        }
        let (c, k) = decode_hand_kind(ch)?;
        for _ in 0..hand_count {
            board.hands_mut().add(c, k);
        }
        hand_count = 0;
    }
    Ok(board)
}

pub fn sfen_to_image_url(sfen: &str) -> String {
    format!(
        "https://ogiekako.github.io/fmrs/?sfen={}",
        utf8_percent_encode(sfen, NON_ALPHANUMERIC)
    )
}

pub fn from_image_url(url: &str) -> anyhow::Result<String> {
    let url = Url::parse(url)?;
    let encoded_sfen = url
        .query_pairs()
        .find(|(key, _)| key == "sfen")
        .map(|(_, value)| Ok(value.to_string()))
        .unwrap_or_else(|| bail!("No sfen parameter"))?;
    Ok(percent_encoding::percent_decode_str(&encoded_sfen)
        .decode_utf8()?
        .chars()
        .collect())
}

#[test]
fn test_encode() {
    let mut board = PositionAux::default();

    board.set(Square::new(0, 0), Color::WHITE, Lance);
    board.set(Square::new(3, 1), Color::BLACK, Pawn);
    board.set(Square::new(6, 1), Color::BLACK, ProRook);
    board.set(Square::new(7, 1), Color::WHITE, Lance);
    board.set(Square::new(0, 2), Color::WHITE, Pawn);
    board.set(Square::new(1, 2), Color::WHITE, Pawn);
    board.set(Square::new(3, 2), Color::BLACK, Gold);
    board.set(Square::new(4, 2), Color::BLACK, Bishop);
    board.set(Square::new(5, 2), Color::WHITE, Pawn);
    board.set(Square::new(8, 2), Color::WHITE, Pawn);
    board.set(Square::new(4, 3), Color::WHITE, Pawn);
    board.set(Square::new(6, 3), Color::WHITE, Silver);
    board.set(Square::new(7, 3), Color::WHITE, Pawn);
    board.set(Square::new(8, 3), Color::WHITE, King);
    board.set(Square::new(2, 4), Color::BLACK, Gold);
    board.set(Square::new(5, 4), Color::BLACK, Pawn);
    board.set(Square::new(7, 4), Color::WHITE, Knight);
    board.set(Square::new(8, 4), Color::BLACK, Knight);
    board.set(Square::new(0, 5), Color::BLACK, Pawn);
    board.set(Square::new(1, 5), Color::BLACK, Pawn);
    board.set(Square::new(4, 5), Color::BLACK, Pawn);
    board.set(Square::new(6, 5), Color::BLACK, Pawn);
    board.set(Square::new(8, 5), Color::BLACK, Pawn);
    board.set(Square::new(6, 6), Color::BLACK, Silver);
    board.set(Square::new(7, 6), Color::BLACK, Pawn);
    board.set(Square::new(1, 7), Color::WHITE, ProRook);
    board.set(Square::new(5, 7), Color::BLACK, Gold);
    board.set(Square::new(6, 7), Color::BLACK, Silver);
    board.set(Square::new(7, 7), Color::BLACK, King);
    board.set(Square::new(0, 8), Color::BLACK, Lance);
    board.set(Square::new(4, 8), Color::WHITE, ProPawn);
    board.set(Square::new(7, 8), Color::BLACK, Knight);
    board.set(Square::new(8, 8), Color::BLACK, Lance);
    board.hands_mut().add(Color::BLACK, Silver);
    board.hands_mut().add(Color::WHITE, Bishop);
    board.hands_mut().add(Color::WHITE, Gold);
    board.hands_mut().add(Color::WHITE, Knight);
    board.hands_mut().add(Color::WHITE, Pawn);
    board.hands_mut().add(Color::WHITE, Pawn);
    board.hands_mut().add(Color::WHITE, Pawn);

    board.set_turn(Color::WHITE);

    assert_eq!(
        "8l/1l+R2P3/p2pBG1pp/kps1p4/Nn1P2G2/P1P1P2PP/1PS6/1KSG3+r1/LN2+p3L w Sbgn3p 1",
        &encode_position(&board)
    );
}

#[test]
fn test_decode() {
    assert_eq!(
        "8l/1l+R2P3/p2pBG1pp/kps1p4/Nn1P2G2/P1P1P2PP/1PS6/1KSG3+r1/LN2+p3L w Sbgn3p 1",
        &encode_position(
            &decode_position(
                "8l/1l+R2P3/p2pBG1pp/kps1p4/Nn1P2G2/P1P1P2PP/1PS6/1KSG3+r1/LN2+p3L w Sbgn3p 1"
            )
            .expect("Failed to decode")
        )
    );
    assert_eq!(
        "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
        &encode_position(
            &decode_position("lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1")
                .expect("Failed to decode")
        )
    );
    assert_eq!(
        "3sks3/9/4+P4/9/7B1/9/9/9/9 b S2rb4gs4n4l17p 1",
        &encode_position(
            &decode_position("3sks3/9/4+P4/9/7B1/9/9/9/9 b S2rb4gs4n4l17p 1")
                .expect("Failed to decode")
        )
    );
}

fn encode_square(pos: Square) -> String {
    format!(
        "{}{}",
        pos.col() + 1,
        char::from_u32(pos.row() as u32 + b'a' as u32).unwrap()
    )
}

fn decode_square(s: &str) -> anyhow::Result<Square> {
    let cs: Vec<char> = s.chars().collect();
    if cs.len() != 2 {
        bail!("{} should have length 2", s);
    }
    for r in &['a', '1'] {
        let col = (cs[0] as usize).wrapping_sub('1' as usize);
        let row = (cs[1] as usize).wrapping_sub(*r as usize);

        if row < 9 && col < 9 {
            return Ok(Square::new(col, row));
        }
    }
    bail!("Illegal pos")
}

pub fn decode_move(s: &str) -> anyhow::Result<Movement> {
    let cs: Vec<char> = s.chars().collect();
    if cs.len() < 4 {
        bail!("Move too short");
    }
    Ok(if cs[1] == '*' {
        Movement::Drop(decode_square(&s[2..])?, decode_hand_kind(cs[0])?.1)
    } else {
        let mut promote = false;
        if cs.len() > 4 {
            promote = true;
            if cs[4] != '+' {
                bail!("Invalid move");
            }
        }
        Movement::move_without_hint(decode_square(&s[0..2])?, decode_square(&s[2..4])?, promote)
    })
}

// USI format defined in http://hgm.nubati.net/usi.html.
// e.g. "4e3c+ P*3d 7g7f"
// As an original extension, it also allows forms like "4533", which means "4e3c".
pub fn decode_moves(sfen: &str) -> anyhow::Result<Vec<Movement>> {
    if sfen.is_empty() {
        return Ok(vec![]);
    }
    sfen.split(' ').map(decode_move).collect()
}

pub fn encode_move(m: &Movement) -> String {
    match m {
        Movement::Drop(pos, k) => {
            format!("{}*{}", encode_piece(Color::BLACK, *k), encode_square(*pos))
        }
        Movement::Move {
            source: from,
            dest: to,
            promote,
            ..
        } => format!(
            "{}{}{}",
            encode_square(*from),
            encode_square(*to),
            if *promote { "+" } else { "" }
        ),
    }
}

#[cfg(test)]
pub mod tests {
    use url::Url;

    use crate::{piece::Kind, position::Movement, position::Square};

    use super::{decode_moves, decode_position, encode_position, position::PositionAux};
    #[test]
    fn test_decode_moves() {
        assert_eq!(
            vec![
                Movement::move_without_hint(Square::new(0, 5), Square::new(4, 1), true,),
                Movement::move_without_hint(Square::new(3, 0), Square::new(4, 1), false,),
                Movement::Drop(Square::new(3, 1), Kind::Silver),
            ],
            decode_moves("1f5b+ 4a5b S*4b").unwrap()
        );
    }

    extern crate percent_encoding;
    fn to_url(sfen: &str) -> String {
        let mut url = Url::parse("https://ogiekako.github.io/fmrs/").unwrap();
        url.query_pairs_mut().append_pair("sfen", sfen);
        url.to_string()
    }

    pub fn encode_position_url(board: &mut PositionAux) -> String {
        to_url(&encode_position(board))
    }

    pub const START: &str = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";
    // The example in https://web.archive.org/web/20080131070731/http://www.glaurungchess.com/shogi/usi.html
    pub const RYUO: &str =
        "8l/1l+R2P3/p2pBG1pp/kps1p4/Nn1P2G2/P1P1P2PP/1PS6/1KSG3+r1/LN2+p3L w Sbgn3p 1";
    // https://ogiekako.github.io/fmrs/?sfen=8l%2F1l%2BR2P3%2Fp2pBG1pp%2Fkps1p4%2FNn1P2G2%2FP1P1P2PP%2F1PS6%2F1KSG3%2Br1%2FLN2%2Bp3L+b+Sbgn3p+1

    #[test]
    fn test_encode_position_url() {
        use pretty_assertions::assert_eq;

        let mut board = decode_position(START).unwrap();
        assert_eq!(encode_position_url(&mut board),
    "https://ogiekako.github.io/fmrs/?sfen=lnsgkgsnl%2F1r5b1%2Fppppppppp%2F9%2F9%2F9%2FPPPPPPPPP%2F1B5R1%2FLNSGKGSNL+b+-+1");

        let mut board = decode_position(RYUO).unwrap();
        assert_eq!(encode_position_url(&mut board),
    "https://ogiekako.github.io/fmrs/?sfen=8l%2F1l%2BR2P3%2Fp2pBG1pp%2Fkps1p4%2FNn1P2G2%2FP1P1P2PP%2F1PS6%2F1KSG3%2Br1%2FLN2%2Bp3L+w+Sbgn3p+1")
    }
}
