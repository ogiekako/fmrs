// https://github.com/na2hiro/json-kifu-format

pub type JsonKifuFormat = shogi_kifu_converter::jkf::JsonKifuFormat;
pub type MoveFormat = shogi_kifu_converter::jkf::MoveFormat;
pub type Color = shogi_kifu_converter::jkf::Color;
pub type Kind = shogi_kifu_converter::jkf::Kind;
pub type Initial = shogi_kifu_converter::jkf::Initial;
pub type Piece = shogi_kifu_converter::jkf::Piece;
pub type Preset = shogi_kifu_converter::jkf::Preset;
pub type StateFormat = shogi_kifu_converter::jkf::StateFormat;
pub type PlaceFormat = shogi_kifu_converter::jkf::PlaceFormat;
pub type MoveMoveFormat = shogi_kifu_converter::jkf::MoveMoveFormat;
pub type Hand = shogi_kifu_converter::jkf::Hand;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde() {
        for data in [
            r#"{
                "header": {
                  "先手": "na2hiro",
                  "後手": "うひょ"
                },
                "initial": null,
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
                "initial": null,
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
            let jkf: JsonKifuFormat = serde_json::from_str(data).unwrap();
            let serialized = serde_json::to_string(&jkf).unwrap();

            let want: serde_json::Value = serde_json::from_str(data).unwrap();
            let got: serde_json::Value = serde_json::from_str(&serialized).unwrap();
            pretty_assertions::assert_eq!(got, want);
        }
    }
}
