# Roadmap - westering

Westering is an offline policy lab for a graded stall rescue ladder, exercised over recorded traces and scripted scenarios in a local sandbox. The setup slice is landed and the contract stub stands at PR-S1 to PR-S5. Round 1 builds the stall detectors for looping and tool fixation plus the ladder's first two rungs, nudge and sharper prompt, exercised only against test agents in local sandboxes.

## Decision models in westering

Judgment enters after a detector fires, and the detectors stay deterministic.

Looping and tool fixation are pattern questions over a recorded trace, and the detectors answer them with rules and evidence spans. No model decides whether a stall happened. What rules cannot settle is what the stall means and how the session is likely to respond. Two judgment points follow from that. The first is severity. A score call can rate how deep a detected stall is, which informs how much of the ladder the session is likely to need. The second is recovery. After a rung fires in the sandbox, a choice call over recovering, still stalled, and getting worse can judge the post-intervention trace, which is the signal the ladder records about its own effect.

What the model decides stays inside the ladder's fixed shape. The escalation order is a rule, nudge before sharper prompt, and a model answer never reorders it, never skips a rung, and never fires one. The rungs themselves fire on the deterministic policy the contract sets, using the model's severity score as one input among the detector's facts. A model abstention or a confidence below the threshold set in the contract leaves the policy to run on detector facts alone, which is how it ran before the model existed.

What stays deterministic is the whole safety frame. Westering rescues automatically and cavalry records human rescues, and a decision model advises inside that line without blurring it. The last rung of the ladder belongs to arvin and is never reimplemented or invoked from here. Every exercise of the ladder happens against scripted test agents in a local sandbox. No code path touches a live fleet.

The candidate model is Typesafe's Jev, called through the program's Typesafe connector, with the OpenAI decision model as the comparison point. Both are evaluated against the same recorded fixtures before either is adopted at a decision point, and the deterministic core of this repo never depends on a provider being reachable.

Tests stay offline. Live answers are captured once, in a bounded recording session, as request and response fixture pairs built from synthetic traces, and the suite plays those fixtures back through a fixture player. No test calls the network. A schema test pins the fixture shape so a provider response that changes shape fails loudly in the recording session instead of silently in a later run.

The live-call budget for this repo is set as an envelope and owned by this arc. Trivial verification is capped at 5 calls. Fixture recording is capped at 150 calls in one designated session per round that needs new fixtures. A comparison evaluation is capped at 300 calls per model when a decision point is being adopted. Every call carries synthetic state only, every session is logged in the round record with its call count, and raising the envelope is a contract change recorded in this repo, never a silent overrun.
