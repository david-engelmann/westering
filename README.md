# westering

An offline policy lab for a graded stall rescue ladder. It reads recorded traces of agent work, detects looping and fixation stalls, and replays scripted scenarios against the ladder, recording which rung clears the stall.

Westering is a command line tool that reads recorded traces of agent work and rescues stalls in a local sandbox. Its detectors catch two failure shapes, looping and tool fixation, and name the evidence span for each catch. A graded ladder then intervenes in a fixed order, a nudge, a sharper prompt, a smaller subtask, and a fork, and every rescue is recorded as one timeline that shows each rung tried and the rung that worked. Nothing here touches a live fleet.

[![CI](https://github.com/david-engelmann/westering/actions/workflows/ci.yml/badge.svg)](https://github.com/david-engelmann/westering/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

```console
$ ./target/debug/westering detect --trace examples/looping_trace.json
{
  "detections": [
    {
      "detail": "call pattern of length 1 repeated 4 times with no progress",
      "evidence_span": {
        "end_step": 4,
        "start_step": 1
      },
      "pattern": [
        "grep(pattern=config, path=src)"
      ],
      "shape": "looping",
      "trace_id": "trace-loop-001"
    }
  ],
  "trace_id": "trace-loop-001"
}
```

## Install

Westering is a Rust crate that builds from source with cargo.

```sh
git clone https://github.com/david-engelmann/westering.git
cd westering
cargo build
```

The binary lands at target/debug/westering, and the test suite runs with `cargo test`.

## Quickstart

From the repository root, with the committed examples.

```sh
./target/debug/westering detect --trace examples/looping_trace.json
```

The command prints the detections at the top of this page.

## Usage

### detect

`westering detect --trace <file>` runs the stall detectors over a recorded trace file and prints the detections as JSON, with the shape and the pattern and the evidence span of each catch. The committed examples carry a looping trace and a fixation trace beside the detection documents a run over them produces.

```console
$ ./target/debug/westering detect --trace examples/fixation_trace.json
{
  "detections": [
    {
      "detail": "tool grep made 4 of 4 calls with no progress while read, edit, test, finish went unused",
      "evidence_span": {
        "end_step": 3,
        "start_step": 0
      },
      "pattern": [
        "grep"
      ],
      "shape": "tool_fixation",
      "trace_id": "trace-fixation-001"
    }
  ],
  "trace_id": "trace-fixation-001"
}
```

### intervene

`westering intervene --scenario <name>` runs a scripted sandbox session for one of the committed scenarios and prints the session record as JSON, the whole rescue as one timeline with every rung tried and the outcome of the stall. The scenarios cover stalls that clear on each rung and a stall that never clears. With `--fixtures <dir>` the session also consults the recorded judgment fixtures in that directory at the two judgment points, severity and recovery. A trusted severity answer records its score on the intervention under the name severity, and a trusted recovery answer records its choice under the name recovery. An abstention, a malformed answer, or an answer whose confidence falls below the trust threshold records nothing at that point, and at that point the session runs exactly as it runs without the flag. The committed fixtures live in the fixtures directory at the repository root, and `./target/debug/westering intervene --scenario loop-clears-on-nudge --fixtures fixtures` runs the nudge scenario against them.

```console
$ ./target/debug/westering intervene --scenario loop-clears-on-nudge
{
  "cleared_by": "nudge",
  "detections": [
    {
      "detail": "call pattern of length 1 repeated 3 times with no progress",
      "evidence_span": {
        "end_step": 2,
        "start_step": 0
      },
      "pattern": [
        "grep(pattern=config, path=src)"
      ],
      "shape": "looping",
      "trace_id": "sandbox-loop-clears-on-nudge"
    }
  ],
  "events": [
    {
      "call": "grep(pattern=config, path=src)",
      "progress": false,
      "step": 0,
      "tool": "grep"
    },
    {
      "call": "grep(pattern=config, path=src)",
      "progress": false,
      "step": 1,
      "tool": "grep"
    },
    {
      "call": "grep(pattern=config, path=src)",
      "progress": false,
      "step": 2,
      "tool": "grep"
    },
    {
      "call": "read(path=src/config.py)",
      "progress": true,
      "step": 3,
      "tool": "read"
    },
    {
      "call": "test(target=config)",
      "progress": true,
      "step": 4,
      "tool": "test"
    },
    {
      "call": "finish(summary=config located)",
      "progress": true,
      "step": 5,
      "tool": "finish"
    }
  ],
  "goal": "Find the config file and report its path",
  "interventions": [
    {
      "at_step": 2,
      "fired_by": {
        "evidence_span": {
          "end_step": 2,
          "start_step": 0
        },
        "pattern": [
          "grep(pattern=config, path=src)"
        ],
        "shape": "looping",
        "trace_id": "sandbox-loop-clears-on-nudge"
      },
      "message": "Stop. Your recent work stalled in the shape looping. Change approach and work toward the goal. The goal is Find the config file and report its path.",
      "rung": "nudge"
    }
  ],
  "outcome": "cleared",
  "session_id": "sandbox-loop-clears-on-nudge",
  "stall_cleared": true
}
```

Every command reads only the files it is handed. A trace or fixture file that is malformed or cannot be read refuses with one error line and exit code 2, and a malformed invocation exits with status 2. A document nested past the reader ceiling refuses with one error line at exit status 1.

## Status

The detectors, the four rung ladder, and the recorded fixture seam are built and tested, and every run happens against scripted agents in a local sandbox. The kill rung is not built here and never will be, because that rung belongs to arvin. Round 8 landed in PR 19, repositioning the repo as the offline policy lab it is built as. Round 9 landed in PR 21, completing the help guards and discharging the port note on the hand root table. Round 10 landed in PR 23, dispositioning the fixture player depth note and pinning the key ceiling on both surfaces. Round 11 landed in PR 25, pinning the argument walk's branches by direct unit tests. Round 12 landed in PR 27, firing the fork rung behind its states in the committed corpus. Round 13 landed in PR 29, firing the detect command behind its states in the committed corpus. Closed rounds 13 of 100. Part of the impatient programming portfolio, in the Fleet Operations arc.

## License

MIT. See [LICENSE](LICENSE).
