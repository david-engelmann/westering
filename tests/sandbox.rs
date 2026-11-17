//! PR-7 and PR-9. The sandbox harness and the escalation
//! outcomes.

mod common;

use std::cell::RefCell;

use common::{assert_py_eq, int_of, list, text_of};
use westering::judgment::JudgmentSource;
use westering::pyjson::Value;
use westering::sandbox::run_session;

fn session(name: &str) -> Value {
    run_session(name, None).expect("session runs")
}

fn rungs_of(record: &Value) -> Vec<String> {
    list(record, "interventions")
        .iter()
        .map(|item| text_of(item, "rung").to_string())
        .collect()
}

#[test]
fn session_record_is_one_timeline_with_steps_on_interventions() {
    let record = session("loop-needs-sharper");
    assert_eq!(text_of(&record, "session_id"), "sandbox-loop-needs-sharper");
    let events = list(&record, "events");
    let steps: Vec<i64> = events.iter().map(|event| int_of(event, "step")).collect();
    let expected: Vec<i64> = (0..events.len() as i64).collect();
    assert_eq!(steps, expected);
    for intervention in list(&record, "interventions") {
        let at_step = int_of(intervention, "at_step");
        assert!(0 <= at_step && (at_step as usize) < events.len());
    }
    let first = &list(&record, "interventions")[0];
    let at_step = int_of(first, "at_step");
    let before: Vec<&Value> = events
        .iter()
        .filter(|event| int_of(event, "step") <= at_step)
        .collect();
    let after: Vec<&Value> = events
        .iter()
        .filter(|event| int_of(event, "step") > at_step)
        .collect();
    assert!(!before.is_empty() && !after.is_empty());
    assert!(
        before
            .iter()
            .all(|event| matches!(event.get("progress"), Some(Value::Bool(false))))
    );
}

#[test]
fn stall_that_clears_after_nudge_never_reaches_the_second_rung() {
    let record = session("loop-clears-on-nudge");
    assert_eq!(record.get("stall_cleared"), Some(&Value::Bool(true)));
    assert_eq!(text_of(&record, "outcome"), "cleared");
    assert_eq!(rungs_of(&record), vec!["nudge".to_string()]);
    assert_eq!(text_of(&record, "cleared_by"), "nudge");
}

#[test]
fn stall_that_survives_nudge_escalates_to_sharper_prompt() {
    let record = session("loop-needs-sharper");
    assert_eq!(record.get("stall_cleared"), Some(&Value::Bool(true)));
    assert_eq!(
        rungs_of(&record),
        vec!["nudge".to_string(), "sharper_prompt".to_string()]
    );
    assert_eq!(text_of(&record, "cleared_by"), "sharper_prompt");
}

#[test]
fn fixation_scenario_is_rescued_by_nudge() {
    let record = session("fixation-clears-on-nudge");
    assert_eq!(record.get("stall_cleared"), Some(&Value::Bool(true)));
    let detections = list(&record, "detections");
    assert_eq!(text_of(&detections[0], "shape"), "tool_fixation");
    assert_eq!(rungs_of(&record), vec!["nudge".to_string()]);
}

// Round 4 alteration, carried from the Python suite. The pin
// asserted the ignoring agent ends with the two Round 1 rungs on
// its timeline. PR-32 extends the same outcome over the third
// rung the ladder gained, so the timeline now records all three
// rungs in order. The claim the pin carries, that an agent
// ignoring every rung ends stalled with every rung tried, is
// unchanged.
// Round 6 alteration. PR-46 extends the same outcome over the
// fourth rung the ladder gains, so the timeline now records all
// four rungs in order. The claim the pin carries is unchanged.
#[test]
fn agent_that_ignores_every_rung_ends_stalled_with_all_recorded() {
    let record = session("loop-never-clears");
    assert_eq!(record.get("stall_cleared"), Some(&Value::Bool(false)));
    assert_eq!(text_of(&record, "outcome"), "stalled");
    assert_eq!(
        rungs_of(&record),
        vec![
            "nudge".to_string(),
            "sharper_prompt".to_string(),
            "smaller_subtask".to_string(),
            "fork".to_string()
        ]
    );
}

/// A judgment source that records the point of every call in
/// order and abstains on all of them, so a test can read the
/// order the harness asked in off the log it leaves behind.
struct CallLog {
    calls: RefCell<Vec<String>>,
}

impl JudgmentSource for CallLog {
    fn judge(&self, point: &str, _request: &Value) -> Option<Value> {
        self.calls.borrow_mut().push(point.to_string());
        None
    }
}

// The call order pin for PR-151. Inside one session step the
// recovery of the pending record is assessed ahead of the
// severity ask for the fresh detection. The corpus sources
// answer by point name, so no corpus case can see the order.
// This source answers nothing and only remembers the order it
// was asked in. In loop-never-clears all four rungs fire in
// turn, so the log reads severity for the opening rung, then
// recovery ahead of severity at each later rung step, and a
// closing recovery for the fork record once no rung stands.
#[test]
fn recovery_is_assessed_ahead_of_severity_inside_one_step() {
    let log = CallLog {
        calls: RefCell::new(Vec::new()),
    };
    let record = run_session("loop-never-clears", Some(&log)).expect("session runs");
    assert_eq!(
        rungs_of(&record),
        vec![
            "nudge".to_string(),
            "sharper_prompt".to_string(),
            "smaller_subtask".to_string(),
            "fork".to_string()
        ]
    );
    let calls = log.calls.borrow().clone();
    assert_eq!(
        calls,
        vec![
            "severity".to_string(),
            "recovery".to_string(),
            "severity".to_string(),
            "recovery".to_string(),
            "severity".to_string(),
            "recovery".to_string(),
            "severity".to_string(),
            "recovery".to_string()
        ]
    );
}

#[test]
fn intervention_records_name_the_rung_and_the_firing_detection() {
    let record = session("loop-needs-sharper");
    let interventions = list(&record, "interventions");
    let detections = list(&record, "detections");
    assert_eq!(interventions.len(), detections.len());
    for (intervention, detection) in interventions.iter().zip(detections.iter()) {
        let fired_by = intervention.get("fired_by").expect("fired_by stands");
        assert_eq!(text_of(fired_by, "shape"), text_of(detection, "shape"));
        assert_py_eq(
            fired_by.get("pattern").expect("pattern stands"),
            detection.get("pattern").expect("pattern stands"),
        );
        assert_py_eq(
            fired_by.get("evidence_span").expect("span stands"),
            detection.get("evidence_span").expect("span stands"),
        );
        assert_eq!(
            text_of(fired_by, "trace_id"),
            text_of(&record, "session_id")
        );
    }
}
