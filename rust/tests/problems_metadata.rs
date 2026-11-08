use std::collections::HashSet;
use std::fs;
use std::path::Path;

use fmrs::solver::{self, Algorithm};
use fmrs_core::sfen::{decode_position, encode_position};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct MetadataDoc {
    schema_version: String,
    author_aliases: std::collections::HashMap<String, Vec<String>>,
    problems: Vec<ProblemMetadata>,
}

#[derive(Debug, Deserialize)]
struct ProblemMetadata {
    id: String,
    file: String,
    sfen: String,
    title: Option<String>,
    author: String,
    author_id: String,
    source: String,
    published_at: String,
    moves: usize,
    rule: String,
    defender_hand: Option<String>,
    awards: Vec<String>,
    url: Option<String>,
    notes: Option<String>,
    fmrs_url: String,
}

#[test]
fn test_problems_metadata_and_sfen_files() {
    let problems_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("problems");
    let metadata_str = fs::read_to_string(problems_dir.join("metadata.json"))
        .expect("Failed to read metadata.json");
    let doc: MetadataDoc =
        serde_json::from_str(&metadata_str).expect("Failed to parse metadata.json");

    assert_eq!(doc.schema_version, "1.0");
    let ogiekako_aliases = doc
        .author_aliases
        .get("ogiekako")
        .expect("Missing ogiekako aliases");
    assert!(ogiekako_aliases.contains(&"ogiekako".to_string()));
    assert!(ogiekako_aliases.contains(&"荻江香木".to_string()));
    assert!(ogiekako_aliases.contains(&"荻絵香木".to_string()));

    let mut seen_ids = HashSet::new();
    let mut seen_files = HashSet::new();
    let mut ogiekako_count = 0;

    for p in &doc.problems {
        assert!(seen_ids.insert(p.id.clone()), "Duplicate id: {}", p.id);
        assert!(
            seen_files.insert(p.file.clone()),
            "Duplicate file: {}",
            p.file
        );
        assert!(!p.source.is_empty(), "Empty source for {}", p.id);
        assert!(
            !p.published_at.is_empty(),
            "Empty published_at for {}",
            p.id
        );
        assert!(
            p.moves > 0 && p.moves % 2 == 1,
            "Invalid moves for {}",
            p.id
        );
        assert!(
            p.rule == "協力詰" || p.rule == "ばか詰",
            "Unexpected rule {} for {}",
            p.rule,
            p.id
        );
        assert!(
            p.fmrs_url.starts_with("https://ogiekako.github.io/fmrs/"),
            "Invalid fmrs_url for {}",
            p.id
        );

        let file_path = problems_dir.join(&p.file);
        let raw_content = fs::read_to_string(&file_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", file_path.display(), e));

        // Ensure KIF-style header comments exist in the .sfen file
        assert!(
            raw_content.contains(&format!("# 作者: {}", p.author)),
            "Missing author header in {}",
            p.file
        );
        assert!(
            raw_content.contains(&format!("# 発表誌: {}", p.source)),
            "Missing source header in {}",
            p.file
        );
        assert!(
            raw_content.contains(&format!("# 手数: {}", p.moves)),
            "Missing moves header in {}",
            p.file
        );
        if let Some(title) = &p.title {
            assert!(
                raw_content.contains(&format!("# 作品名: {}", title)),
                "Missing title header in {}",
                p.file
            );
        }
        if let Some(defender_hand) = &p.defender_hand {
            assert!(
                raw_content.contains(&format!("（受方持駒：{}）", defender_hand)),
                "Missing defender_hand in {}",
                p.file
            );
        }
        if !p.awards.is_empty() {
            assert!(
                raw_content.contains("# 受賞: "),
                "Missing awards in {}",
                p.file
            );
        }
        if let Some(url) = &p.url {
            assert!(
                raw_content.contains(&format!("# 出典URL: {}", url)),
                "Missing url in {}",
                p.file
            );
        }
        if let Some(notes) = &p.notes {
            assert!(
                raw_content.contains(&format!("# 備考: {}", notes)),
                "Missing notes in {}",
                p.file
            );
        }

        // Verify decode_position works on both the raw .sfen file (with comments) and the sfen string
        let pos_from_file = decode_position(&raw_content)
            .unwrap_or_else(|e| panic!("Failed to decode file {}: {}", p.file, e));
        let pos_from_meta = decode_position(&p.sfen)
            .unwrap_or_else(|e| panic!("Failed to decode metadata sfen for {}: {}", p.id, e));
        assert_eq!(
            encode_position(&pos_from_file),
            encode_position(&pos_from_meta),
            "Position mismatch between file and metadata for {}",
            p.id
        );

        if p.author_id == "ogiekako" {
            ogiekako_count += 1;
            assert!(
                ogiekako_aliases.contains(&p.author),
                "Author {} not in ogiekako_aliases for {}",
                p.author,
                p.id
            );
        }
    }

    assert_eq!(ogiekako_count, 33, "Expected 33 ogiekako problems");

    // Verify all .sfen files in rust/problems/ (except .ans.sfen) are present in metadata.json
    for entry in fs::read_dir(&problems_dir).expect("Failed to read problems directory") {
        let entry = entry.expect("Failed to read dir entry");
        let path = entry.path();
        if path.is_file() {
            let file_name = path.file_name().unwrap().to_str().unwrap();
            if file_name.ends_with(".sfen") && !file_name.ends_with(".ans.sfen") {
                assert!(
                    seen_files.contains(file_name),
                    "Unregistered .sfen file in problems/: {}",
                    file_name
                );
            }
        }
    }
}

#[test]
fn test_solve_ogiekako_short_problems_parallel() {
    let problems_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("problems");
    let metadata_str = fs::read_to_string(problems_dir.join("metadata.json"))
        .expect("Failed to read metadata.json");
    let doc: MetadataDoc =
        serde_json::from_str(&metadata_str).expect("Failed to parse metadata.json");

    // Solve short problems (<= 11 steps, excluding ofm-139_5 which explores 43M nodes)
    // using Parallel solver during `cargo t` to verify unique solutions quickly even in debug builds.
    for p in doc
        .problems
        .iter()
        .filter(|p| p.author_id == "ogiekako" && p.moves <= 11 && p.id != "ofm-139_5")
    {
        let raw_content = fs::read_to_string(problems_dir.join(&p.file)).unwrap();
        let position = decode_position(&raw_content).unwrap();
        let solutions = solver::solve(position, Some(2), Algorithm::Parallel, None)
            .unwrap_or_else(|e| panic!("Solver error on {}: {}", p.id, e));
        assert_eq!(
            solutions.len(),
            1,
            "Expected unique solution for {}, got {} solutions",
            p.id,
            solutions.len()
        );
        assert_eq!(
            solutions[0].len(),
            p.moves,
            "Expected {} moves for {}, got {}",
            p.moves,
            p.id,
            solutions[0].len()
        );
    }
}
