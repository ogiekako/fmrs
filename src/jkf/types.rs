// https://github.com/na2hiro/json-kifu-format

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Default, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct JsonKifFormat {
    pub header: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial: Option<Initial>,
    pub moves: Vec<MoveFormat>,
}

#[derive(Default, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct Initial {
    pub preset: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<StateFormat>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct StateFormat {
    pub color: Color,
    pub board: Vec<Vec<Piece>>,
    pub hands: Vec<BTreeMap<RawKind, usize>>,
}

#[derive(Serialize_repr, Deserialize_repr, Clone, Copy, Eq, PartialEq, Debug)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    White = 1,
}

#[derive(Default, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Debug)]

pub struct Piece {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
}

#[derive(Serialize, Deserialize, Hash, Clone, Copy, Eq, PartialEq, Debug, PartialOrd, Ord)]

pub enum RawKind {
    FU,
    KY,
    KE,
    GI,
    KI,
    KA,
    HI,
}

#[derive(Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Debug)]

pub enum Kind {
    FU,
    KY,
    KE,
    GI,
    KI,
    KA,
    HI,
    OU,
    TO,
    NY,
    NK,
    NG,
    UM,
    RY,
}

#[derive(Default, Serialize, Deserialize, PartialEq, Eq, Debug)]

pub struct MoveFormat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#move: Option<MoveMoveFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<Time>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub special: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forks: Option<Vec<Vec<MoveFormat>>>,
}

#[derive(Default, Serialize, Deserialize, PartialEq, Eq, Debug)]

pub struct Time {
    pub now: TimeFormat,
    pub total: TimeFormat,
}

#[derive(Default, Serialize, Deserialize, PartialEq, Eq, Debug)]

pub struct TimeFormat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h: Option<usize>,
    pub m: usize,
    pub s: usize,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]

pub struct MoveMoveFormat {
    pub color: Color,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<PlaceFormat>,
    pub to: PlaceFormat,
    pub piece: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promote: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture: Option<Kind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative: Option<String>,
}

#[derive(Default, Serialize, Deserialize, Eq, PartialEq, Debug)]

pub struct PlaceFormat {
    pub x: usize,
    pub y: usize,
}

#[cfg(test)]
mod tests {
    use super::JsonKifFormat;

    #[test]
    fn serde() {
        for data in [
            r#"{
                "header": {
                  "先手": "na2hiro",
                  "後手": "うひょ"
                },
                "moves": [
                  {},
                  {"move":{"from":{"x":7,"y":7},"to":{"x":7,"y":6},"color":0,"piece":"FU"}},
                  {"move":{"from":{"x":3,"y":3},"to":{"x":3,"y":4},"color":1,"piece":"FU"}},
                  {"move":{"from":{"x":8,"y":8},"to":{"x":2,"y":2},"color":0,"piece":"KA","capture":"KA","promote":false}},
                  {"move":{"from":{"x":3,"y":1},"to":{"x":2,"y":2},"color":1,"piece":"GI","capture":"KA","same":true}},
                  {"move":{"to":{"x":4,"y":5},"color":0,"piece":"KA"}},
              
                  {"special": "CHUDAN"}
                ]
              }"#,
            r#"{
                "header": {},
                "moves": [
                  {"comments":["分岐の例"]},
                  {"move":{"from":{"x":7,"y":7},"to":{"x":7,"y":6},"color":0,"piece":"FU"}},
                  {"move":{"from":{"x":3,"y":3},"to":{"x":3,"y":4},"color":1,"piece":"FU"}, "comments":["次の手で二種類が考えられる：７七桂か２二角成である．","２二角成を選ぶと筋違い角となる．"]},
                  {"move":{"from":{"x":8,"y":9},"to":{"x":7,"y":7},"color":0,"piece":"KE"}, "forks":[
                    [
                      {"move":{"from":{"x":8,"y":8},"to":{"x":2,"y":2},"color":0,"piece":"KA","capture":"KA","promote":false}},
                      {"move":{"from":{"x":3,"y":1},"to":{"x":2,"y":2},"color":1,"piece":"GI","capture":"KA","same":true}},
                      {"move":{"to":{"x":4,"y":5},"color":0,"piece":"KA"}}
                    ]
                  ]},
                  {"move":{"from":{"x":2,"y":2},"to":{"x":7,"y":7},"color":1,"piece":"KA","capture":"KE","promote":true,"same":true}},
                  {"move":{"from":{"x":8,"y":8},"to":{"x":7,"y":7},"color":0,"piece":"KA","capture":"UM","same":true}},
                  {"move":{"to":{"x":3,"y":3},"color":1,"piece":"KE","relative":"H"}}
                ]
              }"#,
            r#"{
                "header": {},
                "initial": {"preset": "6"},
                "moves": [
                  {},
                  {"move":{"from":{"x":5,"y":1},"to":{"x":4,"y":2},"color":1,"piece":"OU"}},
                  {"move":{"from":{"x":7,"y":7},"to":{"x":7,"y":6},"color":0,"piece":"FU"}},
                  {"move":{"from":{"x":6,"y":1},"to":{"x":7,"y":2},"color":1,"piece":"KI"}}
                ]
              }"#,
            r#"{
                "header": {},
                "initial": {
                  "preset": "OTHER",
                  "data": {
                    "board": [
                      [{"color":1, "kind":"KY"}, {                      },{"color":1, "kind":"FU"}, {}, {}, {}, {"color":0, "kind":"FU"}, {                      }, {"color":0, "kind":"KY"}],
                      [{"color":1, "kind":"KE"}, {"color":1, "kind":"KA"},{"color":1, "kind":"FU"}, {}, {}, {}, {                      }, {"color":0, "kind":"HI"}, {"color":0, "kind":"KE"}],
                      [{"color":1, "kind":"GI"}, {                      },{"color":1, "kind":"FU"}, {}, {}, {}, {"color":0, "kind":"FU"}, {                      }, {"color":0, "kind":"GI"}],
                      [{"color":1, "kind":"KI"}, {                      },{"color":1, "kind":"FU"}, {}, {}, {}, {"color":0, "kind":"FU"}, {                      }, {"color":0, "kind":"KI"}],
                      [{"color":1, "kind":"OU"}, {                      },{"color":1, "kind":"FU"}, {}, {}, {}, {"color":0, "kind":"FU"}, {                      }, {"color":0, "kind":"OU"}],
                      [{"color":1, "kind":"KI"}, {                      },{"color":1, "kind":"FU"}, {}, {}, {}, {"color":0, "kind":"FU"}, {                      }, {"color":0, "kind":"KI"}],
                      [{"color":1, "kind":"GI"}, {                      },{"color":1, "kind":"FU"}, {}, {}, {}, {                      }, {                      }, {"color":0, "kind":"GI"}],
                      [{"color":1, "kind":"KE"}, {"color":1, "kind":"HI"},{"color":1, "kind":"FU"}, {}, {}, {}, {"color":0, "kind":"FU"}, {"color":0, "kind":"KA"}, {"color":0, "kind":"KE"}],
                      [{"color":1, "kind":"KY"}, {                      },{"color":1, "kind":"FU"}, {}, {}, {}, {"color":0, "kind":"FU"}, {                      }, {"color":0, "kind":"KY"}]
                    ],
                    "color": 0,
                    "hands":[
                      {"FU":0,"KY":0,"KE":0,"GI":0,"KI":0,"KA":0,"HI":0},
                      {"FU":0,"KY":0,"KE":0,"GI":0,"KI":0,"KA":0,"HI":0}
                    ]
                  }
                },
                "moves": [
                  {"comments": ["飛車角先落ち．"]},
                  {"move":{"from":{"x":2,"y":8},"to":{"x":2,"y":3},"color":0,"piece":"HI","promote":true,"capture":"FU"}}
                ]
              }"#,
        ] {
            let jkf: JsonKifFormat = serde_json::from_str(data).unwrap();
            let serialized = serde_json::to_string(&jkf).unwrap();

            let want: serde_json::Value = serde_json::from_str(data).unwrap();
            let got: serde_json::Value = serde_json::from_str(&serialized).unwrap();
            pretty_assertions::assert_eq!(got, want);
        }
    }
}
