use crate::piece::*;
use crate::position::*;

type Solution = Vec<Movement>;

pub fn solve(board: &Position, size_limit: Option<usize>) -> Result<Vec<Solution>, String> {
    if board.turn() != Black {
        return Err("The turn should be from black".into());
    }
    if board.checked(White) {
        return Err("on black's turn, white is already checked.".into());
    }
    Ok(solve_inner(board, size_limit))
}

use std::collections::{HashMap, VecDeque};

fn solve_inner(board: &Position, limit: Option<usize>) -> Vec<Solution> {
    debug_assert_ne!(board.turn() == Black, board.checked(White));

    // position -> (min step, undo tokens)
    let mut memo = HashMap::new();
    memo.insert(board.clone(), (0, vec![]));
    let mut queue = VecDeque::new();
    queue.push_back((0, board.clone()));

    let mut goal_step = None;
    let mut goals = vec![];
    while let Some((step, board)) = queue.pop_front() {
        let n_step = step + 1;
        debug_assert!(memo.get(&board).is_some());

        if let Some(s) = goal_step {
            if s < step {
                break;
            }
        }

        let mut movable = false;
        for (np, token) in board.next_positions().unwrap() {
            movable = true;
            if goal_step.is_some() {
                break;
            }

            if let Some((min_step, tokens)) = memo.get_mut(&np) {
                if *min_step == n_step {
                    tokens.push(token);
                }
                continue;
            }
            // TODO: Use RC to avoid cloning.
            memo.insert(np.clone(), (n_step, vec![token]));
            queue.push_back((n_step, np));
        }
        if !movable && board.turn() == White && !board.was_pawn_drop() {
            // Checkmate
            goals.push(board);
            goal_step = Some(step);
        }
    }
    let mut res = vec![];
    for mut g in goals.into_iter() {
        update(&mut res, &mut vec![], &memo, &mut g, limit);
    }
    res
}

fn update(
    res: &mut Vec<Solution>,
    mut rev_sol: &mut Solution,
    memo: &HashMap<Position, (usize, Vec<UndoToken>)>,
    g: &mut Position,
    limit: Option<usize>,
) {
    if let Some(lim) = limit {
        if lim <= res.len() {
            return;
        }
    }
    let (step, toks) = memo.get(&g).unwrap();
    if *step == 0 {
        res.push(rev_sol.clone().into_iter().rev().collect());
        return;
    }
    for tok in toks {
        let mv = g.undo(tok);

        rev_sol.push(mv);
        update(res, &mut rev_sol, memo, g, limit);
        let mv = rev_sol.pop().unwrap();

        g.do_move(&mv);
    }
}

#[test]
fn test_solve() {
    use crate::sfen;

    for tc in vec![
        (
            "3+pks3/9/4+P4/9/9/8B/9/9/9 b S2rb4g2s4n4l16p 1",
            vec!["1f5b+ 4a5b S*4b"],
        ),
        (
            "9/9/9/3bkb3/9/3+R1+R3/9/9/9 b 4g4s4n4l18p 1",
            vec!["4644 5463 4433 6354 B*63", "6664 5443 6473 4354 B*43"],
        ),
        (
            "9/6b2/7k1/5b3/7sL/9/9/9/9 b Rr4g3s4n3l18p 1",
            vec!["1513+ 2324 1323 2414 R*13"],
        ),
        // http://cavesfairy.g1.xrea.com/pub/qgfairy/
        (
            // 06-07 (57 steps)
            "9/9/9/9/7bb/1ppssssp1/K5k2/+RL1l1gg2/rL3gg1+l b 4N15P 1",
            vec!["N*29 3829 P*38 3747 N*59 4859 P*48 4757 N*69 6869+ P*58 5767 P*68 6778 P*79 6979 8886 7877 P*78 7768 P*69 5969 7877 6867 P*68 6758 P*59 4959 6867 5857 P*58 5748 P*49 3949 5857 4847 P*48 4738 P*39 2939 4847 3837 P*38 3728 3837 3938 P*29 2817 P*18 1716 N*28 3828 1817 1627 2928 2737 G*38"]
        ),
    ] {
        let board = sfen::decode_position(tc.0).expect("Failed to parse");
        let want = Ok(tc
            .1
            .into_iter()
            .map(|x| sfen::decode_moves(x).unwrap())
            .collect());
        eprintln!("Solving {:?}", board);
        let got = solve(&board, None);
        assert_eq!(got, want);
    }
}

#[test]
fn no_answer() {
    use crate::sfen;

    for sfen in [
        "4k4/9/4P4/9/9/8p/8K/9/9 b G2r2b3g4s4n4l16p 1",
        "9/9/9/5bp1G/6k2/6l1P/8K/9/8N b 2rb3g4s3n3l16p 1",
        "9/9/9/9/9/pp7/kl7/9/K8 b P2r2b4g4s4n3l15p 1",
    ] {
        let board = sfen::decode_position(sfen).unwrap();
        let got = solve(&board, None).unwrap();
        let want: Vec<Vec<Movement>> = vec![];
        eprintln!("Solving {:?}", board);
        assert_eq!(got, want);
    }
}
