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

    /// Run the walk over a token stream and answer the token
    /// list it hands to clap, in text form. A stream on which
    /// help fires fails here instead of answering, so a pin
    /// expecting a run reads a fired help as a failure.
    fn walk_run(tokens: &[&str]) -> Vec<String> {
        let owned: Vec<OsString> = tokens.iter().map(OsString::from).collect();
        match preprocess(&owned) {
            Preprocess::Run(out) => out
                .iter()
                .map(|token| token.to_string_lossy().into_owned())
                .collect(),
            Preprocess::FireHelp(name) => {
                panic!("expected a run and the walk fired help for {name:?}")
            }
        }
    }

    /// Run the walk over a token stream and answer the name of
    /// the parser whose help it fires, or no name for the root.
    /// A stream the walk hands to clap fails here instead.
    fn walk_fires(tokens: &[&str]) -> Option<String> {
        let owned: Vec<OsString> = tokens.iter().map(OsString::from).collect();
        match preprocess(&owned) {
            Preprocess::FireHelp(name) => name,
            Preprocess::Run(out) => {
                panic!("expected fired help and the walk ran {out:?}")
            }
        }
    }

    /// Assert the walk hands the expected token list to clap.
    fn expect_run(tokens: &[&str], expected: &[&str]) {
        let expected: Vec<String> = expected.iter().map(|item| item.to_string()).collect();
        assert_eq!(walk_run(tokens), expected);
    }

    /// Assert the walk fires the help of the expected parser,
    /// or of the root when the name is absent.
    fn expect_fires(tokens: &[&str], expected: Option<&str>) {
        assert_eq!(walk_fires(tokens).as_deref(), expected);
    }

    /// A subcommand word at the root selects that subcommand's
    /// tables, so a help token behind it fires that parser's
    /// help under its name and never the root's.
    #[test]
    fn walk_recognizes_a_subcommand_and_reads_its_tables() {
        expect_fires(
            &["intervene", "--scenario=loop-clears-on-nudge", "--help"],
            Some("intervene"),
        );
    }

    /// A root word naming no subcommand reads as a positional
    /// and blocks the walk, so the subcommand word and the help
    /// token behind it pass through untouched.
    #[test]
    fn walk_blocks_on_a_root_word_that_names_no_subcommand() {
        expect_run(&["word", "detect", "--help"], &["word", "detect", "--help"]);
    }

    /// A root token shaped as an option blocks nothing, so the
    /// walk still recognizes the subcommand behind it and joins
    /// the value behind that.
    #[test]
    fn walk_passes_a_root_option_token_without_blocking() {
        expect_run(
            &["--bogus", "detect", "--trace", "trace.json"],
            &["--bogus", "detect", "--trace=trace.json"],
        );
    }

    /// Recognition is by the whole word, so an abbreviated
    /// subcommand word reads as a positional and blocks, and
    /// the help token behind it never fires.
    #[test]
    fn walk_blocks_on_an_abbreviated_subcommand_word() {
        expect_run(&["det", "--help"], &["det", "--help"]);
    }

    /// The long help spelling at the root fires the root help.
    #[test]
    fn walk_fires_root_help_on_the_long_spelling() {
        expect_fires(&["--help"], None);
    }

    /// An abbreviated long naming the one root long uniquely
    /// fires the root help, as the Python parser's scan fires
    /// an unambiguous abbreviation.
    #[test]
    fn walk_fires_root_help_on_an_abbreviated_long() {
        expect_fires(&["--he"], None);
    }

    /// The shortest abbreviation of the help long still names
    /// it uniquely on the root and fires.
    #[test]
    fn walk_fires_root_help_on_the_shortest_abbreviation() {
        expect_fires(&["--h"], None);
    }

    /// A help long carrying an explicit value is refused, the
    /// token carried as written and the walk handing it to clap.
    #[test]
    fn walk_refuses_root_help_carrying_a_value() {
        expect_run(&["--help=x"], &["--help=x"]);
    }

    /// The refusal of a valued help token runs on the
    /// abbreviated spelling the same way, and the block it
    /// sets keeps the help token behind it from firing.
    #[test]
    fn walk_refuses_an_abbreviated_help_carrying_a_value() {
        expect_run(&["--h=x", "--help"], &["--h=x", "--help"]);
    }

    /// The short help spelling at the root fires the root help.
    #[test]
    fn walk_fires_root_help_on_the_short_spelling() {
        expect_fires(&["-h"], None);
    }

    /// A short bundle continuing past the h with a letter fires
    /// as the short help spelling does.
    #[test]
    fn walk_fires_root_help_on_a_short_bundle() {
        expect_fires(&["-hx"], None);
    }

    /// A short help token carrying a value after an equals sign
    /// is refused, and the walk carries it in the long form
    /// clap refuses in its own voice, and the block it sets
    /// keeps the help token behind it from firing.
    #[test]
    fn walk_refuses_short_help_with_an_equals_value() {
        expect_run(&["-h=x", "--help"], &["--help=x", "--help"]);
    }

    /// A short help token continuing with a dash reads as a
    /// help token carrying a value and is refused in the same
    /// carried form, under the same block.
    #[test]
    fn walk_refuses_short_help_with_a_dash_value() {
        expect_run(&["-h-x", "--help"], &["--help=-x", "--help"]);
    }

    /// A long token extending past the help long names no long
    /// on the root and reads as no help, and it blocks nothing,
    /// so the subcommand behind it is still recognized.
    #[test]
    fn walk_reads_no_help_in_a_longer_long_token() {
        expect_fires(&["--helpme", "detect", "--help"], Some("detect"));
    }

    /// Help at the root fires where the scan reaches it, ahead
    /// of a subcommand word standing later in the stream.
    #[test]
    fn walk_fires_root_help_before_any_subcommand() {
        expect_fires(&["-h", "detect"], None);
    }

    /// The long help spelling behind detect fires the detect
    /// help under its name.
    #[test]
    fn walk_fires_detect_help_and_names_it() {
        expect_fires(&["detect", "--help"], Some("detect"));
    }

    /// The short help spelling behind intervene fires the
    /// intervene help under its name.
    #[test]
    fn walk_fires_intervene_help_and_names_it() {
        expect_fires(&["intervene", "-h"], Some("intervene"));
    }

    /// An abbreviated help long fires behind a subcommand as it
    /// fires at the root, against the subcommand's own longs.
    #[test]
    fn walk_fires_help_on_an_abbreviation_behind_detect() {
        expect_fires(&["detect", "--he"], Some("detect"));
    }

    /// The shortest abbreviation fires behind intervene, whose
    /// long set carries two value flags beside the help long
    /// and neither of them starts with the letter h.
    #[test]
    fn walk_fires_help_on_the_shortest_abbreviation_behind_intervene() {
        expect_fires(&["intervene", "--h"], Some("intervene"));
    }

    /// A valued help token behind a subcommand is refused and
    /// carried as written, and the block it sets keeps the
    /// help token behind it from firing.
    #[test]
    fn walk_refuses_help_carrying_a_value_behind_intervene() {
        expect_run(
            &["intervene", "--help=x", "--help"],
            &["intervene", "--help=x", "--help"],
        );
    }

    /// A short help token carrying a value behind detect is
    /// refused and carried in the long form, under the same
    /// block.
    #[test]
    fn walk_refuses_short_help_behind_detect_and_carries_the_long_form() {
        expect_run(
            &["detect", "-h=y", "--help"],
            &["detect", "--help=y", "--help"],
        );
    }

    /// A short token that is no help token behind a subcommand
    /// passes as it stands and blocks nothing, so help behind
    /// it still fires.
    #[test]
    fn walk_passes_a_short_no_read_token_behind_a_subcommand() {
        expect_fires(&["detect", "-x", "--help"], Some("detect"));
    }

    /// A word value standing after its flag joins onto the
    /// flag with an equals sign.
    #[test]
    fn walk_joins_a_word_value_onto_its_flag() {
        expect_run(
            &["detect", "--trace", "trace.json"],
            &["detect", "--trace=trace.json"],
        );
    }

    /// The join is made on the flag token as written, so an
    /// abbreviated flag keeps its abbreviation in the joined
    /// token while the canonical name serves the validation.
    #[test]
    fn walk_joins_a_value_onto_the_flag_as_it_was_written() {
        expect_run(
            &["intervene", "--scen", "loop-clears-on-nudge"],
            &["intervene", "--scen=loop-clears-on-nudge"],
        );
    }

    /// A lone dash reads as a positional and joins as a value.
    #[test]
    fn walk_joins_a_lone_dash_value() {
        expect_run(&["detect", "--trace", "-"], &["detect", "--trace=-"]);
    }

    /// An empty token reads as a positional and joins as an
    /// empty value.
    #[test]
    fn walk_joins_an_empty_value() {
        expect_run(&["detect", "--trace", ""], &["detect", "--trace="]);
    }

    /// A subcommand word in a value position is a value like
    /// any word, for a flag declaring no fixed set.
    #[test]
    fn walk_joins_a_subcommand_word_as_a_value() {
        expect_run(
            &["detect", "--trace", "intervene"],
            &["detect", "--trace=intervene"],
        );
    }

    /// After a join the walk continues, and the next value
    /// flag in the stream takes its own value the same way.
    #[test]
    fn walk_continues_after_a_join_and_joins_the_next_flag() {
        expect_run(
            &[
                "intervene",
                "--scenario",
                "loop-clears-on-nudge",
                "--fixtures",
                "dir",
            ],
            &[
                "intervene",
                "--scenario=loop-clears-on-nudge",
                "--fixtures=dir",
            ],
        );
    }

    /// A scenario value inside the declared set joins onto
    /// its flag.
    #[test]
    fn walk_joins_a_scenario_value_inside_the_set() {
        expect_run(
            &["intervene", "--scenario", "loop-needs-fork"],
            &["intervene", "--scenario=loop-needs-fork"],
        );
    }

    /// A scenario value outside the declared set is validated
    /// as it is consumed and blocks the walk, the flag and the
    /// value standing separate in the handed over stream.
    #[test]
    fn walk_blocks_on_a_scenario_value_outside_the_set() {
        expect_run(
            &["intervene", "--scenario", "detect"],
            &["intervene", "--scenario", "detect"],
        );
    }

    /// The block on a violating value is real, so a help token
    /// behind the value does not fire.
    #[test]
    fn walk_blocks_on_a_scenario_value_outside_the_set_and_stays_blocked() {
        expect_run(
            &["intervene", "--scenario", "detect", "--help"],
            &["intervene", "--scenario", "detect", "--help"],
        );
    }

    /// An attached scenario value inside the set is validated
    /// and passes, the walk staying open behind it.
    #[test]
    fn walk_passes_an_attached_scenario_value_inside_the_set() {
        expect_fires(
            &["intervene", "--scenario=loop-clears-on-nudge", "--help"],
            Some("intervene"),
        );
    }

    /// An attached scenario value outside the set blocks the
    /// walk as a separate value does, and help behind it does
    /// not fire.
    #[test]
    fn walk_blocks_on_an_attached_scenario_value_outside_the_set() {
        expect_run(
            &["intervene", "--scenario=nope", "--help"],
            &["intervene", "--scenario=nope", "--help"],
        );
    }

    /// The attached form resolves its flag by prefix before it
    /// validates, so an abbreviated choices flag validates its
    /// value against the set. The value here stands outside
    /// the set, so the block it raises can arise only if the
    /// abbreviated flag was resolved to the choices flag and
    /// its value validated, and the help token behind it does
    /// not fire.
    #[test]
    fn walk_validates_an_attached_value_under_an_abbreviated_flag() {
        expect_run(
            &["intervene", "--scen=nope", "--help"],
            &["intervene", "--scen=nope", "--help"],
        );
    }

    /// The attached value is read after the first equals sign,
    /// so a value carrying a further equals sign stands
    /// outside the set and blocks.
    #[test]
    fn walk_blocks_on_an_attached_value_carrying_a_further_equals() {
        expect_run(
            &["intervene", "--scenario=loop-clears-on-nudge=x", "--help"],
            &["intervene", "--scenario=loop-clears-on-nudge=x", "--help"],
        );
    }

    /// A flag token standing where a value is owed reads as
    /// no value and blocks the walk, the pending flag standing
    /// unjoined in the handed over stream.
    #[test]
    fn walk_blocks_when_a_flag_token_follows_a_pending_flag() {
        expect_run(
            &["detect", "--trace", "--fixtures"],
            &["detect", "--trace", "--fixtures"],
        );
    }

    /// The block keeps the second flag from taking a pending
    /// of its own, so the word behind it is not joined to it.
    #[test]
    fn walk_blocks_and_the_second_flag_takes_no_value() {
        expect_run(
            &["intervene", "--scenario", "--fixtures", "dir"],
            &["intervene", "--scenario", "--fixtures", "dir"],
        );
    }

    /// A long help token in a value position never fires, as
    /// PR-43 states, and blocks the walk instead.
    #[test]
    fn walk_blocks_on_a_long_help_token_in_a_value_position() {
        expect_run(
            &["intervene", "--scenario", "--help"],
            &["intervene", "--scenario", "--help"],
        );
    }

    /// A short help token in a value position blocks the same
    /// way, the help family's letter keeping it from reading
    /// as a value at all.
    #[test]
    fn walk_blocks_on_a_short_help_token_in_a_value_position() {
        expect_run(
            &["intervene", "--scenario", "-h"],
            &["intervene", "--scenario", "-h"],
        );
    }

    /// A separator closes the walk and is duplicated after
    /// its recorded position in the handed over stream, so
    /// clap refuses the copy as an unexpected argument.
    #[test]
    fn walk_duplicates_a_separator_behind_a_subcommand() {
        expect_run(&["detect", "--", "x"], &["detect", "--", "--", "x"]);
    }

    /// The separator arm runs at the root the same way, and a
    /// subcommand word after the separator is not recognized.
    #[test]
    fn walk_duplicates_a_separator_at_the_root() {
        expect_run(&["--", "detect"], &["--", "--", "detect"]);
    }

    /// A help token after a separator passes through with the
    /// rest of the stream and never fires.
    #[test]
    fn walk_passes_a_help_token_after_a_separator_without_firing() {
        expect_run(
            &["detect", "--", "--help"],
            &["detect", "--", "--", "--help"],
        );
    }

    /// A separator standing where a value is owed closes the
    /// walk in the pending arm, the flag standing unjoined
    /// and the duplication landing after the separator.
    #[test]
    fn walk_closes_on_a_separator_in_a_value_position() {
        expect_run(
            &["intervene", "--fixtures", "--", "y"],
            &["intervene", "--fixtures", "--", "--", "y"],
        );
    }

    /// Only the first separator records a position, so a
    /// second one passes through and the duplication stands
    /// once.
    #[test]
    fn walk_duplicates_only_the_first_separator() {
        expect_run(&["detect", "--", "--"], &["detect", "--", "--", "--"]);
    }

    /// A separator met after a block passes through once and
    /// is never duplicated, the walk already standing closed
    /// in the other way.
    #[test]
    fn walk_carries_a_separator_after_a_block_once() {
        expect_run(&["word", "--", "x"], &["word", "--", "x"]);
    }

    /// A help token met after a block does not fire, the whole
    /// remainder of the stream passing through untouched.
    #[test]
    fn walk_does_not_fire_help_after_a_block() {
        expect_run(&["--help=x", "--help"], &["--help=x", "--help"]);
    }

    /// The same pass through holds inside a subcommand, where
    /// a separator after a pending arm block stands once.
    #[test]
    fn walk_carries_a_separator_after_a_block_inside_a_subcommand_once() {
        expect_run(
            &["detect", "--trace", "--fixtures", "--"],
            &["detect", "--trace", "--fixtures", "--"],
        );
    }

    /// An integer negative reads as a value under the matcher
    /// and joins onto its flag.
    #[test]
    fn walk_joins_an_integer_negative_value() {
        expect_run(&["detect", "--trace", "-1"], &["detect", "--trace=-1"]);
    }

    /// A dotted negative with digits on both sides of the
    /// point reads as a value and joins.
    #[test]
    fn walk_joins_a_dotted_negative_value() {
        expect_run(&["detect", "--trace", "-1.5"], &["detect", "--trace=-1.5"]);
    }

    /// The whole part of the dotted form may stand empty, so
    /// a negative carrying only a fraction reads as a value.
    #[test]
    fn walk_joins_a_negative_with_an_empty_whole_part() {
        expect_run(&["detect", "--trace", "-.5"], &["detect", "--trace=-.5"]);
    }

    /// The fraction part may not stand empty, so a negative
    /// ending at the point reads as no value and blocks.
    #[test]
    fn walk_blocks_on_a_negative_with_an_empty_fraction() {
        expect_run(&["detect", "--trace", "-1."], &["detect", "--trace", "-1."]);
    }

    /// A second point in the token ends the match, the
    /// fraction run taking digits alone, and the token blocks.
    #[test]
    fn walk_blocks_on_a_negative_with_a_second_point() {
        expect_run(
            &["detect", "--trace", "-1.5.6"],
            &["detect", "--trace", "-1.5.6"],
        );
    }

    /// The pattern's end anchor forgives one trailing
    /// newline, so a negative carrying one reads as a value.
    #[test]
    fn walk_joins_a_negative_with_one_trailing_newline() {
        expect_run(&["detect", "--trace", "-1\n"], &["detect", "--trace=-1\n"]);
    }

    /// Two trailing newlines stand past the anchor's
    /// forgiveness and the token blocks.
    #[test]
    fn walk_blocks_on_a_negative_with_two_trailing_newlines() {
        expect_run(
            &["detect", "--trace", "-1\n\n"],
            &["detect", "--trace", "-1\n\n"],
        );
    }

    /// The matcher's digit class is the full Unicode decimal
    /// set, so a negative written in Arabic-Indic digits reads
    /// as a value and joins.
    #[test]
    fn walk_joins_a_negative_in_unicode_decimal_digits() {
        expect_run(&["detect", "--trace", "-١٢"], &["detect", "--trace=-١٢"]);
    }

    /// A dash token carrying a space reads as a value
    /// whatever its prefix, here a short dash token.
    #[test]
    fn walk_joins_a_short_dash_value_carrying_a_space() {
        expect_run(&["detect", "--trace", "-x y"], &["detect", "--trace=-x y"]);
    }

    /// The space rule runs ahead of the long rule as well, so
    /// a long dash token carrying a space joins as a value.
    #[test]
    fn walk_joins_a_long_dash_value_carrying_a_space() {
        expect_run(
            &["intervene", "--fixtures", "--a b"],
            &["intervene", "--fixtures=--a b"],
        );
    }

    /// The one exception to the space rule stands ahead of
    /// it, and a token of the help family carrying a space
    /// never reads as a value and blocks in the pending arm.
    #[test]
    fn walk_blocks_on_a_help_family_token_carrying_a_space() {
        expect_run(
            &["detect", "--trace", "-h y"],
            &["detect", "--trace", "-h y"],
        );
    }

    /// A long token uniquely prefixing one value flag of the
    /// subcommand is read as that flag and stands pending
    /// under it, the join keeping the abbreviation as written.
    #[test]
    fn walk_reads_a_unique_flag_prefix_as_the_flag() {
        expect_run(&["detect", "--tr", "a.json"], &["detect", "--tr=a.json"]);
    }

    /// A long token naming no value flag of the subcommand
    /// passes as it stands and blocks nothing.
    #[test]
    fn walk_passes_a_long_token_naming_no_value_flag() {
        expect_fires(&["detect", "--bogus", "--help"], Some("detect"));
    }

    /// The match runs from the flag toward the token, so a
    /// token longer than the flag it extends matches nothing
    /// and passes unblocked.
    #[test]
    fn walk_passes_a_token_longer_than_the_flag_it_extends() {
        expect_fires(&["detect", "--tracex", "--help"], Some("detect"));
    }

    /// The degenerate attached token carries the name of two
    /// dashes alone, and that name prefixes the one value flag
    /// detect carries, so its attached arm runs and the token
    /// stands unblocked as written. This pin is the check on
    /// that reading of the census.
    #[test]
    fn walk_passes_the_degenerate_attached_token_behind_detect() {
        expect_fires(&["detect", "--=x", "--help"], Some("detect"));
    }

    /// Behind intervene the same token prefixes two value
    /// flags, so the match is not unique and it falls through
    /// unblocked the same way.
    #[test]
    fn walk_passes_the_degenerate_attached_token_behind_intervene() {
        expect_fires(&["intervene", "--=x", "--help"], Some("intervene"));
    }
}
