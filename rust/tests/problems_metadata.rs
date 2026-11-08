use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use fmrs::solver::{self, Algorithm};
use fmrs_core::converter::convert_to_kif;
use fmrs_core::piece::{Color, Kind, KINDS, NUM_HAND_KIND};
use fmrs_core::position::advance::advance::advance_aux;
use fmrs_core::position::{AdvanceOptions, Square};
use fmrs_core::sfen::{decode_moves, decode_position, encode_position, parse_problem_sfen};
use serde::Deserialize;

const OGIEKAKO_ALIASES: &[&str] = &["ogiekako", "荻江香木", "荻絵香木"];

#[derive(Debug, Deserialize)]
struct CompressedSolution {
    seq: String,
    rules: Vec<String>,
}

fn load_embedded_solutions_from_ts() -> BTreeMap<String, CompressedSolution> {
    let ts_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/src/ogiekako_problems.ts");
    let content = fs::read_to_string(&ts_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", ts_path.display(), e));

    let marker = "const SOLUTIONS_BY_ID: Record<string, { seq: string; rules: string[] }> = ";
    let start_idx = content
        .find(marker)
        .expect("Failed to find SOLUTIONS_BY_ID in ogiekako_problems.ts")
        + marker.len();
    let rest = &content[start_idx..];
    let end_idx = rest
        .find("\n};")
        .expect("Failed to find end of SOLUTIONS_BY_ID in ogiekako_problems.ts")
        + 2;
    let json_str = &rest[..end_idx];

    serde_json::from_str(json_str)
        .unwrap_or_else(|e| panic!("Failed to parse SOLUTIONS_BY_ID as JSON: {}", e))
}

fn expand_moves(seq: &str, rules: &[String]) -> String {
    fn expand_token(tok: &str, rules: &[String], memo: &mut HashMap<String, String>) -> String {
        if !tok.starts_with('@') {
            return tok.to_string();
        }
        if let Some(cached) = memo.get(tok) {
            return cached.clone();
        }
        let idx: usize = tok[1..]
            .parse()
            .unwrap_or_else(|e| panic!("Invalid rule token '{}': {}", tok, e));
        let rule = rules
            .get(idx)
            .unwrap_or_else(|| panic!("Rule index {} out of bounds (len={})", idx, rules.len()));
        let mut parts = rule.split_whitespace();
        let a = parts.next().expect("Empty rule");
        let b = parts.next().expect("Rule missing second token");
        assert!(
            parts.next().is_none(),
            "Rule '{}' has more than 2 tokens",
            rule
        );
        let res = format!(
            "{} {}",
            expand_token(a, rules, memo),
            expand_token(b, rules, memo)
        );
        memo.insert(tok.to_string(), res.clone());
        res
    }

    let mut memo = HashMap::new();
    seq.split_whitespace()
        .map(|tok| expand_token(tok, rules, &mut memo))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn test_problems_sfen_files_and_metadata() {
    let problems_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("problems");
    let mut sfen_files: Vec<_> = fs::read_dir(&problems_dir)
        .expect("Failed to read problems directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".sfen") && !n.ends_with(".ans.sfen"))
        })
        .collect();
    sfen_files.sort();

    for file_path in &sfen_files {
        let file_name = file_path.file_name().unwrap().to_str().unwrap();
        let raw_content = fs::read_to_string(file_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", file_path.display(), e));

        let (meta, pos_from_meta) = parse_problem_sfen(&raw_content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", file_name, e));

        assert!(!meta.source.is_empty(), "Empty source in {}", file_name);
        assert!(
            !meta.published_at.is_empty(),
            "Empty published_at in {}",
            file_name
        );
        assert!(
            meta.moves > 0 && meta.moves % 2 == 1,
            "Invalid moves {} in {}",
            meta.moves,
            file_name
        );
        assert_eq!(
            meta.rule, "協力詰",
            "Unexpected rule {} in {}",
            meta.rule, file_name
        );

        let pos_from_file = decode_position(&raw_content)
            .unwrap_or_else(|e| panic!("Failed to decode file {}: {}", file_name, e));
        assert_eq!(
            encode_position(&pos_from_file),
            encode_position(&pos_from_meta),
            "Position mismatch in {}",
            file_name
        );

        let mut non_king_count = 0usize;
        for sq in Square::iter() {
            if let Some((_, k)) = pos_from_meta.get(sq) {
                if k != Kind::King {
                    non_king_count += 1;
                }
            }
        }
        for c in [Color::BLACK, Color::WHITE] {
            for &k in &KINDS[..NUM_HAND_KIND] {
                non_king_count += pos_from_meta.hands().count(c, k);
            }
        }
        assert_eq!(
            meta.defender_hand.is_some(),
            non_king_count < 38,
            "Defender hand restriction mismatch in {}: defender_hand={:?}, non_king_count={}",
            file_name,
            meta.defender_hand,
            non_king_count
        );

        if !OGIEKAKO_ALIASES.contains(&meta.author.as_str()) {
            assert!(
                !meta.selected,
                "Non-ogiekako problem {} should not be marked selected",
                file_name
            );
        }
    }
}

#[test]
fn test_embedded_solutions_are_consistent_and_reach_checkmate() {
    let solutions_by_id = load_embedded_solutions_from_ts();
    let problems_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("problems");

    let mut ogiekako_ids = BTreeSet::new();
    let options = AdvanceOptions::default();

    for entry in fs::read_dir(&problems_dir).expect("Failed to read problems directory") {
        let path = entry.expect("Failed to read dir entry").path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".sfen") || file_name.ends_with(".ans.sfen") {
            continue;
        }
        let id = file_name.trim_end_matches(".sfen");
        let raw_content = fs::read_to_string(&path).unwrap();
        let (meta, initial_pos) = parse_problem_sfen(&raw_content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", file_name, e));

        if !OGIEKAKO_ALIASES.contains(&meta.author.as_str()) {
            continue;
        }
        ogiekako_ids.insert(id.to_string());

        let compressed = solutions_by_id.get(id).unwrap_or_else(|| {
            panic!(
                "Missing embedded solution for '{}' in ogiekako_problems.ts",
                id
            )
        });

        let moves_sfen = expand_moves(&compressed.seq, &compressed.rules);
        let movements = decode_moves(&moves_sfen)
            .unwrap_or_else(|e| panic!("Failed to decode moves for {}: {}", id, e));

        assert_eq!(
            movements.len(),
            meta.moves,
            "Move count mismatch for {}: embedded solution has {} moves, metadata says {}",
            id,
            movements.len(),
            meta.moves
        );

        let mut pos = initial_pos.clone();
        let mut legal_moves = Vec::new();
        for (step, mv) in movements.iter().enumerate() {
            legal_moves.clear();
            let is_mate = advance_aux(&mut pos, &options, &mut legal_moves).unwrap_or_else(|e| {
                panic!("advance_aux failed at step {} of {}: {:?}", step + 1, id, e)
            });
            assert!(
                !is_mate,
                "Position prematurely reached mate at step {} of {}",
                step + 1,
                id
            );
            assert!(
                legal_moves.contains(mv),
                "Illegal move {:?} at step {} (1-indexed) of {} (turn={:?}, sfen={})",
                mv,
                step + 1,
                id,
                pos.turn(),
                pos.sfen()
            );
            pos.do_move(mv);
        }

        assert_eq!(
            pos.turn(),
            Color::WHITE,
            "Final turn after {} moves in {} must be WHITE",
            meta.moves,
            id
        );
        legal_moves.clear();
        let is_legal_mate = advance_aux(&mut pos, &options, &mut legal_moves)
            .unwrap_or_else(|e| panic!("Final advance_aux failed for {}: {:?}", id, e));
        assert!(
            is_legal_mate && legal_moves.is_empty(),
            "Final position of {} after {} moves is not legal checkmate (is_legal_mate={}, remaining_legal_moves={}, sfen={})",
            id,
            meta.moves,
            is_legal_mate,
            legal_moves.len(),
            pos.sfen()
        );

        let mut kif_pos = initial_pos;
        let kif = convert_to_kif(&mut kif_pos, &[movements]);
        assert!(
            kif.contains("手数----指手---------消費時間--"),
            "Invalid KIF generated for {}",
            id
        );
    }

    let solution_ids: BTreeSet<String> = solutions_by_id.keys().cloned().collect();
    assert_eq!(
        ogiekako_ids, solution_ids,
        "Mismatch between ogiekako .sfen problem IDs and SOLUTIONS_BY_ID keys"
    );
}

#[test]
fn test_solve_ogiekako_short_problems_parallel() {
    let problems_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("problems");
    for entry in fs::read_dir(&problems_dir).expect("Failed to read problems directory") {
        let path = entry.expect("Failed to read dir entry").path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".sfen")
            || file_name.ends_with(".ans.sfen")
            || file_name == "ofm-139_5.sfen"
        {
            continue;
        }
        let raw_content = fs::read_to_string(&path).unwrap();
        let (meta, position) = parse_problem_sfen(&raw_content).unwrap();
        if !OGIEKAKO_ALIASES.contains(&meta.author.as_str()) || meta.moves > 11 {
            continue;
        }
        let solutions = solver::solve(position, Some(2), Algorithm::Parallel, None)
            .unwrap_or_else(|e| panic!("Solver error on {}: {}", file_name, e));
        assert_eq!(
            solutions.len(),
            1,
            "Expected unique solution for {}, got {} solutions",
            file_name,
            solutions.len()
        );
        assert_eq!(
            solutions[0].len(),
            meta.moves,
            "Expected {} moves for {}, got {}",
            meta.moves,
            file_name,
            solutions[0].len()
        );
    }
}
