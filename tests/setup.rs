//! The setup invariants, re-expressed as Rust tests under
//! PR-42.

mod common;

use common::root;

const CANONICAL_PREFIX_SHA256: &str =
    "63d2f04005dc8eb111d3efca1be55c0911f9ad46bc7cf3c74d9f9908f4a86f11";
const EXPECTED_PREFIX: &str = "PR";
const TEXT_SUFFIXES: [&str; 11] = [
    "md", "yml", "yaml", "py", "toml", "txt", "json", "jsonl", "rs", "sh", "lock",
];
const SKIP_PARTS: [&str; 8] = [
    ".git",
    "target",
    ".pytest_cache",
    "__pycache__",
    ".venv",
    "venv",
    "node_modules",
    ".tox",
];

#[test]
fn contract_stub_obligations_exist() {
    let text = std::fs::read_to_string(root().join("CONTRACT.md")).unwrap();
    let mut found: Vec<(u32, String)> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix(EXPECTED_PREFIX) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix("-S") else {
            continue;
        };
        let Some((number, _)) = rest.split_once('.') else {
            continue;
        };
        if number.len() == 1
            && let Ok(number) = number.parse::<u32>()
        {
            found.push((number, line.to_string()));
        }
    }
    assert!(
        found.len() >= 5,
        "expected at least 5 stub obligations, found {}",
        found.len()
    );
    let mut numbers: Vec<u32> = found.iter().map(|(number, _)| *number).collect();
    numbers.sort_unstable();
    assert_eq!(
        numbers,
        vec![1, 2, 3, 4, 5],
        "stub obligations must be exactly S1 to S5"
    );
    for (_, line) in &found {
        let words = line.split_whitespace().count();
        assert!(words >= 20, "obligation too thin at {} words", words);
    }
}

#[test]
fn agents_canonical_block_present() {
    let text = std::fs::read_to_string(root().join("AGENTS.md")).unwrap();
    assert!(text.contains("repo annex below this line"));
    let prefix = text.split("## Repo annex").next().unwrap();
    let digest = sha256_hex(prefix.as_bytes());
    assert_eq!(
        digest, CANONICAL_PREFIX_SHA256,
        "canonical block above the annex line has drifted"
    );
    let annex = text.split("## Repo annex").nth(1).unwrap();
    assert!(annex.contains("CONTRACT.md"));
}

#[test]
fn readme_status_reports_rounds() {
    let text = std::fs::read_to_string(root().join("README.md")).unwrap();
    // PR-42. The closed round count is a claim the Status
    // section makes, so the pin counts it there and nowhere
    // else. The phrase standing in another section does not
    // satisfy the pin, and a README with no Status section
    // fails it.
    let counts = closed_round_counts(&status_section(&text));
    assert_eq!(
        counts,
        vec!["17".to_string()],
        "README status must report exactly one closed round count"
    );
}

#[test]
fn the_round_count_is_read_in_the_status_section_only() {
    let outside = "## What this is\n\nClosed rounds 9 of 100.\n\n## Status\n\nNo count recorded.\n\n## Licence\n\nMIT.\n";
    assert!(closed_round_counts(&status_section(outside)).is_empty());
    let inside = "## Status\n\nClosed rounds 9 of 100.\n\n## Licence\n\nMIT.\n";
    assert_eq!(
        closed_round_counts(&status_section(inside)),
        vec!["9".to_string()]
    );
    let missing = "## What this is\n\nClosed rounds 9 of 100.\n";
    assert!(closed_round_counts(&status_section(missing)).is_empty());
}

/// The text of the Status section, from its heading to the
/// next heading of the same level or the end of the text. A
/// text with no Status heading yields an empty section.
fn status_section(text: &str) -> String {
    let mut in_section = false;
    let mut section = String::new();
    for line in text.lines() {
        if line.starts_with("## ") {
            if line == "## Status" {
                in_section = true;
                continue;
            }
            if in_section {
                break;
            }
        }
        if in_section {
            section.push_str(line);
            section.push('\n');
        }
    }
    section
}

/// Every match of the pinned form, a closed rounds count whose
/// number is not followed by another digit.
fn closed_round_counts(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let marker = "Closed rounds ";
    let mut search = text;
    while let Some(at) = search.find(marker) {
        let after = &search[at + marker.len()..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rest = &after[digits.len()..];
        if !digits.is_empty() && rest.starts_with(" of 100") {
            let tail = &rest[" of 100".len()..];
            if !tail.starts_with(|c: char| c.is_ascii_digit()) {
                found.push(digits);
            }
        }
        search = &search[at + marker.len()..];
    }
    found
}

#[test]
fn no_em_or_en_dash_in_text_files() {
    let em = "\u{2014}".as_bytes();
    let en = "\u{2013}".as_bytes();
    let mut bad = Vec::new();
    let mut stack = vec![root()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root()).unwrap().to_path_buf();
            if relative
                .components()
                .any(|part| SKIP_PARTS.contains(&part.as_os_str().to_str().unwrap_or("")))
            {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let suffix = path
                .extension()
                .map(|s| s.to_str().unwrap_or(""))
                .unwrap_or("");
            // PR-42 covers the text files of the tree, so the
            // scan takes every file whose suffix names a text
            // form and every file with no suffix at all whose
            // bytes decode as UTF-8. A file with no suffix whose
            // bytes do not decode is not a text file and stands
            // outside the scan, as does any file whose suffix
            // names no text form.
            let bytes = std::fs::read(&path).unwrap();
            let is_text = TEXT_SUFFIXES.contains(&suffix)
                || (suffix.is_empty() && std::str::from_utf8(&bytes).is_ok());
            if !is_text {
                continue;
            }
            if contains_bytes(&bytes, em) || contains_bytes(&bytes, en) {
                bad.push(relative);
            }
        }
    }
    assert!(bad.is_empty(), "em or en dash found in {:?}", bad);
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// SHA-256 from the sha2 crate, the adopted tool for it. The
/// AGENTS pin below is its own check, and the known vector test
/// pins the digest itself.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(data);
    digest.iter().map(|byte| format!("{:02x}", byte)).collect()
}

#[test]
fn the_standing_boundary_statements_name_arvin() {
    let roadmap = std::fs::read_to_string(root().join("ROADMAP.md")).unwrap();
    assert!(
        roadmap.contains("The last rung of the ladder belongs to arvin"),
        "the roadmap boundary statement must name arvin"
    );
    let ladder = std::fs::read_to_string(root().join("src/ladder.rs")).unwrap();
    assert!(
        ladder.contains("kill belongs to arvin"),
        "the ladder boundary statement must name arvin"
    );
    let agents = std::fs::read_to_string(root().join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("Arvin, owned by another chat, is the last rung"),
        "the annex boundary statement must name arvin"
    );
    let readme = std::fs::read_to_string(root().join("README.md")).unwrap();
    assert!(
        readme.contains("that rung belongs to arvin"),
        "the README boundary statement must name arvin"
    );
    let contract = std::fs::read_to_string(root().join("CONTRACT.md")).unwrap();
    assert!(
        contract.contains("belongs to arvin alone"),
        "the PR-45 boundary statement must name arvin"
    );
    for (name, text) in [
        ("README.md", &readme),
        ("ROADMAP.md", &roadmap),
        ("src/ladder.rs", &ladder),
        ("AGENTS.md", &agents),
    ] {
        assert!(
            !text.to_lowercase().contains("deadman"),
            "{name} must not name the kill rung's owner deadman"
        );
    }
    let section = contract
        .find("## Round 7 obligations")
        .expect("the Round 7 section exists");
    assert!(
        !contract[..section].to_lowercase().contains("deadman"),
        "deadman may stand only inside the Round 7 amendment record"
    );
}

const DESCRIPTION: &str = "An offline policy lab for a graded stall rescue ladder. It reads recorded traces of agent work, detects looping and fixation stalls, and replays scripted scenarios against the ladder, recording which rung clears the stall.";

#[test]
fn the_description_stands_on_every_surface() {
    for path in [
        "README.md",
        "Cargo.toml",
        "src/lib.rs",
        "tests/cli_corpus.json",
    ] {
        let text = std::fs::read_to_string(root().join(path)).unwrap();
        assert!(
            text.contains(DESCRIPTION),
            "the description must stand in {path}"
        );
    }
}

#[test]
fn sha256_matches_a_known_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}
