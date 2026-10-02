//! The binary: argument parsing, the three non-interactive modes, and the TUI.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use agentic_console::{
    actions::{self, ActionKind, Guards, PromptTarget},
    app,
    config::Config,
    conversation::{Conversation, DEFAULT_RING},
    dump, probe, state, timings, ui, wizard, USAGE,
};

#[derive(Debug, Default)]
struct Args {
    repo: Option<PathBuf>,
    config: Option<PathBuf>,
    container: Option<String>,
    dump: bool,
    probe_timings: bool,
    dry_run_actions: bool,
    dry_run_action: Option<String>,
    value: Option<String>,
    /// `--wizard-plan`: the whole runtime chain, run nothing.
    wizard_plan: bool,
    /// `--wizard-run`: walk the chain, one confirmation per command step.
    wizard_run: bool,
    /// `--prompt TEXT`: send one turn of this console's own conversation, for a script and for the
    /// end-to-end test.
    prompt: Option<String>,
    help: bool,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut index = 0usize;
    while index < argv.len() {
        let token = argv[index].as_str();
        let mut take_value = |name: &str| -> Result<String, String> {
            index += 1;
            argv.get(index)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match token {
            "-h" | "--help" => args.help = true,
            "--dump" => args.dump = true,
            "--probe-timings" => args.probe_timings = true,
            "--dry-run-actions" => args.dry_run_actions = true,
            "--repo" => args.repo = Some(PathBuf::from(take_value("--repo")?)),
            "--config" => args.config = Some(PathBuf::from(take_value("--config")?)),
            "--container" => args.container = Some(take_value("--container")?),
            "--dry-run-action" => args.dry_run_action = Some(take_value("--dry-run-action")?),
            "--value" => args.value = Some(take_value("--value")?),
            "--wizard-plan" => args.wizard_plan = true,
            "--wizard-run" => args.wizard_run = true,
            "--prompt" => args.prompt = Some(take_value("--prompt")?),
            other => return Err(format!("unknown argument {other:?}\n\n{USAGE}")),
        }
        index += 1;
    }
    Ok(args)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("agentic-console: {message}");
            return ExitCode::from(2);
        }
    };
    if args.help {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let repo = args
        .repo
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let repo = repo.canonicalize().unwrap_or(repo);
    let mut config = Config::load(&repo, args.config.as_deref());
    if let Some(container) = &args.container {
        config.console.container = container.clone();
        config.container_overridden = true;
    }
    if !repo.join(config.repo_marker()).is_file() {
        eprintln!(
            "agentic-console: warning: {} has no {} -- not a repository this kit was ported to?",
            repo.display(),
            config.repo_marker()
        );
    }

    if args.dump {
        let mut out = dump::render(&config, &state::Snapshot::collect(&config));
        dump::render_conversation(&mut out, &Conversation::new(DEFAULT_RING));
        print!("{out}");
        return ExitCode::SUCCESS;
    }
    if args.probe_timings {
        print!("{}", timings::render(&config));
        return ExitCode::SUCCESS;
    }
    if args.wizard_plan {
        let plan = wizard::plan_from_snapshot(&config);
        print!("{}", wizard::render_plan(&plan));
        return ExitCode::SUCCESS;
    }
    if args.wizard_run {
        return run_wizard(&config);
    }
    if args.dry_run_actions {
        print!("{}", actions::dry_run_all(&config));
        return ExitCode::SUCCESS;
    }
    if let Some(id) = &args.dry_run_action {
        match actions::dry_run_one(&config, id, args.value.as_deref()) {
            Ok(text) => {
                print!("{text}");
                return ExitCode::SUCCESS;
            }
            Err(message) => {
                eprintln!("agentic-console: {message}");
                return ExitCode::from(2);
            }
        }
    }

    if let Some(prompt) = args.prompt.clone() {
        return run_prompt(&config, &prompt);
    }

    match ui::run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("agentic-console: {message}");
            ExitCode::FAILURE
        }
    }
}

/// `--wizard-run`: walk the runtime chain, one confirmation per command step.
///
/// This is the same [`wizard::walk`] the interactive console drives, with the two faces it needs
/// supplied from the command line: the one confirmation per command step is a line read from
/// standard input (`y`/`yes` runs it, anything else stops the walk), and the re-read is the
/// console's own uncached probe collection. A step whose command does not change what the console
/// reads stops the walk -- the next command is never run -- and a human step is printed and the walk
/// ends there rather than faking it.
fn run_wizard(config: &Config) -> ExitCode {
    let plan = wizard::plan_from_snapshot(config);
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut confirm = |_step: &wizard::WizardStep| -> bool {
        print!("  confirm [y/N]: ");
        let _ = std::io::stdout().flush();
        match lines.next() {
            Some(Ok(line)) => matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
            // No input at all (a closed stdin) is not a confirmation.
            _ => false,
        }
    };
    let mut run = |step: &wizard::WizardStep| wizard::start(step, &config.repo);
    let observed = config.clone();
    let mut observe = move || agentic_console::state::Snapshot::collect(&observed).runtime;
    let mut out = std::io::stdout();
    let report = wizard::walk(&plan, &mut confirm, &mut run, &mut observe, &mut out);
    let _ = writeln!(out);
    if report.ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `--prompt TEXT`: send one turn and print it, for a script and for the end-to-end test.
///
/// The same argv the composer builds -- `actions::build` with `ActionKind::Prompt` through the same
/// config -- run without the confirmation screen, because this mode *is* the operator's decision: a
/// line typed on a command line is a confirmed one. The turn's own stream is printed line by line as
/// it arrives, then the console's own transcript, so a caller can read either.
///
/// Nothing is invented when the turn fails: a rate-limited turn prints the run's own envelope
/// (`is_error: true`) and the transcript's refusal line, because the whole point of the transcript is
/// that it does not draw a failure as a success.
fn run_prompt(config: &Config, prompt: &str) -> ExitCode {
    let minted = match agentic_console::uuid::v4() {
        Ok(id) => id,
        Err(error) => {
            eprintln!("agentic-console: no session id could be minted: {error}; nothing was sent");
            return ExitCode::from(2);
        }
    };
    let guards = Guards {
        evaluated: false,
        prompt: PromptTarget {
            session: None,
            minted: Some(minted),
        },
        ..Guards::default()
    };
    let command = match actions::build(config, ActionKind::Prompt, prompt, guards) {
        Ok(command) => command,
        Err(reason) => {
            eprintln!("agentic-console: {reason}");
            return ExitCode::from(2);
        }
    };
    println!("argv: {}", command.display());
    for line in &command.note {
        println!("note: {line}");
    }
    let started = match actions::start(&command) {
        Ok(started) => started,
        Err(error) => {
            eprintln!("agentic-console: could not start: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut conversation = Conversation::new(DEFAULT_RING);
    conversation.open_turn(prompt);
    conversation.note(format!("the console ran: {}", command.display()));
    let mut child = started.child;
    let status_word = loop {
        for channel in &started.channels {
            while let Ok(line) = channel.try_recv() {
                println!("{line}");
                conversation.apply_line(&line);
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break match status.code() {
                    Some(code) => format!("exited with code {code}"),
                    None => "was killed by a signal".to_string(),
                };
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(error) => break format!("could not be waited on: {error}"),
        }
    };
    // The reader threads may still be holding the last of the stream: the child's exit does not
    // guarantee the lines have crossed the channel yet, and a transcript that loses its own last line
    // is the exact lie this console exists not to tell.
    for channel in &started.channels {
        while let Ok(line) = channel.try_recv() {
            println!("{line}");
            conversation.apply_line(&line);
        }
    }
    conversation.close_turn(&status_word);
    println!("\n{status_word}");
    // The run's own record is the only thing that can confirm the lines just streamed, and this path
    // has no worker to take that read, so it is taken here, once the child has exited and everything it
    // was going to write has been written. The reasoning is the same function the interactive console
    // uses, so the two cannot describe one read differently.
    if let Some(session) = conversation.session.clone() {
        let path = format!(
            "{}/{session}.jsonl",
            config.console.session_dir.trim_end_matches('/')
        );
        let read = probe::container_transcript(
            &config.console.container,
            &session,
            &path,
            probe::TRANSCRIPT_TAIL_BYTES,
            probe::now(),
        );
        println!("record: {}", read.source);
        let (confirmed, note) = app::reconcile_read(&mut conversation, &session, &read);
        conversation.note(note);
        println!("reconciled: {confirmed} line(s)");
    }
    let mut out = String::new();
    dump::render_conversation(&mut out, &conversation);
    print!("{out}");
    ExitCode::SUCCESS
}
