//! An offline policy lab for a graded stall rescue ladder. It reads recorded traces of agent work, detects looping and fixation stalls, and replays scripted scenarios against the ladder, recording which rung clears the stall.

pub mod detect;
pub mod judgment;
pub mod ladder;
pub mod pyjson;
pub mod sandbox;
pub mod trace;

pub use detect::detect;
pub use judgment::{
    CONFIDENCE_THRESHOLD, FixturePlayer, JudgmentError, check_answer, trusted, validate_fixture,
};
pub use ladder::{LadderError, apply_rung, build_intervention, next_rung};
pub use sandbox::{Scenario, run_session, scenario_names, scenarios};
pub use trace::{Event, Trace, TraceError, load_trace};
