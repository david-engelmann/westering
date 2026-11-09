//! PR-65 to PR-67. The fixture player key ceiling, on the
//! judge surface and the load surface.

mod common;

use std::path::PathBuf;

use common::{assert_py_eq, temp_dir, value};
use westering::judgment::{FixtureLoadError, FixturePlayer};
use westering::pyjson::{self, Value};

/// The fixture document for a severity request whose evidence
/// span carries a chain of nested lists in a field the request
/// validation does not read, as text. The container chain of the
/// key document runs the root, the request object, and the span
/// object ahead of the lists, so a chain of k lists stands at a
/// key document path of 3 + k containers. A chain of 9988 lists
/// stands at 9991, the deepest path PR-40 carries, and a chain of
/// 9989 stands at 9992, the first path it refuses. The fixture
/// file's own deepest path is the same count, so both documents
/// stand under the reader ceiling and parse.
fn fixture_text(lists: usize) -> String {
    let mut text = String::new();
    text.push_str(
        "{\"synthetic\": true, \"judgment_point\": \"severity\", \
         \"request\": {\"trace_id\": \"deep-ceiling\", \"shape\": \"loop\", \
         \"pattern\": [], \"evidence_span\": {\"start_step\": 0, \
         \"end_step\": 0, \"nest\": ",
    );
    for _ in 0..lists {
        text.push('[');
    }
    for _ in 0..lists {
        text.push(']');
    }
    text.push_str(
        "}}, \"response\": {\"kind\": \"score\", \"score\": 0.5, \
         \"confidence\": 0.9}}",
    );
    text
}

fn write_deep_fixture(tag: &str, lists: usize) -> (PathBuf, String) {
    let dir = temp_dir(tag);
    let text = fixture_text(lists);
    std::fs::write(dir.join("deep.json"), &text).expect("fixture written");
    (dir, text)
}

fn request_of(text: &str) -> Value {
    let pair = pyjson::parse(text).expect("fixture text parses");
    pair.get("request").expect("request stands").clone()
}

/// Run work on a thread with the stack the binary itself uses,
/// because the reader and the key dump recurse once per
/// container, the same device the reader ceiling test uses.
fn on_ceiling_stack(work: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(work)
        .expect("thread spawns")
        .join()
        .expect("the ceiling work completes");
}

#[test]
fn player_answers_the_request_at_the_deepest_carried_path() {
    on_ceiling_stack(|| {
        let (dir, text) = write_deep_fixture("deep-carried", 9988);
        let request = request_of(&text);
        let player = FixturePlayer::new(&dir).expect("player loads");
        let answer = player
            .judge("severity", &request)
            .expect("an answer is recorded");
        assert_py_eq(
            &answer,
            &value(r#"{"kind": "score", "score": 0.5, "confidence": 0.9}"#),
        );
        std::fs::remove_dir_all(&dir).ok();
    });
}

#[test]
fn player_abstains_on_the_request_one_container_past_the_ceiling() {
    on_ceiling_stack(|| {
        let (dir, _) = write_deep_fixture("deep-abstention", 9988);
        let player = FixturePlayer::new(&dir).expect("player loads");
        let deeper = request_of(&fixture_text(9989));
        assert!(player.judge("severity", &deeper).is_none());
        std::fs::remove_dir_all(&dir).ok();
    });
}

#[test]
fn player_refuses_the_fixture_one_container_past_the_ceiling() {
    on_ceiling_stack(|| {
        let (dir, _) = write_deep_fixture("deep-refused", 9989);
        let err = FixturePlayer::new(&dir).expect_err("the player refuses");
        assert!(
            matches!(err, FixtureLoadError::TooDeep(_)),
            "the refusal is the nested too deeply kind, got {}",
            err
        );
        assert!(
            err.to_string().contains("deep.json"),
            "the refusal names the fixture, got {}",
            err
        );
        std::fs::remove_dir_all(&dir).ok();
    });
}
