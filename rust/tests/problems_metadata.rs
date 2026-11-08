use std::fs;
use std::path::Path;

use fmrs::solver::{self, Algorithm};
use fmrs_core::sfen::{decode_position, encode_position, parse_problem_sfen};

const OGIEKAKO_ALIASES: &[&str] = &["ogiekako", "荻江香木", "荻絵香木"];

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

    let mut ogiekako_count = 0;
    let mut selected_count = 0;

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
        assert!(
            meta.rule == "協力詰" || meta.rule == "ばか詰",
            "Unexpected rule {} in {}",
            meta.rule,
            file_name
        );

        let pos_from_file = decode_position(&raw_content)
            .unwrap_or_else(|e| panic!("Failed to decode file {}: {}", file_name, e));
        assert_eq!(
            encode_position(&pos_from_file),
            encode_position(&pos_from_meta),
            "Position mismatch in {}",
            file_name
        );

        if OGIEKAKO_ALIASES.contains(&meta.author.as_str()) {
            ogiekako_count += 1;
            if meta.selected {
                selected_count += 1;
            }
        } else {
            assert!(
                !meta.selected,
                "Non-ogiekako problem {} should not be marked selected",
                file_name
            );
        }
    }

    assert_eq!(ogiekako_count, 34, "Expected 34 ogiekako problems");
    assert_eq!(selected_count, 29, "Expected 29 selected ogiekako problems");
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
