//! Stall detectors over recorded traces.
//!
//! Two failure shapes are detected in Round 1. Looping is the same
//! call, or a short cycle of calls, repeating without progress.
//! Tool fixation is one tool called over and over with varying
//! calls while the other available tools go unused and the goal
//! does not advance.
//!
//! A stall window is a maximal run of consecutive events in which
//! no event reports progress. Detectors only ever look inside
//! stall windows, so a span containing a progress call is never
//! reported. Both detectors search subspans inside a window, and a
//! fixation detection reports the shortest qualifying subspan for
//! its tool rather than the whole window. A window yields one
//! fixation detection per dominant tool that holds a qualifying
//! span in it.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::pyjson::{Value, int_text_cmp};
use crate::trace::{Event, Trace};

pub const LOOP_MIN_RUN: usize = 3;
pub const CYCLE_MIN_REPEATS: usize = 2;
pub const CYCLE_MAX_LENGTH: usize = 4;
pub const FIXATION_MIN_DOMINANT_CALLS: usize = 4;
pub const FIXATION_MIN_SHARE_NUM: i64 = 3;
pub const FIXATION_MIN_SHARE_DEN: i64 = 4;

/// Maximal index ranges of consecutive events with no progress.
fn stall_windows(events: &[Event]) -> Vec<(usize, usize)> {
    let mut windows = Vec::new();
    let mut start: Option<usize> = None;
    for (idx, event) in events.iter().enumerate() {
        if event.progress {
            if let Some(open) = start.take() {
                windows.push((open, idx - 1));
            }
        } else if start.is_none() {
            start = Some(idx);
        }
    }
    if let Some(open) = start {
        windows.push((open, events.len() - 1));
    }
    windows
}

fn detection(
    trace: &Trace,
    shape: &str,
    pattern: Vec<Value>,
    first: usize,
    last: usize,
    detail: String,
) -> Value {
    Value::object(vec![
        ("trace_id".to_string(), Value::Str(trace.trace_id.clone())),
        ("shape".to_string(), Value::Str(shape.to_string())),
        ("pattern".to_string(), Value::List(pattern)),
        (
            "evidence_span".to_string(),
            Value::object(vec![
                (
                    "start_step".to_string(),
                    Value::Int(trace.events[first].step.clone()),
                ),
                (
                    "end_step".to_string(),
                    Value::Int(trace.events[last].step.clone()),
                ),
            ]),
        ),
        ("detail".to_string(), Value::Str(detail)),
    ])
}

fn first_loop_match(trace: &Trace, first: usize, last: usize) -> Option<(Value, usize)> {
    let calls: Vec<&str> = trace.events[first..=last]
        .iter()
        .map(|event| event.call.as_str())
        .collect();
    for offset in 0..calls.len() {
        for length in 1..=CYCLE_MAX_LENGTH {
            if offset + length > calls.len() {
                break;
            }
            let pattern = &calls[offset..offset + length];
            let needed = if length == 1 {
                LOOP_MIN_RUN
            } else {
                CYCLE_MIN_REPEATS
            };
            let mut repeats = 0usize;
            let mut pos = offset;
            while pos + length <= calls.len() && &calls[pos..pos + length] == pattern {
                repeats += 1;
                pos += length;
            }
            if repeats >= needed {
                let end = first + offset + repeats * length - 1;
                let detail = format!(
                    "call pattern of length {} repeated {} times with no progress",
                    length, repeats
                );
                let found = detection(
                    trace,
                    "looping",
                    pattern
                        .iter()
                        .map(|call| Value::Str(call.to_string()))
                        .collect(),
                    first + offset,
                    end,
                    detail,
                );
                return Some((found, end));
            }
        }
    }
    None
}

fn looping_in_window(trace: &Trace, first: usize, last: usize) -> Vec<Value> {
    let mut found = Vec::new();
    let mut start = first;
    while start <= last {
        match first_loop_match(trace, start, last) {
            Some((found_detection, end)) => {
                found.push(found_detection);
                start = end + 1;
            }
            None => break,
        }
    }
    found
}

/// Available tools that make no call inside the index range.
fn unused_available(
    trace: &Trace,
    positions_by_tool: &BTreeMap<String, Vec<usize>>,
    start: usize,
    end: usize,
) -> Vec<String> {
    let mut unused = Vec::new();
    for tool in &trace.tools_available {
        match positions_by_tool.get(tool) {
            None => unused.push(tool.clone()),
            Some(positions) => {
                let idx = positions.partition_point(|position| *position < start);
                if idx >= positions.len() || positions[idx] > end {
                    unused.push(tool.clone());
                }
            }
        }
    }
    unused
}

/// The shortest span in the window where the tool fixates, or
/// None. A qualifying span holds at least
/// FIXATION_MIN_DOMINANT_CALLS calls to the tool, the tool makes
/// at least three quarters of the calls in the span, the calls to
/// the tool are not all identical, and at least one available
/// tool goes unused in the span. The shortest qualifying span
/// starts and ends on a call to the tool, so the search walks the
/// tool's own call positions inside the window. Returns (length,
/// start, end, dominant_calls, unused).
fn fixation_span_for_tool(
    trace: &Trace,
    positions_by_tool: &BTreeMap<String, Vec<usize>>,
    tool: &str,
) -> Option<(usize, usize, usize, usize, Vec<String>)> {
    let positions = positions_by_tool.get(tool)?;
    if positions.len() < FIXATION_MIN_DOMINANT_CALLS {
        return None;
    }
    let events = &trace.events;
    let calls: Vec<&str> = positions
        .iter()
        .map(|idx| events[*idx].call.as_str())
        .collect();
    let total = positions.len();
    // differs_at[i] is the first occurrence after i whose call
    // differs from the call at occurrence i, or None when there
    // is none.
    let mut differs_at: Vec<Option<usize>> = vec![None; total];
    for i in (0..total.saturating_sub(1)).rev() {
        if calls[i + 1] != calls[i] {
            differs_at[i] = Some(i + 1);
        } else {
            differs_at[i] = differs_at[i + 1];
        }
    }
    let mut best: Option<(usize, usize, usize, usize, Vec<String>)> = None;
    for i in 0..=(total - FIXATION_MIN_DOMINANT_CALLS) {
        let mut j = i + FIXATION_MIN_DOMINANT_CALLS - 1;
        while j < total {
            let start = positions[i];
            let end = positions[j];
            let length = end - start + 1;
            if let Some(current) = &best
                && length >= current.0
            {
                break;
            }
            let count = j - i + 1;
            let deficit =
                length as i64 * FIXATION_MIN_SHARE_NUM - count as i64 * FIXATION_MIN_SHARE_DEN;
            if deficit > 0 {
                // One more call to this tool can cut the deficit
                // by at most FIXATION_MIN_SHARE_DEN minus
                // FIXATION_MIN_SHARE_NUM, which is one, so no
                // occurrence before j plus the deficit can reach
                // the share threshold from this start.
                j += deficit as usize;
                continue;
            }
            match differs_at[i] {
                None => break,
                Some(differs) if differs > j => {
                    j = differs;
                    continue;
                }
                _ => {}
            }
            let unused = unused_available(trace, positions_by_tool, start, end);
            if unused.is_empty() {
                // A longer span from this start only uses more
                // tools.
                break;
            }
            best = Some((length, start, end, count, unused));
            break;
        }
    }
    best
}

/// One fixation detection per dominant tool with a qualifying
/// span. Detections are ordered by the start step of their
/// evidence spans, with the tool name breaking a tie between
/// spans that start at the same step.
fn fixation_in_window(trace: &Trace, first: usize, last: usize) -> Vec<Value> {
    let mut positions_by_tool: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (idx, event) in trace.events.iter().enumerate().take(last + 1).skip(first) {
        positions_by_tool
            .entry(event.tool.clone())
            .or_default()
            .push(idx);
    }
    let mut found: Vec<(String, String, Value)> = Vec::new();
    for tool in positions_by_tool.keys() {
        let Some((length, start, end, count, unused)) =
            fixation_span_for_tool(trace, &positions_by_tool, tool)
        else {
            continue;
        };
        let detail = format!(
            "tool {} made {} of {} calls with no progress while {} went unused",
            tool,
            count,
            length,
            unused.join(", ")
        );
        let found_detection = detection(
            trace,
            "tool_fixation",
            vec![Value::Str(tool.clone())],
            start,
            end,
            detail,
        );
        found.push((
            trace.events[start].step.clone(),
            tool.clone(),
            found_detection,
        ));
    }
    found.sort_by(|a, b| int_text_cmp(&a.0, &b.0).then_with(|| a.1.cmp(&b.1)));
    found
        .into_iter()
        .map(|(_, _, found_detection)| found_detection)
        .collect()
}

/// Every detection in the trace at or after from_step, in span
/// order.
pub fn detect(trace: &Trace, from_step: i64) -> Vec<Value> {
    let from = from_step.to_string();
    let events: Vec<Event> = trace
        .events
        .iter()
        .filter(|event| int_text_cmp(&event.step, &from) != Ordering::Less)
        .cloned()
        .collect();
    let view = Trace {
        trace_id: trace.trace_id.clone(),
        goal: trace.goal.clone(),
        tools_available: trace.tools_available.clone(),
        events,
    };
    let mut found: Vec<Value> = Vec::new();
    for (first, last) in stall_windows(&view.events) {
        found.extend(looping_in_window(&view, first, last));
        found.extend(fixation_in_window(&view, first, last));
    }
    found.sort_by(|a, b| {
        int_text_cmp(span_start(a), span_start(b)).then_with(|| shape_of(a).cmp(shape_of(b)))
    });
    found
}

fn span_start(detection: &Value) -> &str {
    match detection
        .get("evidence_span")
        .and_then(|span| span.get("start_step"))
    {
        Some(Value::Int(text)) => text,
        _ => "",
    }
}

fn shape_of(detection: &Value) -> &str {
    match detection.get("shape") {
        Some(Value::Str(text)) => text,
        _ => "",
    }
}
