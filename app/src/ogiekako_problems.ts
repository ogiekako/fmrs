import { moves_to_kif } from "./wasm_api";
import type { Response as SolveApiResponse } from "./solve";
import { parseProblemSfen, ParsedProblemSfen } from "./model/sfen";

export interface OgiekakoProblem extends ParsedProblemSfen {
  solutionSeq: string;
  solutionRules: string[];
}

const OGIEKAKO_ALIASES = new Set(["ogiekako", "荻江香木", "荻絵香木"]);

// Precomputed grammar-compressed solutions keyed by problem ID (also defines canonical order within same month).
const SOLUTIONS_BY_ID: Record<string, { seq: string; rules: string[] }> = {
  "forest-04-05_11": {
    "seq": "6g5h 4g4f 6f5g 4f4e 5g4f 4e5d 4f4e 5d6e 4e5d 6e5f 6h5g",
    "rules": []
  },
  "forest-05-10_31": {
    "seq": "1d2d P*2e L*2i L*2h 2i2h N*2g 2h2g 2f3f N*2h 3f4f L*4h S*4g 4h4g 4f5e S*4f 5e6e 6i8g G*7f 8g7f 6e7f G*8g 7f8g 5i8i B*8h 8i8h 9g8h B*6e R*7f 6e7f 8g9g R*9h",
    "rules": []
  },
  "forest-05-13_57": {
    "seq": "P*1e 1d1e P*1f 1e1f @0 9c9g 1g1f P*1g 1f1e 1g1f 1e1d 1f1e 1d1c 9g9c 5e7c 1e1d 1c1d P*1e 1d1e P*1f 1e1f @0 9c9g 6f6g+ @1 9g9f 6g6f 1h1g 1f1g 9f9g 4e3g+ @1 1h1g 1f1e 9g9e 6f6e 1g1f 1e1f 9e9f 3g3f @0 9f9g 7h7g @1 1h1g 1f1e 1g1f 1e1d 1f1e 1d1c 2h2c+",
    "rules": [
      "P*1g 1f1g",
      "P*1h 1g1f"
    ]
  },
  "forest-06-05_37": {
    "seq": "8i9i P*8h 9i8h P*7g 8h7g P*6f 7g6f P*5e 6f5e P*4d 5e4d P*3c 4d3c 2a2b 3c2b 2g2b P*1b 1a2a 1b1a+ 2a3b P*3c 3b3c P*3d 3c3d P*3e 3d2e P*2f 2e1f P*1g 1f2g 4i3h 2g2f 3h2g 2f2e P*2f 2e1e 1g1f",
    "rules": []
  },
  "forest-06-10_97": {
    "seq": "8d9c 9b9c P*9d 9c9d P*9e 9d9e P*9f 9e9f P*9g 9f9g 8i7i 9g9f 7i6i 9f9e 6i5i 8e8f P*9f 9e9f 5i6i 8f8g P*9g 9f9g 6i7i 8g8h P*9h 9g9f 7i6i 6g7h 9h9g 9f9e 9g9f 9e9d 9f9e 9d9e 6i5i 9e9d 5i4i 4h5h P*9e 9d9e 4i5i 5h6h P*9f 9e9d 5i4i 5g5h+ 9f9e @0 5h5g @2 5g6g @3 6g6f @2 6f7f @3 7f7e @2 7e8e @3 8e8d @1 P*9e 9d8e 3i4i 8e7e 4i3i 7e7f 3i4i 7f6f 4i3i 6f6g 3i4i 6g5g 4i3i 5g5h 3i4i 3h4i 1c1h 5h6i 1h6h 6i5i G*5h",
    "rules": [
      "9d9c 4i3i",
      "9e9d 9c9d",
      "@1 3i4i",
      "P*9e @0"
    ]
  },
  "forest-06-12_361": {
    "seq": "2e2d @33 @33 @33 @33 @32 @9 8d8e 9f8e P*9f 9e9f P*9g 9f9g P*9h 9g8h P*8i 8h7h P*7i 7h6h P*6i 6h5h P*5i 5h4h P*4i 4h3g P*3h 3g3f 3h3g 3f4g 4i4h 4g5g 5i5h 5g6g 6i6h 6g7g 7i7h 7g7f 7h7g 7f7e 7g7f 7e7d 7f7e 7d8c 9b8b 8c8b 9c9b+ 8b7b 9b8b 7b6a 5c5b",
    "rules": [
      "3d4e 2d4d",
      "@0 4e5e",
      "@1 4d4e",
      "@2 5e6d",
      "@3 4e6e",
      "@4 6d7d",
      "@5 6e6d",
      "@6 7d8e",
      "@7 6d8d",
      "@8 8e9e",
      "@9 8d9d",
      "@10 9e8e",
      "@11 9d9e",
      "@12 8e7d",
      "@13 9e9d",
      "@14 7d6e",
      "@15 9d6d",
      "@16 6e5e",
      "@17 6d6e",
      "@18 5e4d",
      "@19 6e4e",
      "@20 4d3d",
      "@21 4e4d",
      "@22 3d2e",
      "@23 4d2d",
      "@24 2e1e",
      "@25 2d1d",
      "@26 1e2e",
      "@27 1d1e",
      "@28 2e3d",
      "@29 1e1d",
      "@30 P*2d",
      "@31 1d2d",
      "@32 @32"
    ]
  },
  "hanran-25-03_75": {
    "seq": "3c3b P*8h 9i8h P*7g 8h7g 6e6f 7g6f P*5e 6f5e P*4d 5e4d 3d3c 4d3c P*2b N*2c 1b2c P*1b 1a1b P*1c 1b1c P*1d 1c1d P*1e 1d1e P*1f 1e1f P*1g 1f2g 3i2h 2g2h 4i3i 2h3i 5i4i 3i2i 4i3i 2i1i 3i2i 3g2i+ 7c8d+ P*8b 9a8b 7a7c 8b7c N*6d 7c6d P*5e 6d5e P*4f 5e4f P*3g 4f3g 2i2h 3g2h 1i2h L*2i 2h3g N*4i 3g4g N*3i 4g5h P*5i 5h6h P*6i 6h7h P*7i 7h8g P*8h 8g8f 8h8g 8f7e 8d8e 7e6e 4i5g 6e7f 8e8f",
    "rules": []
  },
  "forest-08-06_7": {
    "seq": "L*4i 5g4g 6d5b+ 7e5e 6h5f L*5g N*5h",
    "rules": []
  },
  "forest-10-04_7": {
    "seq": "B*9h 6h9h L*6i 9h6h 6i6h 6e5f R*5e",
    "rules": []
  },
  "forest-11-04_43": {
    "seq": "N*3h 4f3e 6a3a 3e4d 3a4a 4d5c 4a5a 5c6d 5a6a 6d7e 6a7a 7e6f 7a6a 6f5g 5i5h 5g4g 6a4a 4g3f 4a3a 3f4e 3a4a 4e3d 4a3a G*3b 3h4f 3d4e 3a4a 4e5f 5h5g 5f4g 4f5d G*4b P*4h 4g4f 4h4g 4f4e 4g4f 4e4d 4f4e 4d4c 5d4b+ 3b4b G*4d",
    "rules": []
  },
  "forest-12-03_9": {
    "seq": "L*9i P*9h 9i9h 9g8g P*8h 8g7g 5i6h 7g8f S*7g",
    "rules": []
  },
  "chain_207": {
    "seq": "7i6i 2d2e 1f1e @4 2e2f P*1f @7 8e8d @2 8d7e @3 7e7d @2 7d6e @3 6e6d @2 6d5e @3 5e5d @2 5d4e @3 4e4d @2 4d3e @3 3e3d @2 3d2e P*1f 1e1d 1f1e 1d2d 9e9d 2d3e 9d9e 3e4d 9e9d 4d5e 9d9e 5e6d 9e9d 6d7e 9d9e 7e8f 6i5i 7h7g 9e9f 8f7e 9f9e 7e6d 9e9d 6d5e 9d9e 5e4d 9e9d 4d3e 9d9e 3e2d 9e9d 2d1e P*1f 2e1f 9d9e @9 @0 @10 1e2e 5i6i 2e2f 6i5i 2f3f 5i6i 3f3g 6i5i 3g4g 5i6i 4g5g 6i7i 5g4g 7i6i 4g3g 6i5i 3g3f 5i6i 3f2f 6i5i 2f2e 5i6i 2e1e 6i5i @7 @4 3e2f 9d9e @9 4f5h+ P*1e 1d1c 6i7i 5h6h 1e1d 1c1d 7i6i @0 @10 @7 @4 3e2f 9d9e 1e2d 9e9d 2d3e 9d9e 3e4d 9e9d 4d5e 9d9e 5e6d 9e9d 6d7e 9d9e 7e8f 9e9f 8f8g 5i6i 6h7h 9f9g 8g8f 9g9f 8f7e 9f9e 7e6d 9e9d 6d5e 9d9e 5e4d 9e9d 4d3e 9d9e 3e2d 9e9d 2d1e 9d9e @10 @9 @4 1f2f P*1f 1e2e 5i6i 3e3f 9d9e 2e2d 6i7i 2d1d 1f1e",
    "rules": [
      "1d1e 9d9e",
      "1e1d 9e9d",
      "1f1e @0",
      "P*1f @1",
      "1d1e 6i5i",
      "1e1d 5i6i",
      "@5 5d3f",
      "@6 9e9d",
      "@1 3f5d",
      "@8 5i6i",
      "2f3e 6i5i"
    ]
  },
  "tsumeyon-04-07_45": {
    "seq": "5f6e 4d4e @9 4c4e @9 4b4e @9 4a4e @0 4e2e 2d2e L*1f 1e2d S*3e",
    "rules": [
      "4f4e 1f2e",
      "@0 L*1f",
      "@1 1e1f",
      "@2 6e5f",
      "@3 L*4f",
      "@4 4e4f",
      "@5 1f1e",
      "@6 L*1f",
      "@7 2e1f",
      "@8 5f6e"
    ]
  },
  "tsumeyon-04-08_51": {
    "seq": "9d9c 9a8c 7e7d 7c7d 9c9d 7d6e 9d9e 6e5f 9e9f 5f4e 9f9e 8c7e P*4f 4e4f 9e9f 4f3g 9f9g 3g2f 9g9f 2f1g 9f9g 7e6g P*1h 1g2f 9g9f 2f3e 9f9e 3e4f 9e9f 4f5e 9f9e 5e6f 9e9f 6f7g 9f9g 7g6h 4i5i 6g5i+ 9g9h 6h5g 9h9g 5g4f P*4g 4f3f 9g9f G*4f P*3g 3f4e 4g4f 4e4d G*4e",
    "rules": []
  },
  "tsumeyon-04-09_139": {
    "seq": "B*2h 1g2h B*4f @22 @22 @22 @22 @7 B*4f 5d4f @8 P*4g 4f4e 4g4f 4e5d N*6f 5d6e 6g7f 6e7f P*7g 7f7g P*7h 7g8h P*8i 8h8g 8i8h 8g8f 8h8g 8f8e 8g8f 8e8d 8f8e 8d8c 8e8d 8c7b 5f6d 7b6a S*6b",
    "rules": [
      "B*3g 4f3g",
      "@0 2h3g",
      "@1 B*5i",
      "@2 B*4h",
      "@3 5i4h",
      "@4 3g4f",
      "@5 4h3g",
      "@6 4f3e",
      "3g4f 3e4f",
      "@7 @8",
      "@9 B*2d",
      "@10 B*3e",
      "@11 2d3e",
      "@12 4f3g",
      "@13 B*5i",
      "@14 P*4h",
      "@15 5i4h",
      "@16 3g2h",
      "@17 4h3g",
      "@18 2h1g",
      "@19 3g2h",
      "@20 1g2h",
      "@21 3e4f"
    ]
  },
  "forest-15-09_87": {
    "seq": "7a5c L*8a 9a8a P*7a 8a7a P*6a 7a6a P*5a 6a5a P*4a 5a4a P*3a 4a3a 2b2a 3a2a 1a2a L*2b 2a3a 2b2a+ 3a4a 2a3a 4a5a 7b6a+ 5a6a P*6b 6a7a 6b6a+ 7a8a 6a7a 8a9a 7a8a 9a8a 5c6c 8a7a P*7b 7a8a 7b7a+ 8a9a 7a8a 9a9b 8a8b 9b9c 8b9b 2i9b 9e9d 9c9d P*9e 9d9e P*9f 9e9f P*9g 9f9g P*9h 9g9h P*9i 9h8i 7h7i 8i7i 7g7h 7i6i 7h7i 6i5i 7i6i 5i4i 6i5i 4i3i 5i4i 3i2i 1i1h 2i1h 4e4d 1f2g P*1i 1h1g 1i1h 1g1f 1h1g 1f1e 1g1f 1e2d 4d3d 2d1c 3d2d 1c2b 2d2c 2b1a S*2b",
    "rules": []
  },
  "kapitan-21_29": {
    "seq": "4b4h 3d3h P*1i 1h1g 4h4g 2e3g+ 1i1h 1g1f 4g4f 3c3f 1h1g 1f1e 4f4e 2d2e 1g1f 1e1d 4e4d 2c3d 1f1e 1d1c 4d4c 2b2c 1e1d 1c2b 1d1c+ 2b1a 4c4a 1b2a L*1b",
    "rules": []
  },
  "jigsaw-03-a3_41": {
    "seq": "P*5i 5h5g 5i5h 5g4f P*4g 4f3g 3i3h 3g3f 3h3g 3f3e 3g3f 3e2d 2g2f P*2e 2f2e 2d3d 3f3e 3d4e 4g4f 4e5f 5h5g 5f6f P*6g 6f7g P*7h 7g8f 8i8h 8f7e N*8g 7e8f 8g7e 8f7f 7h7g 7f6e 6g6f 6e6d 7e6c+ 6d7d 6c6d 7d7e 6d6e",
    "rules": []
  },
  "ofm-139_5": {
    "seq": "N*2i 3i2i S*3h 3g4h G*4i",
    "rules": []
  },
  "shin-taizen-2-3_395": {
    "seq": "G*2h 3h4h 2h3h 4h5h 3h4h 5h6h 4h5h 6h7h 5h6h 7h8h 6h7h 8h9g 7h8h 9g9f P*9g 9h9g 8h9g 9f9e P*9f 8e9f 9g9f 9e9f G*9e @38 @33 2c8c 7d8d @28 @38 @23 8c7c 6d7d @24 @28 @38 7c6c 5d6d @35 @36 @21 6c5c 4d5d @22 @35 @36 5c4c 3d4d @37 @34 4c3c 2d3d @31 @37 @29 3c2c 1d2c 1g1f 2e2d 1f1e 2d3e 1e2e 3e4e @37 @19 P*1g 1h1g @20 @30 @31 @37 @16 2h1h 3h2h 1h1i 2h2i 1i2i G*1i 2i3h 1i2i 3h2g 2i3h 2g1h P*1i 1h2i 3h3i 2i1i G*2i",
    "rules": [
      "9f9g 9e9f",
      "@0 9g9h",
      "@1 9f9g",
      "@2 9h8h",
      "@3 9g9h",
      "@4 8h7h",
      "@5 9h8h",
      "@6 7h6h",
      "@7 8h7h",
      "@8 6h5h",
      "@9 7h6h",
      "@10 5h4h",
      "@11 6h5h",
      "@12 4h3h",
      "@13 5h4h",
      "@14 3h2h",
      "@15 4h3h",
      "@16 2h1g",
      "@17 3h2h",
      "@18 1g1f",
      "2h1g 1f2e",
      "2e3e 4e5e",
      "3e4e 5e6e",
      "4e5e 6e7e",
      "5e6e 7e8e",
      "6e7e 8e9e",
      "@25 7e8e",
      "@26 9e9f",
      "@27 8e9e",
      "@19 @20",
      "1g1f 2e3e",
      "1f2e 3e4e",
      "@21 @22",
      "@23 @24",
      "@29 @30",
      "@33 @28",
      "@34 @31",
      "@32 @35",
      "@36 @32"
    ]
  },
  "shin-taizen-2-4_241": {
    "seq": "9g1g P*1f 1g1f 1d2e @3 @45 @4 1f1h @46 @41 @12 8g9h 1g1h 8i8h P*9i 9h9g 1h1g 8h8g @42 8g7h P*9i @46 @45 1f1g 7h7g @42 7g6h P*9i @46 4g4h P*4i 4h3h 1g1h 3h4g 1h1g @45 1f1g 6h6g @42 6g5h P*9i 9h9g 1h1g 5h5g @42 5g4h P*9i @21 @5 4g5f G*5g",
    "rules": [
      "1f1e 2e3f",
      "@0 1e1f",
      "@1 3f4g",
      "@2 1f1g",
      "9i9h 9g9h",
      "P*3g 4i4h",
      "4g5f 1g1f",
      "@6 5f6g",
      "@7 1f1g",
      "@8 6g7f",
      "@9 1g1f",
      "@10 7f8g",
      "@11 1f1g",
      "9h8g 1h1g",
      "@13 8g7f",
      "@14 1g1f",
      "@15 7f6g",
      "@16 1f1g",
      "@17 6g5f",
      "@18 1g1f",
      "@19 5f4g",
      "@20 1f1g",
      "4g3h P*3i",
      "@22 3h4h",
      "@23 1g1h",
      "@24 P*3h",
      "@25 P*4i",
      "@26 4h3g",
      "@27 3i3h",
      "@28 3g4g",
      "@29 1h1g",
      "@30 @5",
      "@31 4g3f",
      "@32 3h3g",
      "@33 3f4f",
      "@34 1g1f",
      "@35 P*3f",
      "@36 4h4g",
      "@37 4f3e",
      "@38 3g3f",
      "@39 3e2e",
      "@40 @3",
      "@4 1g1h",
      "@12 8g8f",
      "@43 1g1f",
      "@44 8f9g",
      "@21 @41"
    ]
  },
  "wfp-56-12_2571": {
    "seq": "9d9c+ 8b9c P*9d 9c9d P*9e 9d8e P*8f 8e7f P*7g 7f6g P*6h 6g5f P*5g 5f5e 1d1e 5e4d 1e1d 4d3e @13 @15 @14 @25 6h7g @39 7g8f @47 @42 8h8g 8f9e P*9f 9e8e @48 @42 @47 @42 8h8g 8f9f P*9g 9f8e @48 @42 @47 @40 @36 @28 @14 @9 1d1e 3e4d 1h1g P*3e 1e1d @20 5g4h P*4i 4h4g 4i4h 4g3f @4 3e3f @7 @15 @9 1h1g @2 4e5f 5h5g @16 @21 6h7g 7i7h 7g8f 8h8g 8f9e 9g9f 9e9d 9f9e 9d9c 9e9d 9c8b 6a7b+ 8b7b 1e1b 6c6b 1g1h 7b8b 9d9c+ 8b9c P*9d 9c9d P*9e 9d8e @48 @37 @23 5f5e 5g5f 5e4d 1b1d 4d3e P*3f @2 S*3e 3f3e 4e4d S*4e",
    "rules": [
      "4e4d 1e1d",
      "@0 4d3e",
      "3e4e 1d1e",
      "P*3g 3f4e",
      "@3 1g1h",
      "1d1e @1",
      "@5 3g3f",
      "@6 @2",
      "P*3e 3f3e",
      "@8 @1",
      "1h1g 3e3f",
      "@10 @4",
      "@11 P*3f",
      "@12 @7",
      "@9 @13",
      "@14 @14",
      "5f5g P*5h",
      "4d5e 5g5f",
      "@17 5e5f",
      "@18 P*5g",
      "@19 @16",
      "5g6h P*6i",
      "6i6h 6g5f",
      "@22 5h5g",
      "@0 @20",
      "@24 @21",
      "@23 5f4e",
      "@26 1d1e",
      "@27 @15",
      "@28 @15",
      "@29 @25",
      "@30 6h7g",
      "P*7h 7g6g",
      "@32 @30",
      "@33 6h7h",
      "@34 P*7i",
      "@35 7h6g",
      "7h7g 7f6g",
      "@36 @31",
      "@38 7i7h",
      "@37 @31",
      "@40 @39",
      "@41 7g8f",
      "P*8g 8f7f",
      "@43 @41",
      "@44 7g8g",
      "@45 P*8h",
      "@46 8g7f",
      "8g8f 8e7f"
    ]
  },
  "wfp-56-12a_2683": {
    "seq": "9d9c+ 8b9c P*9d 9c9d P*9e 9d8e @42 @32 6i6h 6g5g 5i5h 5g5f 5h5g 5f5e 1c1e 5e4d 1e1d 4d3e @13 @15 @14 @25 6h7g @39 @48 @41 @49 8f9e P*9f 9e8e @42 @41 @48 @41 @49 8f9f P*9g 9f8e @42 @41 @48 @40 @37 @28 @14 @7 1d1e 3e4d 1h1g P*3e 1e1d @21 5g4h P*4i 4h4g 4i4h 4g3f @3 1g1h 3e3f @11 @15 @7 @9 1d1e 4e5f 5h5g @16 @18 6h7g 7i7h @49 8f9e 9g9f 9e9d 9f9e 9d9c 9e9d 9c8b 6b7b 8b7b 1e1b 6c6b 1g1h 7b8b 9d9c+ 8b9c P*9d 9c9d P*9e 9d8e @42 @32 @23 5f5e 5g5f 5e4d 1h1g 4d4e @10 1b1e @5 @15 @15 @7 1h1g @2 4e5f @17 @18 6h7g P*7h @43 8f9e P*9f 9e9d 9f9e 9d9c 9e9d 9c8b 9d9c+ 8b7b 1g1h 6b6c 1e1b 5a6b 9c8c 7b6a G*5b",
    "rules": [
      "4e4d 1e1d",
      "@0 4d3e",
      "3e4e 1d1e",
      "P*3g 3f4e",
      "@1 3g3f",
      "@4 @2",
      "P*3e 3f3e",
      "@6 @1",
      "1h1g 3e3f",
      "@8 @3",
      "1g1h P*3f",
      "1d1e @5",
      "@9 @10",
      "@12 @11",
      "@7 @13",
      "@14 @14",
      "5f5g P*5h",
      "P*5g @16",
      "5g6h P*6i",
      "4d5e 5g5f",
      "@19 5e5f",
      "@20 @17",
      "6i6h 6g5f",
      "@22 5h5g",
      "@0 @21",
      "@24 @18",
      "@23 5f4e",
      "@26 1d1e",
      "@27 @15",
      "@28 @15",
      "@29 @25",
      "@30 6h7g",
      "7h7g 7f6g",
      "P*7h 7g6g",
      "@33 @30",
      "@34 6h7h",
      "@35 P*7i",
      "@36 7h6g",
      "@37 @31",
      "@38 7i7h",
      "@32 @31",
      "@40 @39",
      "8g8f 8e7f",
      "7g8f P*8g",
      "@43 8f7f",
      "@44 @41",
      "@45 7g8g",
      "@46 P*8h",
      "@47 8g7f",
      "7g8f 8h8g"
    ]
  },
  "bataco_4247": {
    "seq": "1h1g 1f2g @37 @25 @7 8g9g 9i9h 9g9f @23 P*6h @62 2h1g @77 1e2e @78 2e2d @81 2d3d @78 3d3c @81 3c4c @78 4c4b @81 4b5b @73 1g1f 1e1d 1f1e 1d1e 6a5a @75 @65 @70 @63 @53 @26 @22 @12 8d9c 5b6b @21 6i6h @60 2h1g @72 3b4c @78 1e1d 5a4a 1d1e @79 @37 @25 @22 7d6b+ 9f9e G*9d",
    "rules": [
      "8d9c P*8d",
      "@0 9c9d",
      "@1 9e8e",
      "@2 9d8d",
      "@3 8e9e",
      "@4 @4",
      "7i7h 7g8g",
      "@6 8i8h",
      "9h9g 9f9e",
      "@8 @5",
      "@9 @5",
      "@10 @5",
      "@11 @4",
      "9g9f 9e9f",
      "@13 P*9g",
      "@14 9f9g",
      "@15 P*9h",
      "@16 9g8h",
      "@17 P*8i",
      "@18 8h7h",
      "@19 P*7i",
      "@20 7h6g",
      "@7 8g9f",
      "@12 @21",
      "@22 @23",
      "6h6g 6f7g",
      "P*6i 6h7g",
      "@26 @24",
      "@27 6i6h",
      "@25 @24",
      "@29 P*6h",
      "2i2h 2g3f",
      "@31 3h3g",
      "@32 3f4e",
      "@33 4g4f",
      "@34 4e5f",
      "@35 5h5g",
      "@36 5f6f",
      "6g5g P*5h",
      "@38 5g6h",
      "@39 @28",
      "@40 6g5f",
      "@41 5h5g",
      "@42 5f4f",
      "@43 P*4g",
      "@44 4f4g",
      "@45 P*4h",
      "@46 4g5g",
      "@47 P*5h",
      "@48 5g6f",
      "@49 @30",
      "@50 6g5h",
      "@51 P*5i",
      "@52 5h6h",
      "6g5g 5i5h",
      "@54 5g4f",
      "@55 4h4g",
      "@56 4f3g",
      "@57 P*3h",
      "@58 3g2h",
      "@59 P*2i",
      "@53 @28",
      "@61 @60",
      "@37 @30",
      "@63 @62",
      "@64 2h1g",
      "P*1h 1g2g",
      "@66 @64",
      "@67 2h1h",
      "@68 P*1i",
      "@69 1h2g",
      "1i1h 1g1f",
      "@71 5a6a",
      "1h1g 1f1e",
      "1e1f P*1g",
      "@74 1f2g",
      "@70 @65",
      "@76 @72",
      "@73 6a5a",
      "1g1f @75",
      "@79 @65",
      "@80 @77"
    ]
  },
  "hanran-39-05_327": {
    "seq": "@2 @21 P*5f 6e5f @23 P*6e 5d6e @28 P*7f 6e7f @27 @26 6d6e P*6f 5f6f 1d1e 6e6d @25 P*6e 7f6e @24 @21 P*5f 6e5f @23 1e1d 6d7e P*7f 8d7f @27 @28 1d1e 7e8d P*8e 7d8e 1e1d 8d7e @27 @28 1d1e 7e8d 1e1d 8d9e P*9f 8e9f 1d1e 9e8d 1e1d 8d7e @27 @24 @6 N*3e 3f3e 4e5e 3e3d 5e4d N*3f",
    "rules": [
      "4e4d 1e1d",
      "1d1e @0",
      "@1 4d5e",
      "1d1e 5e4d",
      "@3 1e1d",
      "@4 4d4e",
      "@5 1d1e",
      "@6 P*3e",
      "@7 3f3e",
      "@8 @0",
      "@9 4d3e",
      "@10 1h1g",
      "@11 3e3f",
      "@12 P*3g",
      "@13 3f4e",
      "@14 1g1h",
      "@15 P*3f",
      "@16 @1",
      "@17 4d3e",
      "@18 3g3f",
      "@19 3e4e",
      "@20 @2",
      "@21 1d1e",
      "@22 5e6d",
      "1e1d 6d5e",
      "@24 @23",
      "@25 1e1d",
      "1d1e 7e6d",
      "@26 6d7e"
    ]
  },
  "wfp-68-unpromote-5_233": {
    "seq": "4d4c @17 @17 @17 @17 @17 @9 4c4e+ 5d5c 4e5e 6d5d 5e6d 5c6d P*6e 6d7c P*7d 7c8c 7d7c+ 8d7c P*8d 8c9c 8d8c+ 9d8c P*9d 9c9d P*9e 9d9e P*9f 9e9f P*9g 9f9g P*9h 9g8h P*8i 8h7g P*7h 7g6h 4h5i 6h6g 5i6h 6g7h 6h7i 7h7g B*5i R*6h 7i6h 7g7h R*7i",
    "rules": [
      "3c2d 4c2c",
      "@0 2d1d",
      "@1 2c1c",
      "@2 1d2d",
      "@3 1c1d",
      "@4 2d3c",
      "@5 1d1c",
      "@6 3c4d",
      "@7 1c4c",
      "@8 4d5d",
      "@9 4c5c",
      "@10 5d4d",
      "@11 5c5d",
      "@12 4d3c",
      "@13 5d5c",
      "@14 P*4c",
      "@15 5c4c",
      "@16 @16"
    ]
  },
  "jugemu_gentei_kai_36607": {
    "seq": "P*8g 8f9g 9i9h @35 @16 6c7c 8c7c @0 @18 @83 8g7f P*7g 7f8f @31 @28 @83 8g7g @33 @28 @83 @37 P*6h 6g5g @68 4g3f @78 3f2e @90 2e1d P*1e 1d2d @91 @82 @90 2e1e P*1f 1e2d @91 @82 @90 2e1d 1f1e 1d1c 1e1d 1c2b 1d1c+ 2b1c P*1d 1c2d @91 @79 @69 @50 @33 @13 9d8c @2 N*7b 8b7b 7c6c 7b7c 6d7c N*5e 6c6b 7a5a+ 6b5a 5e4c 5a6a G*7b",
    "rules": [
      "6b7b 7c8c",
      "7b8b 8c7c",
      "@1 6a7a",
      "@2 7c6b",
      "@3 8b7b",
      "@4 6b6c",
      "@5 7a6a",
      "@6 P*6b",
      "@7 7b6b",
      "@8 6c7c",
      "@9 @0",
      "@10 @10",
      "9g9f 9e9d",
      "@12 9f9e",
      "@13 9d9c",
      "@14 9e9d",
      "@15 9c8c",
      "@11 @11",
      "@17 @11",
      "9d9c+ 8c9c",
      "@19 P*9d",
      "@20 9c9d",
      "@21 P*9e",
      "@22 9d9e",
      "@23 P*9f",
      "@24 9e9f",
      "@25 P*9g",
      "@26 9f8g",
      "@16 @18",
      "@28 @10",
      "@29 @27",
      "8h8g 8f9e",
      "P*7h 7g8f",
      "@32 @31",
      "9g9f 9h9g",
      "@34 9f9e",
      "8g7f 7h7g",
      "@36 7f6g",
      "@33 @30",
      "@38 P*8h",
      "@39 8g8h",
      "@40 P*8i",
      "@41 8h9g",
      "@42 P*9h",
      "@43 @35",
      "@44 @30",
      "@45 8i8h",
      "@46 @37",
      "5i5h 5g6f",
      "@48 6h6g",
      "@49 6f7g",
      "@50 @47",
      "@51 P*6h",
      "@52 6g6h",
      "@53 P*6i",
      "@54 6h7g",
      "@55 @47",
      "@56 6i6h",
      "@57 6g5h",
      "@58 P*5i",
      "@59 5h4g",
      "@60 P*4h",
      "@61 4g5g",
      "@62 @59",
      "@63 5h4h",
      "@64 P*4i",
      "@65 4h5g",
      "@66 @60",
      "@67 4i4h",
      "4h4g 4f5g",
      "@69 @68",
      "@70 4g3f",
      "P*3g 3f4f",
      "@72 @70",
      "@73 4g3g",
      "@74 P*3h",
      "@75 3g4f",
      "@76 @71",
      "@77 3h3g",
      "3g3f 3e4f",
      "@79 @71",
      "@80 @78",
      "@81 3f2e",
      "@27 P*8h",
      "P*2f 2e3e",
      "@84 @81",
      "@85 3f2f",
      "@86 P*2g",
      "@87 2f3e",
      "@88 @82",
      "@89 2g2f",
      "2f2e 2d3e"
    ]
  },
  "ogiekako-no-pawn-smoke-43": {
    "seq": "4f4h 5g4h 6f4h 3g4h R*3i 4i3i 5i4h 3i4h L*4i 4h5h L*5i 5h6i 8i7i 6i5i 7i6i 5i4i 6i5i 4i3i 5i4i 3i2i 4i3i 2i1i 1g1h 1i1h 2g1g 1h1g 2f2g 1g1h 3i2i 1h2g 2i1h 2g3g 1h2g 3g3h 2g3g 3h2h 3g2g 2h2g 4e3f 2g1f 3f2e 1f1e G*1f",
    "rules": []
  },
  "wfp-182-07_51": {
    "seq": "3e2e 4e2e 3f2e 1d2e R*1d 2e1d 3b3e 1d2e 3e2e 1e1d S*1e 1f1e 2e1e 1d1e S*1f 1e1f 2h1g 1f1e 1g1f 1e1f 3i1g 1f1g 3g2h 1g1f 2h1g 1f1g 4f2h 1g2h 4h3h 2h1h 3h2h 1h1i 2h2i 1i2i S*1h 2i2h 1h1g 2h2g 1g1f 2g2f 1f1e 2f2e 1e1d 2e2d 1d1c 2d1c 2b1b 1c1b N*2d 1b1a G*1b",
    "rules": []
  },
  "wfp-182-08_75": {
    "seq": "1g1h 1i1h 1f1g 1h1g 1e1f 2f1f 3f1f 2d1f P*1h 1g1h 5d2g 1h2g 2e2f 2g1g 2f2g 3g2g 6a1f 1g1h 1f2g 1h2i 2g1h 2i1h P*1i 1h1g 1i1h 1g2g N*1i 2g3g 5i4h 4g4h N*2i 3g4g 4i4h 4g4h L*4i 4h5h L*5i 5h6g 6i6h 6g6h P*6i 6h7i 9i8i 7i6i 8i7i 6i5i 7i6i 5i4i 6i5i 4i3i 5i4i 3i2h 4i3i 2h1h 3i2h 1h1i 2h1h 1i2i 1h1i 2i3h 1i2h 3h4g 2h3g 4g5f 3g4f 5f6e 6c6d 6e6d 4f5e 6d5c 5e6d 5c6b 6d7c 6b7a 7c7b",
    "rules": []
  },
  "mitochondria_3933": {
    "seq": "7d8d 9e8d @65 6i6h 6g5g 5i5h 5g5f @40 6h7g @51 7g8f @66 P*9f 9e8e @67 @62 8f9f P*9g 9f8e @67 @66 9g9f 9e9d 9f9e 9d9c 9a9b 8a9b 9e9d 9c9d @65 @41 @23 @7 G*1d @10 1c2b @25 G*4d 5c4b 2b3c",
    "rules": [
      "1c2b 3c3d",
      "@0 2b3c",
      "@1 3d2e",
      "@2 3c3d",
      "@3 2e1d",
      "@4 3d2e",
      "@5 1d1c",
      "@6 2e1e",
      "1e1d 1c2b",
      "@8 1d1c",
      "@9 2b3c",
      "@7 P*1d",
      "@11 @10",
      "@12 @12",
      "@13 @13",
      "5h5g 5f5e",
      "@15 5g5f",
      "@16 5e5d",
      "@17 5f5e",
      "@18 5d5c",
      "@19 5e5d",
      "@20 5c4b",
      "@21 3d3c+",
      "@22 4b3c",
      "3c4b 5d5c+",
      "@24 4b5c",
      "@23 @14",
      "@26 @14",
      "@27 P*3d",
      "@28 @25",
      "@29 P*5d",
      "@30 5c5d",
      "@31 P*5e",
      "@32 5d5e",
      "@33 P*5f",
      "@34 5e5f",
      "@35 P*5g",
      "@36 5f5g",
      "@37 P*5h",
      "@38 5g6h",
      "@39 P*6i",
      "6i6h 6g5f",
      "@41 @40",
      "@42 6h7g",
      "7h7g 7f6g",
      "P*7h 7g6g",
      "@45 @42",
      "@46 6h7h",
      "@47 P*7i",
      "@48 7h6g",
      "@49 @43",
      "@50 7i7h",
      "@44 @43",
      "@52 @51",
      "@53 7g8f",
      "8g8f 8e7f",
      "P*8g 8f7f",
      "@56 @53",
      "@57 7g8g",
      "@58 P*8h",
      "@59 8g7f",
      "@60 @54",
      "@61 8h8g",
      "P*9e 9d8e",
      "@63 @55",
      "@64 @44",
      "@62 8f9e",
      "@55 @54"
    ]
  },
  "bataco2_6557": {
    "seq": "5c6c 7c8c 6c7c 8c9d @10 @20 5i5h 5g4g 4i4h 4g4h P*4i 4h3g 3i3h 3g2g 2i2h 2g2f 2h2g 2f1e 1g1f 2d1f 4a5a 1e2e 2g2f 2e3f @54 @43 @27 @11 5a6a @3 @12 @4 @10 @38 @50 4h3g @61 3g2f @75 5b4b @76 5a6a 4b4c @77 @64 @75 4c3c @76 5a6a 3c3d @77 @64 @75 3b3c @76 @77 @54 @43 @27 @11 8c9c 9d8d 9c9d 8d7c 9d8c 7c7d 5a4a 6b6c 8c7c 7d6d 4a3a 5d5c 7c7d 6d5d 3a2a 5c4c R*5b",
    "rules": [
      "P*8c 9b9c",
      "@0 9d8d",
      "@1 9c8c",
      "@2 8d9d",
      "8c9b @3",
      "@4 @4",
      "9f9e 9d9e",
      "@6 P*9f",
      "@7 9e9f",
      "@8 P*9g",
      "@9 9f8g",
      "9g9f 9e9d",
      "@5 @5",
      "@11 @12",
      "@13 @5",
      "@14 @10",
      "8i8h 8g7g",
      "@16 7i7h",
      "@17 7g6f",
      "@18 6h6g",
      "@19 6f5g",
      "P*6h 6g7h",
      "@21 P*7i",
      "@22 7h7g",
      "@23 7i7h",
      "@24 7g8f",
      "@25 8h8g",
      "@26 8f9e",
      "P*8h 8g7h",
      "@28 P*7i",
      "@29 7h8h",
      "@30 P*8i",
      "@31 8h9g",
      "@32 P*9h",
      "@33 9g9f",
      "@34 9h9g",
      "@35 9f9e",
      "@36 @15",
      "@37 @20",
      "@27 @15",
      "@39 @38",
      "4i4h 4g5f",
      "@41 5h5g",
      "@42 5f6g",
      "P*5h 5g5h",
      "@44 P*5i",
      "@45 5h6g",
      "@46 @40",
      "@47 5i5h",
      "@48 5g4h",
      "@49 P*4i",
      "@43 @40",
      "@51 @50",
      "@52 4h3g",
      "3h3g 3f4g",
      "P*3h 3g4g",
      "@55 @52",
      "@56 4h3h",
      "@57 P*3i",
      "@58 3h4g",
      "@59 @53",
      "@60 3i3h",
      "@54 @53",
      "@62 @61",
      "@63 3g2f",
      "P*2g 2f3f",
      "@65 @63",
      "@66 3g2g",
      "@67 P*2h",
      "@68 2g3f",
      "@69 @64",
      "@70 2h2g",
      "@71 2f2e",
      "@72 2g2f",
      "@73 2e2d",
      "@74 6a5a",
      "2f2e 2d2e",
      "P*2f 2e3f"
    ]
  },
  "noroshi_113": {
    "seq": "7h6g 5g6g S*5g 6g5g 7g5g 4g5g 8g5g 4f5g G*6g 5f4g 6g5g 4g3g 5g4g 3f4g 6f4h 4g4h 7e4h 3g4h L*4i 4h5h L*5i 5h6h P*6i 6h6g 6i6h 6g6f 6d6e 6f6e 5c6d+ 6e6d 4d5d 6d6e 5d6d 6e6f 6h6g 6f6g P*6h 6g6h P*6i 6h7i 9i8i 7i6i 8i7i 6i5i 7i6i 5i4i 6i5i 4i3i 5i4i 3i2i 1h1i 2i1i 1g1h 1i2i 4i3i 2i1h 3i2i 1h1g 2i1h 1g1f 1h1g 1f1g P*1h 1g2h P*2i 2h2g 2i2h 2g1f 1h1g 1f1e 1c1d 1e1d N*2f 2e2f 3h2f 1d1e 1g1f 1e2f 2h2g 2f2e 2g2f 2e1d 1f1e 1d1c 1e1d 2d1d 1a1b 1c2d 2f2e 2d3d P*3e 3d4e P*4f 4e4d 4f4e 4d5e 6d5d 5e4e 5d4d 4e3e 4d3d 3e2e 3d2d 2e1e 2d1d 1e1d 1b1c 1d1c P*1d 1c1b 1d1c+ 1b1a 1c2b",
    "rules": []
  },
  "banri_no_uma_10453": {
    "seq": "N*1h 2f2e 1c2d+ 2e1f 2d2e @4 @12 1f2f 3f4e 2f2g 4e4f 2g2h 4g3g P*4g @28 @35 @21 1g2g 3g4h 2g2f @45 5g4g @37 @38 4h5h P*5i 6h5i @58 6g7f @72 7f8e @85 P*9e 9d9c 9e9d 9c8b 6c7c 8b7c 6b6c 7c8d @86 @85 P*9e 9d8d @86 @83 8e9e P*9f 9e8d @86 @85 9f9e 9d9c 9e9d 9c8b 6c7c 5a7c 9d9c+ 8b9c P*9d 9c8d @84 @73 @61 @39 2f2e L*3f @25 @10 L*2f",
    "rules": [
      "2f3g 2h1g",
      "2g3f 2e1f",
      "1f1g 2e2f",
      "@2 1g2h",
      "@3 @0",
      "3g2h 1g1f",
      "@5 2h2g",
      "@6 1f2e",
      "@7 @1",
      "@8 3f2g",
      "@9 1f2e",
      "@10 2g1f",
      "@11 2e3f",
      "4f3f 2h2g",
      "@13 3f2e",
      "@14 @1",
      "@15 3f2e",
      "@16 @4",
      "@17 3g2f",
      "@18 1g2h",
      "@19 2f1g",
      "@20 2h3g",
      "P*4h 4g3g",
      "@22 2e2f",
      "@23 3g2h",
      "@24 @0",
      "@21 1g2f",
      "@26 3g4g",
      "@27 2f2e",
      "P*3f @25",
      "@29 @12",
      "@30 1f2g",
      "@31 3f4f",
      "@32 2g2h",
      "@33 P*3g",
      "@34 4h4g",
      "@35 @28",
      "2f2e @36",
      "4g4h 2e2f",
      "2e2f 4h4g",
      "2f2e 5h4h",
      "@40 @39",
      "@41 @37",
      "@42 @36",
      "@43 @38",
      "4h5g P*5h",
      "@44 @45",
      "@46 5g6g",
      "@47 P*6h",
      "@48 6g5h",
      "@49 @46",
      "@50 5g6h",
      "@51 P*6i",
      "@52 6h5h",
      "@53 @44",
      "@54 4h5h",
      "@55 2f2e",
      "@56 5h6g",
      "@57 6i6h",
      "6h6g 6f5g",
      "@59 P*5h",
      "@60 5g4h",
      "@61 2e2f",
      "@62 4h5h",
      "@63 @58",
      "@64 6g7f",
      "P*7g 7f6f",
      "@66 @64",
      "@67 6g7g",
      "@68 P*7h",
      "@69 7g6f",
      "@70 @65",
      "@71 7h7g",
      "7g7f 7e6f",
      "@73 @65",
      "@74 @72",
      "@75 7f8e",
      "P*8f 8e7e",
      "@77 @75",
      "@78 7f8f",
      "@79 P*8g",
      "@80 8f7e",
      "@81 @76",
      "@82 8g8f",
      "8f8e 8d7e",
      "@83 8e9d",
      "@84 @76"
    ]
  }
};

const ORDER_INDEX = new Map<string, number>(
  Object.keys(SOLUTIONS_BY_ID).map((id, idx) => [id, idx])
);

function loadOgiekakoProblems(): OgiekakoProblem[] {
  const req = (require as unknown as {
    context: (
      dir: string,
      useSubdirectories: boolean,
      regExp: RegExp
    ) => { keys: () => string[]; (key: string): string };
  }).context("../../rust/problems", false, /\.sfen$/);

  const problems: OgiekakoProblem[] = [];
  for (const key of req.keys()) {
    const file = key.replace(/^\.\//, "");
    if (file.endsWith(".ans.sfen")) continue;
    const id = file.replace(/\.sfen$/, "");
    const raw = req(key);
    const parsed = parseProblemSfen(id, file, raw);
    if (!OGIEKAKO_ALIASES.has(parsed.author)) continue;
    const sol = SOLUTIONS_BY_ID[id] ?? { seq: "", rules: [] };
    problems.push({
      ...parsed,
      solutionSeq: sol.seq,
      solutionRules: sol.rules,
    });
  }

  problems.sort((a, b) => {
    const oa = ORDER_INDEX.get(a.id) ?? 9999;
    const ob = ORDER_INDEX.get(b.id) ?? 9999;
    if (oa !== ob) return oa - ob;
    const cmpDate = a.publishedAt.localeCompare(b.publishedAt);
    if (cmpDate !== 0) return cmpDate;
    return a.id.localeCompare(b.id);
  });

  return problems;
}

export const OGIEKAKO_PROBLEMS: OgiekakoProblem[] = loadOgiekakoProblems();


function expandMoves(seq: string, rules: string[]): string {
  const memo = new Map<string, string>();
  const expandToken = (tok: string): string => {
    if (!tok.startsWith("@")) {
      return tok;
    }
    const cached = memo.get(tok);
    if (cached !== undefined) {
      return cached;
    }
    const idx = parseInt(tok.slice(1), 10);
    const [a, b] = rules[idx].split(" ");
    const res = expandToken(a) + " " + expandToken(b);
    memo.set(tok, res);
    return res;
  };
  return seq.split(" ").map(expandToken).join(" ");
}

function normalizeSfenKey(sfen: string): string {
  const parts = sfen.trim().split(/\s+/);
  return parts.slice(0, 3).join(" ");
}

const PROBLEM_BY_SFEN_KEY = new Map<string, OgiekakoProblem>(
  OGIEKAKO_PROBLEMS.map((p) => [normalizeSfenKey(p.sfen), p])
);

export function findOgiekakoProblemBySfen(sfen: string): OgiekakoProblem | undefined {
  return PROBLEM_BY_SFEN_KEY.get(normalizeSfenKey(sfen));
}

export function buildPrecomputedSolveResponse(
  problem: OgiekakoProblem
): SolveApiResponse | undefined {
  const movesSfen = expandMoves(problem.solutionSeq, problem.solutionRules);
  const kif = moves_to_kif(problem.sfen, movesSfen);
  if (!kif) {
    return undefined;
  }
  return {
    redundant: false,
    solutions: 1,
    kif,
    sfen: problem.sfen,
    fromWhite: false,
  };
}

export function getPrecomputedSolveResponseBySfen(
  sfen: string
): SolveApiResponse | undefined {
  const problem = findOgiekakoProblemBySfen(sfen);
  if (!problem) {
    return undefined;
  }
  return buildPrecomputedSolveResponse(problem);
}
