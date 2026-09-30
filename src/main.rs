//! The binary: argument parsing, the three non-interactive modes, and the TUI.

use std::path::PathBuf;
use std::process::ExitCode;

use agentic_console::{actions, config::Config, dump, state, timings, ui, USAGE};

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
        print!(
            "{}",
            dump::render(&config, &state::Snapshot::collect(&config))
        );
        return ExitCode::SUCCESS;
    }
    if args.probe_timings {
        print!("{}", timings::render(&config));
        return ExitCode::SUCCESS;
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

    match ui::run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("agentic-console: {message}");
            ExitCode::FAILURE
        }
    }
}
