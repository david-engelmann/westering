//! Command line entry point, the westering binary.
//!
//! Subcommands
//!   detect     run the stall detectors over a trace file
//!   intervene  run a scripted sandbox session
//!
//! The argument parsing is clap, adopted under the maintainer's
//! tool choice ruling. Before clap sees the arguments, a
//! preprocess walks them the way the Python parser's own scan
//! walked them. It joins onto their flags the values that scan
//! would have taken, fires help where that scan fired it, and
//! duplicates a separator token that scan refused, so the
//! functional outcomes of the two command lines agree, the same
//! subcommands and flags, misuse at exit status 2, help at exit
//! status 0, and unambiguous long flag abbreviations still
//! executing. Help text and malformed invocation error text are
//! clap's own forms. The documents the tool emits and the one
//! error line discipline for its own refusals are unchanged. A
//! failure the Python tool lets escape uncaught answers at
//! status 1, and a standard output failure that surfaces only at
//! its exit flush answers at status 120. Errors in main ride on
//! anyhow. The library's error types derive thiserror.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand};

use westering::detect;
use westering::judgment::{FixtureLoadError, FixturePlayer, JudgmentSource};
use westering::pyjson::{self, Value, decimal_digit_value};
use westering::sandbox::{RunSessionError, run_session, scenario_names};
use westering::trace::{LoadTraceError, load_trace};

#[derive(Parser)]
#[command(
    about = "An offline policy lab for a graded stall rescue ladder. It reads recorded traces of agent work, detects looping and fixation stalls, and replays scripted scenarios against the ladder, recording which rung clears the stall.",
    subcommand_required = true,
    infer_long_args = true,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the stall detectors over a trace file.
    #[command(infer_long_args = true, args_override_self = true)]
    Detect(DetectArgs),
    /// Run a scripted sandbox session.
    #[command(infer_long_args = true, args_override_self = true)]
    Intervene(InterveneArgs),
}

#[derive(Args)]
struct DetectArgs {
    /// Path to a synthetic trace JSON file.
    #[arg(long)]
    trace: PathBuf,
}

#[derive(Args)]
struct InterveneArgs {
    /// The scripted scenario to run.
    #[arg(long, value_parser = scenario_value_parser())]
    scenario: String,
    /// Path to a directory of recorded judgment fixtures.
    #[arg(long)]
    fixtures: Option<PathBuf>,
}

fn scenario_value_parser() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(scenario_names())
}

/// A failure the Python tool lets escape uncaught.
///
/// The binary carries the failure in its own type and main
/// answers it with status 1 and one error line, where every
/// other refusal answers with status 2.
#[derive(Debug)]
struct Uncaught(String);

impl std::fmt::Display for Uncaught {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Uncaught {}

/// A standard output failure the Python tool discovers only
/// when its buffer is flushed as the tool exits.
///
/// The Python tool holds its printed output in a buffer of
/// 8192 bytes, so a failure under a smaller output is never
/// discovered during the run. It surfaces at the shutdown
/// flush, where the Python tool reports it as an exception
/// ignored and exits with status 120 and no traceback, so
/// the binary carries the failure in its own type and main
/// answers it with status 120 and one error line.
#[derive(Debug)]
struct FailedFlush(String);

impl std::fmt::Display for FailedFlush {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FailedFlush {}

/// The size of the buffer the Python tool holds its printed
/// output in, in bytes. A printed output of this size or more
/// is written through as it is printed, so a failure under it
/// surfaces during the run, and a smaller output is flushed
/// only as the tool exits.
const STDOUT_BUFFER_BYTES: usize = 8192;

fn print_out(text: &str) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    lock.write_all(text.as_bytes())
        .and_then(|()| lock.flush())
        .map_err(|error| {
            let message = format!("stdout: {}", error);
            if text.len() < STDOUT_BUFFER_BYTES {
                anyhow::Error::new(FailedFlush(message))
            } else {
                anyhow::Error::new(Uncaught(message))
            }
        })
}

fn cmd_detect(args: &DetectArgs) -> anyhow::Result<i32> {
    let trace = load_trace(&args.trace).map_err(|error| match error {
        LoadTraceError::Trace(refusal) => anyhow::Error::new(refusal),
        LoadTraceError::TooDeep(display) => anyhow::Error::new(Uncaught(format!(
            "trace file is nested too deeply, {}",
            display
        ))),
    })?;
    let payload = Value::object(vec![
        ("trace_id".to_string(), Value::Str(trace.trace_id.clone())),
        (
            "detections".to_string(),
            Value::List(detect::detect(&trace, 0)),
        ),
    ]);
    print_out(&(pyjson::dumps_indent2(&payload) + "\n"))?;
    Ok(0)
}

fn cmd_intervene(args: &InterveneArgs) -> anyhow::Result<i32> {
    let player = match &args.fixtures {
        Some(directory) => Some(FixturePlayer::new(directory).map_err(|error| match error {
            FixtureLoadError::Judgment(refusal) => anyhow::Error::new(refusal),
            FixtureLoadError::TooDeep(display) => anyhow::Error::new(Uncaught(format!(
                "fixture file is nested too deeply, {}",
                display
            ))),
        })?),
        None => None,
    };
    let judgments: Option<&dyn JudgmentSource> =
        player.as_ref().map(|item| item as &dyn JudgmentSource);
    let record = run_session(&args.scenario, judgments).map_err(|error| match error {
        RunSessionError::UnknownScenario(name) => {
            anyhow::anyhow!("unknown scenario {}", name)
        }
        RunSessionError::Ladder(refusal) => anyhow::Error::new(Uncaught(refusal.0.clone())),
    })?;
    print_out(&(pyjson::dumps_indent2(&record) + "\n"))?;
    Ok(0)
}

/// The exit status an error from a command answers with.
///
/// A failure the Python tool lets escape uncaught answers at
/// status 1. A standard output failure discovered only at the
/// exit flush answers with status 120, the status the Python
/// tool exits with when it reports that failure as an exception
/// ignored. Every other refusal the tool makes answers with
/// status 2.
fn refusal_status(error: &anyhow::Error) -> u8 {
    if error.downcast_ref::<FailedFlush>().is_some() {
        return 120;
    }
    if error.downcast_ref::<Uncaught>().is_some() {
        return 1;
    }
    2
}

/// Whether the token after its dash reads as a negative number
/// under the argparse pattern, one run of decimal digits, or a
/// run of digits, a point, and another run. The digit set is the
/// full Unicode decimal set, as it is in a Python text pattern.
fn is_negative_number(token: &str) -> bool {
    let Some(body) = token.strip_prefix('-') else {
        return false;
    };
    let digit = |ch: char| decimal_digit_value(ch).is_some();
    if !body.is_empty() && body.chars().all(digit) {
        return true;
    }
    match body.split_once('.') {
        Some((whole, fraction)) => {
            whole.chars().all(digit) && !fraction.is_empty() && fraction.chars().all(digit)
        }
        None => false,
    }
}

/// Whether the negative number pattern matches the token, where
/// the pattern's end anchor also matches just before one
/// trailing newline, as a Python text pattern's does.
fn negative_number_matches(token: &str) -> bool {
    is_negative_number(token) || token.strip_suffix('\n').is_some_and(is_negative_number)
}

/// Whether a token reads as a positional string rather than as
/// an option, the classification the Python parser makes when it
/// meets the token. A bare word and a lone dash are positional.
/// A dash token carrying a space reads as positional whatever
/// its prefix, the one exception being the short help family,
/// whose letter after the dash decides ahead of the space rule,
/// as the Python parser refuses a help token with a trailing
/// word in a value position rather than consuming it. A dash
/// token without a space is positional only when it matches
/// the negative number pattern. Any long token without a space
/// is an option.
fn reads_as_positional(token: &str) -> bool {
    if token == "--" {
        return false;
    }
    if !token.starts_with('-') || token == "-" {
        return true;
    }
    if token.chars().nth(1) == Some('h') {
        return false;
    }
    if token.contains(' ') {
        return true;
    }
    if token.starts_with("--") {
        return false;
    }
    negative_number_matches(token)
}

/// One subcommand of the command line, with the long flags the
/// derive model declares for it.
///
/// The flag tables of the preprocess come from clap's own model
/// of the derive structs, read through the command factory, so
/// the derive model stays the one owner of the flag set and this
/// layer restates none of it.
struct SubcommandSpec {
    name: String,
    value_flags: Vec<String>,
    longs: Vec<String>,
    /// The value flags that declare a fixed set of possible
    /// values, with the values clap's own model declares for
    /// them. The Python parser validates such a value when it
    /// consumes it, so the walk below validates it at the same
    /// moment rather than after a later help token has fired.
    choices: Vec<(String, Vec<String>)>,
}

fn subcommand_specs() -> Vec<SubcommandSpec> {
    Cli::command()
        .get_subcommands()
        .map(|sub| {
            let mut value_flags: Vec<String> = Vec::new();
            let mut longs: Vec<String> = Vec::new();
            let mut choices: Vec<(String, Vec<String>)> = Vec::new();
            for arg in sub.get_arguments() {
                let Some(long) = arg.get_long() else {
                    continue;
                };
                let name = format!("--{}", long);
                if !longs.contains(&name) {
                    longs.push(name.clone());
                }
                if matches!(arg.get_action(), clap::ArgAction::Set) {
                    value_flags.push(name.clone());
                    let possible: Vec<String> = arg
                        .get_possible_values()
                        .iter()
                        .map(|value| value.get_name().to_string())
                        .collect();
                    if !possible.is_empty() {
                        choices.push((name, possible));
                    }
                }
            }
            if !longs.iter().any(|long| long == "--help") {
                longs.push("--help".to_string());
            }
            SubcommandSpec {
                name: sub.get_name().to_string(),
                value_flags,
                longs,
                choices,
            }
        })
        .collect()
}

/// Whether a value handed to one of a subcommand's flags
/// violates the fixed set that flag declares. A flag outside
/// the choices table declares no set and takes any value, as
/// the Python parser takes it.
fn choice_violation(spec: &SubcommandSpec, flag: &str, value: &str) -> bool {
    spec.choices
        .iter()
        .find(|(name, _)| name == flag)
        .is_some_and(|(_, values)| !values.iter().any(|item| item == value))
}

/// How a token reads as a help token on the current parser.
enum HelpRead {
    /// Not a help token.
    No,
    /// The help action fires.
    Fire,
    /// A help token carrying an explicit value, an error in both
    /// parsers. The scan carries the token in the form clap
    /// refuses in its own voice, and nothing later can fire help.
    Refuse(OsString),
}

/// Read a token as a help token against the long flags of the
/// parser the scan stands on. The Python parser fires help when
/// its scan reaches the token, ahead of extra tokens it has
/// already collected, and it refuses a help token that carries an
/// explicit value, ahead of any help token that follows. A short
/// help token fires when its bundle continues with anything but
/// an equals sign or a dash, the two continuations the parser
/// reads as an explicit value arriving with the token.
fn help_read(token: &str, longs: &[String]) -> HelpRead {
    if token.starts_with("--") {
        let name = token.split('=').next().unwrap_or(token);
        if name.len() > 2 {
            let matches: Vec<&String> =
                longs.iter().filter(|long| long.starts_with(name)).collect();
            if matches.len() == 1 && matches[0] == "--help" {
                if token.contains('=') {
                    return HelpRead::Refuse(OsString::from(token));
                }
                return HelpRead::Fire;
            }
        }
        return HelpRead::No;
    }
    if token.starts_with('-') && token.chars().nth(1) == Some('h') {
        return match token.chars().nth(2) {
            None => HelpRead::Fire,
            Some('=') => HelpRead::Refuse(OsString::from(format!("--help={}", &token[3..]))),
            Some('-') => HelpRead::Refuse(OsString::from(format!("--help={}", &token[2..]))),
            Some(_) => HelpRead::Fire,
        };
    }
    HelpRead::No
}

/// The outcome of the preprocessed argument walk.
enum Preprocess {
    /// Hand these arguments to clap.
    Run(Vec<OsString>),
    /// Print the help of the named subcommand parser, or of the
    /// top level parser when the name is absent, at status 0.
    FireHelp(Option<String>),
}

/// The long flags the walk reads help tokens against on the
/// root, held by hand at the one help long.
///
/// The root command declares no long but --help, so the hand set
/// holds that long alone. The set stands in one named place so
/// the guard pin in the unit tests below reads the same
/// construction the walk reads, and the contract states the pin
/// that proves the hand set and the derived set equal.
fn root_longs() -> Vec<String> {
    vec!["--help".to_string()]
}

/// Walk the arguments the way the Python parser's scan walked
/// them and prepare them for clap.
///
/// The Python parser consumes its arguments from the left. A
/// value flag takes the next token when that token reads as a
/// positional string, and this walk joins such a value onto its
/// flag in the equals form. A flag that declares a fixed set of
/// values has its value validated as it is consumed, in the
/// joined form and the attached form alike, and a value outside
/// the set is an error the parser exits on at once, so the walk
/// stops there and hands the invocation to clap untouched rather
/// than letting a later help token fire over it. A value flag
/// that meets an option token is an error the parser exits on
/// at once, as are a word the subcommand slot cannot take and a
/// help token carrying an explicit value, and past any of those
/// this walk hands the arguments to clap untouched, since clap
/// refuses the same invocation in its own voice. Help fires
/// where the scan reaches it, ahead of extra tokens already
/// collected, and this walk answers a fired help token itself.
/// A separator token ends the scan. The Python parser refuses
/// a separator that follows an otherwise complete invocation,
/// where clap accepts it silently, so this walk duplicates the
/// separator and clap refuses the copy as an unexpected
/// argument in its own voice.
fn preprocess(tokens: &[OsString]) -> Preprocess {
    let specs = subcommand_specs();
    let root_longs = root_longs();
    let mut out: Vec<OsString> = Vec::new();
    let mut current: Option<usize> = None;
    let mut pending: Option<String> = None;
    let mut blocked = false;
    let mut separator: Option<usize> = None;
    for token in tokens {
        if separator.is_some() || blocked {
            out.push(token.clone());
            continue;
        }
        let text = token.to_string_lossy().into_owned();
        if let Some(flag_name) = pending.take() {
            if text == "--" {
                separator = Some(out.len());
                out.push(token.clone());
            } else if reads_as_positional(&text) {
                let spec = &specs[current.expect("a pending flag stands inside a subcommand")];
                if choice_violation(spec, &flag_name, &text) {
                    blocked = true;
                    out.push(token.clone());
                } else {
                    let mut flag = out.pop().expect("a pending flag precedes its value");
                    flag.push("=");
                    flag.push(token);
                    out.push(flag);
                }
            } else {
                blocked = true;
                out.push(token.clone());
            }
            continue;
        }
        if text == "--" {
            separator = Some(out.len());
            out.push(token.clone());
            continue;
        }
        let longs: &[String] = match current {
            Some(index) => &specs[index].longs,
            None => &root_longs,
        };
        match help_read(&text, longs) {
            HelpRead::Fire => {
                return Preprocess::FireHelp(current.map(|index| specs[index].name.clone()));
            }
            HelpRead::Refuse(replacement) => {
                blocked = true;
                out.push(replacement);
                continue;
            }
            HelpRead::No => {}
        }
        if current.is_none() {
            if let Some(index) = specs.iter().position(|spec| spec.name == text) {
                current = Some(index);
                out.push(token.clone());
                continue;
            }
            if reads_as_positional(&text) {
                blocked = true;
            }
            out.push(token.clone());
            continue;
        }
        let spec = &specs[current.expect("a subcommand is selected")];
        if text.starts_with("--") {
            let name = text.split('=').next().unwrap_or(&text);
            let matched: Vec<&String> = spec
                .value_flags
                .iter()
                .filter(|flag| flag.starts_with(name))
                .collect();
            if matched.len() == 1 {
                let canonical = matched[0].clone();
                match text.split_once('=') {
                    Some((_, value)) => {
                        if choice_violation(spec, &canonical, value) {
                            blocked = true;
                        }
                    }
                    None => {
                        pending = Some(canonical);
                    }
                }
                out.push(token.clone());
                continue;
            }
        }
        out.push(token.clone());
    }
    if let Some(index) = separator {
        out.insert(index + 1, OsString::from("--"));
    }
    Preprocess::Run(out)
}

/// Answer a clap error the way the command line answers it.
///
/// An error clap prints to standard error, a malformed
/// invocation, is printed there and answered with its own exit
/// code. An error clap prints to standard output is the help
/// text, and help output takes the failed write discipline of
/// every document the tool prints, because the Python tool's
/// help goes to the same buffered standard output its documents
/// do. The help is rendered and passed through print_out, so a
/// standard output that cannot take the bytes answers at the
/// status the failure family carries, 120 under a small output
/// and 1 under a larger one, with the tool's one error line,
/// rather than exiting 0 over a print whose failure was
/// discarded.
fn answer_clap_error(error: clap::Error) -> ExitCode {
    if error.use_stderr() {
        let code = error.exit_code();
        let _ = error.print();
        return ExitCode::from(code as u8);
    }
    let text = error.render().to_string();
    match print_out(&text) {
        Ok(()) => ExitCode::from(error.exit_code() as u8),
        Err(failure) => {
            eprintln!("error: {}", failure);
            ExitCode::from(refusal_status(&failure))
        }
    }
}

fn main() -> ExitCode {
    // The Python reader reaches documents nested thousands of
    // containers deep before its own ceiling refuses them, and
    // the port's reader, writer, and value walks recurse once per
    // container on the deepest path, as the Python tool's do. The
    // binary therefore runs on a worker thread with a stack sized
    // for that depth, so a document at the ceiling is refused or
    // carried on the tool's own terms and never on the stack's.
    match std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(run)
    {
        Ok(handle) => handle.join().unwrap_or(ExitCode::from(2)),
        Err(_) => run(),
    }
}

fn run() -> ExitCode {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_else(|| OsString::from("westering"));
    let rest: Vec<OsString> = args.collect();
    let joined = match preprocess(&rest) {
        Preprocess::Run(joined) => joined,
        Preprocess::FireHelp(subcommand) => {
            let mut argv = vec![program];
            if let Some(name) = subcommand {
                argv.push(OsString::from(name));
            }
            argv.push(OsString::from("--help"));
            return match Cli::try_parse_from(argv) {
                Ok(_) => ExitCode::from(0),
                Err(error) => answer_clap_error(error),
            };
        }
    };
    let cli = match Cli::try_parse_from(std::iter::once(program).chain(joined)) {
        Ok(cli) => cli,
        Err(error) => return answer_clap_error(error),
    };
    let outcome = match &cli.command {
        Command::Detect(args) => cmd_detect(args),
        Command::Intervene(args) => cmd_intervene(args),
    };
    match outcome {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("error: {}", error);
            ExitCode::from(refusal_status(&error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Derive the root long set from the built command in the
    /// same manner subcommand_specs derives the long set of a
    /// subcommand, collecting the long of every argument the root
    /// command carries and completing the set with the help long
    /// where it stands absent.
    fn derived_root_longs() -> Vec<String> {
        let mut command = Cli::command();
        command.build();
        let mut longs: Vec<String> = Vec::new();
        for arg in command.get_arguments() {
            let Some(long) = arg.get_long() else {
                continue;
            };
            let name = format!("--{}", long);
            if !longs.contains(&name) {
                longs.push(name);
            }
        }
        if !longs.iter().any(|long| long == "--help") {
            longs.push("--help".to_string());
        }
        longs
    }

    /// The root command declares no long but --help, and the hand
    /// set the walk reads on the root holds that long alone. A
    /// long added to the root command breaks the first assertion,
    /// and an edit to the hand set alone breaks the second, so
    /// neither change leaves the hand set standing behind the
    /// parser in silence. The derivation completes the set with
    /// the help long where it stands absent, so the guard runs
    /// one way and the help long's own presence on the root stands
    /// pinned by the corpus help cases rather than by this pin.
    #[test]
    fn root_long_set_is_the_one_help_long() {
        let derived = derived_root_longs();
        assert_eq!(derived, vec!["--help".to_string()]);
        assert_eq!(root_longs(), derived);
    }

    /// Collect the help short of every command in the tree,
    /// walking the root command and every subcommand in turn and
    /// finding on each the argument whose long is the help long.
    fn collect_help_shorts(command: &clap::Command, shorts: &mut Vec<Option<char>>) {
        let arg = command
            .get_arguments()
            .find(|arg| arg.get_long() == Some("help"))
            .expect("every command must have a --help argument");
        shorts.push(arg.get_short());
        for subcommand in command.get_subcommands() {
            collect_help_shorts(subcommand, shorts);
        }
    }

    /// The two reads of the walk test the second character of a
    /// token against the letter h in every context they reach,
    /// because the help argument of every command in the tree
    /// carries the short h. A help short customized to another
    /// letter on any command breaks this pin, and a command
    /// carrying no help argument breaks it, so neither change
    /// leaves the reads standing behind the parser in silence.
    #[test]
    fn help_short_of_every_command_is_h() {
        let mut command = Cli::command();
        command.build();
        let mut shorts: Vec<Option<char>> = Vec::new();
        collect_help_shorts(&command, &mut shorts);
        assert_eq!(shorts.len(), 3, "the count of the commands in the tree");
        assert!(
            shorts.iter().all(|short| *short == Some('h')),
            "every help short is the letter h, got {shorts:?}"
        );
    }
}
