// https://github.com/na2hiro/json-kifu-format

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct JsonKifFormat {
    header: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    initial: Option<Initial>,
    moves: Vec<MoveFormat>,
}

#[derive(Serialize, Deserialize)]
pub struct Initial {
    preset: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<StateFormat>,
}

#[derive(Serialize, Deserialize)]
pub struct StateFormat {
    color: Color,
    board: Vec<Vec<Piece>>,
    hands: Vec<HashMap<RawKind, usize>>,
}

type Color = u8;

#[derive(Serialize, Deserialize)]

pub struct Piece {
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<Kind>,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash)]

enum RawKind {
    FU,
    KY,
    KE,
    GI,
    KI,
    KA,
    HI,
}

#[derive(Serialize, Deserialize)]

enum Kind {
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

#[derive(Serialize, Deserialize)]

pub struct MoveFormat {
    #[serde(skip_serializing_if = "Option::is_none")]
    comments: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#move: Option<MoveMoveFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time: Option<Time>,
    #[serde(skip_serializing_if = "Option::is_none")]
    special: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    forks: Option<Vec<Vec<MoveFormat>>>,
}

#[derive(Serialize, Deserialize)]

pub struct Time {
    now: TimeFormat,
    total: TimeFormat,
}

#[derive(Serialize, Deserialize)]

pub struct TimeFormat {
    #[serde(skip_serializing_if = "Option::is_none")]
    h: Option<usize>,
    m: usize,
    s: usize,
}

#[derive(Serialize, Deserialize)]

pub struct MoveMoveFormat {
    color: Color,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<PlaceFormat>,
    to: PlaceFormat,
    piece: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    promote: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capture: Option<Kind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    relative: Option<String>,
}

#[derive(Serialize, Deserialize)]

pub struct PlaceFormat {
    x: usize,
    y: usize,
}

#[cfg(test)]
mod tests {
    use super::JsonKifFormat;

    #[test]
    fn serde() {
        let data = r#"
{
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
}
"#;
        let jkf: JsonKifFormat = serde_json::from_str(data).unwrap();
        let serialized = serde_json::to_string(&jkf).unwrap();

        let want: serde_json::Value = serde_json::from_str(data).unwrap();
        let got: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        pretty_assertions::assert_eq!(got, want);
    }
}
