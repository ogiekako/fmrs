use std::{
    fmt::Formatter,
    ops::{Index, IndexMut},
};

use fmrs_core::{
    memo::MemoStub,
    nohash::NoHashMap,
    position::{advance::advance, AdvanceOptions, Position, PositionExt},
};

pub fn direct_mate(sfen: &str) -> anyhow::Result<Option<usize>> {
    let position = Position::from_sfen(sfen)?;

    let mut graph = Graph::new(position);

    loop {
        graph.advance();

        match graph.result() {
            Status::Solved(steps) => {
                println!("Solved in {} steps", steps);
                return Ok(Some(steps as usize));
            }
            Status::Intermediate => continue,
            Status::Unsolvable => {
                println!("Unsolvable");
                return Ok(None);
            }
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Value(u32);

impl Value {
    const ZERO: Self = Self(0);
    const SMALLEST_OMEGA: Self = Self(u32::MAX / 4);
    const OMEGA: Self = Self(u32::MAX / 2);
    const INF: Self = Self(u32::MAX);

    fn omega(step: usize) -> Self {
        Self(Self::OMEGA.0 - step as u32)
    }

    fn is_omega(self) -> bool {
        self < Self::INF && self >= Self::SMALLEST_OMEGA
    }

    fn is_inf(self) -> bool {
        self == Self::INF
    }

    fn next(self) -> Self {
        if self >= Self::SMALLEST_OMEGA {
            self
        } else {
            Self(self.0 + 1)
        }
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.is_omega() {
            if self == &Self::OMEGA {
                write!(f, "ω")
            } else {
                write!(f, "ω-{}", Self::OMEGA.0 - self.0)
            }
        } else if *self == Value::INF {
            write!(f, "∞")
        } else {
            write!(f, "{}", self.0)
        }
    }
}

#[derive(Debug)]
struct Node {
    position: Position,
    // 0, 1, ... - solvable
    // inf       - unsolvable
    // omega     - not calculated
    value: Value,
    expanded: Option<usize>,
    neighbors: Vec<usize>,
    rev_neighbors: Vec<usize>,
}

impl Node {
    fn new(position: Position, step: usize) -> Self {
        Self {
            position,
            value: Value::omega(step),
            expanded: None,
            neighbors: vec![],
            rev_neighbors: vec![],
        }
    }
}

#[derive(Debug, Default)]
struct Nodes {
    ids: NoHashMap<usize>,
    nodes: Vec<Node>,
}

impl Nodes {
    fn push(&mut self, node: Node) -> usize {
        let id = self.nodes.len();
        self.ids.insert(node.position.digest(), id);
        self.nodes.push(node);
        id
    }

    fn len(&self) -> usize {
        self.nodes.len()
    }

    fn index_of(&self, digest: &u64) -> Option<usize> {
        self.ids.get(&digest).copied()
    }
}

impl Index<usize> for Nodes {
    type Output = Node;
    fn index(&self, index: usize) -> &Self::Output {
        &self.nodes[index]
    }
}

impl IndexMut<usize> for Nodes {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.nodes[index]
    }
}

enum Status {
    Solved(u32),
    Intermediate,
    Unsolvable,
}

#[derive(Debug, Default)]
struct Graph {
    // black, white
    nodes: [Nodes; 2],
    step: usize,
}

const BLACK: usize = 0;
#[allow(dead_code)]
const WHITE: usize = 1;

impl Graph {
    fn new(position: Position) -> Self {
        let mut graph = Self::default();
        graph.nodes[BLACK].push(Node::new(position, 0));
        graph
    }

    fn advance(&mut self) {
        let turn = self.step % 2;

        let mut movements = vec![];

        let n = self.nodes[turn].len();
        for i in 0..n {
            if !self.should_expand(turn, i) {
                continue;
            }
            self.nodes[turn][i].expanded = Some(self.step);

            movements.clear();
            // TODO: check drop pawn mate from white.
            advance(
                &self.nodes[turn][i].position,
                &mut MemoStub::default(),
                1,
                &AdvanceOptions {
                    no_memo: true,
                    ..AdvanceOptions::default()
                },
                &mut movements,
            )
            .unwrap();

            for movement in movements.iter() {
                let digest = self.nodes[turn][i].position.moved_digest(movement);

                let id = match self.nodes[turn ^ 1].index_of(&digest) {
                    Some(id) => id,
                    None => {
                        let mut np = self.nodes[turn][i].position.clone();
                        np.do_move(movement);
                        self.nodes[turn ^ 1].push(Node::new(np, self.step + 1))
                    }
                };
                self.add_neighbor(turn, i, id);
            }
        }
        for i in 0..n {
            if self.nodes[turn][i].expanded != Some(self.step) {
                continue;
            }

            self.update_value(turn, i);
        }

        let smallest_omega = Value::omega(self.step + 1);
        for color in 0..2 {
            for i in 0..self.nodes[color].len() {
                let value = self.nodes[color][i].value;
                if value.is_omega() && value != smallest_omega {
                    self.nodes[color][i].value = Value::INF;
                }
            }
        }

        self.step += 1;
    }

    fn add_neighbor(&mut self, turn: usize, from: usize, to: usize) {
        self.nodes[turn][from].neighbors.push(to);
        self.nodes[turn ^ 1][to].rev_neighbors.push(from);
    }

    fn should_expand(&self, turn: usize, i: usize) -> bool {
        if self.nodes[turn][i].expanded.is_some() {
            return false;
        }

        if turn == BLACK && i == 0 {
            return true;
        }

        self.nodes[turn][i]
            .rev_neighbors
            .iter()
            .any(|&j| self.nodes[turn ^ 1][j].value.is_omega())
    }

    fn update_value(&mut self, turn: usize, i: usize) {
        assert!(self.nodes[turn][i].expanded.is_some());

        let values = self.nodes[turn][i]
            .neighbors
            .iter()
            .map(|&j| self.nodes[turn ^ 1][j].value);

        let value = if turn == BLACK {
            values.min().map(Value::next).unwrap_or(Value::INF)
        } else {
            values.max().map(Value::next).unwrap_or(Value::ZERO)
        };

        if self.nodes[turn][i].value == value {
            return;
        }
        self.nodes[turn][i].value = value;

        for j in 0..self.nodes[turn][i].rev_neighbors.len() {
            self.update_value(turn ^ 1, self.nodes[turn][i].rev_neighbors[j]);
        }
    }

    fn result(&self) -> Status {
        let value = self.nodes[BLACK][0].value;
        if value.is_omega() {
            Status::Intermediate
        } else if value.is_inf() {
            Status::Unsolvable
        } else {
            Status::Solved(value.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_mate() {
        for (sfen, step) in [
            ("4k4/9/4P4/9/9/9/9/9/9 b G2r2b3g4s4n4l17p", Some(1)),
            ("3sks3/9/4+P4/9/9/8B/9/9/9 b S2rb4gs4n4l17p", Some(3)),
            // ("4k4/9/PPPPPPPPP/9/9/9/9/9/9 b B4L2rb4g4s4n9p", Some(11)),
        ] {
            assert_eq!(direct_mate(sfen).unwrap(), step);
        }
    }
}
