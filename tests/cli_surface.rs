//! The command line surface corpus.
//!
//! Every case in cli_corpus.json was classified by construction
//! against the Python command line of the Round 4 tree and the
//! port's binary together. A case whose exit code, streams, and
//! final files all match the Python tool byte for byte is marked
//! oracle and carries the Python expectation. A case whose exit
//! code and final files match the Python tool but whose stream
//! text differs is marked clap and carries the binary's text,
//! because help text and malformed invocation error text are
//! clap's presentation by the maintainer's tool choice ruling.
//! Nothing else can enter the corpus, since a case that differs
//! in code or files is a functional divergence and stops the
//! regeneration. The split is pinned below by count, so a case
//! cannot change sides without the suite failing. Each case runs
//! the binary in a fresh directory seeded with the committed
//! example trace and the committed fixtures, plus any files the
//! case itself seeds, and compares the exit code, both streams,
//! and the full final state of the directory.
//!
//! Round 6 regeneration. Thirteen cases were re-recorded
//! against the Round 6 binary and no case was added or removed
//! and no case changed sides, so the counts below stand.
//! Eleven cases carry the scenario enumeration in their help
//! or error text, and the enumeration gains the fork scenario
//! under PR-48. Two oracle cases run the loop-never-clears
//! session, whose record gains the fork intervention under
//! PR-46. Every other case stands byte for byte as the Round 5
//! tree left it.
//!
//! Round 9 addition. One case was added under PR-63,
//! intervene-help-short, firing the help short behind intervene
//! in the direct position, and no standing case changed its
//! expectation or its side, so the counts below move by that
//! one case alone.
//!
//! Round 12 addition. Eight oracle cases were added under
//! PR-74, firing the fork scenario through intervene behind
//! its states, plain and with the judgment seam's answer
//! families on the way to the fork, and no standing case
//! changed its expectation or its side, so the total and
//! oracle counts below move by those eight cases alone.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::{run_cli, temp_dir};
use westering::pyjson::{self, Value};

const EXPECTED_CASES: usize = 159;
const EXPECTED_ORACLE_CASES: usize = 105;
const EXPECTED_CLAP_CASES: usize = 54;

fn string(value: &Value) -> &str {
    value.as_str().expect("corpus text is a string")
}

fn object_entries(value: &Value) -> &Vec<(String, Value)> {
    match value {
        Value::Object(entries) => entries,
        _ => panic!("corpus value is an object"),
    }
}

fn write_seed(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("seed directory created");
    }
    std::fs::write(&path, text).expect("seed written");
}

fn snapshot(dir: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).expect("case dir readable") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                let rel = path
                    .strip_prefix(dir)
                    .expect("path stands under the case dir")
                    .to_string_lossy()
                    .replace('\\', "/");
                let text = std::fs::read_to_string(&path).expect("case file is UTF-8");
                files.insert(rel, text);
            }
        }
    }
    files
}

fn run_cases(source: &str, expected: usize) {
    let corpus = pyjson::parse(include_str!("cli_corpus.json")).expect("corpus parses");
    let setup = object_entries(corpus.get("setup").expect("setup present")).clone();
    let cases = corpus
        .get("cases")
        .expect("cases present")
        .as_list()
        .expect("cases list")
        .clone();
    assert_eq!(cases.len(), EXPECTED_CASES, "corpus case count");
    let selected: Vec<&Value> = cases
        .iter()
        .filter(|case| string(case.get("source").expect("source")) == source)
        .collect();
    assert_eq!(
        selected.len(),
        expected,
        "corpus {source} case count, a case changed sides"
    );
    for case in selected {
        let name = string(case.get("name").expect("name"));
        let dir = temp_dir("corpus");
        for (file, text) in &setup {
            write_seed(&dir, file, string(text));
        }
        if let Some(seed) = case.get("seed") {
            for (file, text) in object_entries(seed) {
                write_seed(&dir, file, string(text));
            }
        }
        let argv: Vec<String> = case
            .get("argv")
            .expect("argv")
            .as_list()
            .expect("argv list")
            .iter()
            .map(|item| string(item).to_string())
            .collect();
        let arg_refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        let out = run_cli(&arg_refs, &dir);
        let expect = case.get("expect").expect("expect");
        let code = match expect.get("code").expect("code") {
            Value::Int(text) => text.parse::<i32>().expect("code is a number"),
            _ => panic!("code is an integer"),
        };
        assert_eq!(out.code, code, "case {name}: exit code");
        assert_eq!(
            out.stdout,
            string(expect.get("stdout").expect("stdout")),
            "case {name}: stdout"
        );
        assert_eq!(
            out.stderr,
            string(expect.get("stderr").expect("stderr")),
            "case {name}: stderr"
        );
        let mut want: BTreeMap<String, String> = BTreeMap::new();
        for (file, text) in object_entries(expect.get("files").expect("files")) {
            want.insert(file.clone(), string(text).to_string());
        }
        assert_eq!(snapshot(&dir), want, "case {name}: final files");
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn oracle_cases_match_the_python_oracle_byte_for_byte() {
    run_cases("oracle", EXPECTED_ORACLE_CASES);
}

#[test]
fn clap_cases_match_the_oracle_on_code_and_files_and_carry_clap_text() {
    run_cases("clap", EXPECTED_CLAP_CASES);
}
