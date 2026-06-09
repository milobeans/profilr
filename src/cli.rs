use std::{
    fs::OpenOptions,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::{
    config::{load_config, Config},
    languages::{language_names, LANGUAGES},
    model::{DoctorCheck, DoctorReport, SortKey},
    profile::profile_project,
    report::{emit_command, emit_json, emit_project_profile, emit_snippet, ReportFormat},
    snippet::{profile_command, profile_snippet, runner_statuses, CommandOptions, SnippetOptions},
    tui::run_tui,
};

#[derive(Parser, Debug)]
#[command(
    name = "profilr",
    version,
    about = "Fast project, command, and snippet profiler",
    long_about = "profilr profiles a codebase by default with a responsive TUI, and exposes JSON-first CLI modes for agents and automation."
)]
pub struct Cli {
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,

    #[arg(
        long,
        global = true,
        help = "Emit stable JSON where the command supports it"
    )]
    json: bool,

    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Use a specific .profilr.toml config"
    )]
    config: Option<PathBuf>,

    #[arg(long, global = true, value_enum, default_value_t = LogLevel::Warn)]
    log_level: LogLevel,

    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Append lightweight run logs to a file"
    )]
    log_file: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Profile a project and write a deterministic report.
    Run(RunArgs),
    /// Open the interactive terminal UI for a project profile.
    Tui(RunArgs),
    /// Profile a project and write pretty, JSON, Markdown, or CSV output.
    Report(ReportArgs),
    /// Time a small code snippet with a language runner.
    Snippet(SnippetArgs),
    /// Time an arbitrary command.
    Command(CommandArgs),
    /// Check local setup, config, and snippet runners.
    Doctor,
    /// Print default configuration.
    Config(ConfigArgs),
    /// Inspect profilr language adapters and runner support.
    Tools(ToolsArgs),
}

#[derive(Args, Debug, Clone)]
struct RunArgs {
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
    #[arg(long, value_enum)]
    sort: Option<SortKey>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long = "language", value_name = "NAME")]
    languages: Vec<String>,
    #[arg(long, value_enum, default_value_t = ReportFormat::Pretty)]
    format: ReportFormat,
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
struct ReportArgs {
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
    #[arg(long, value_enum)]
    sort: Option<SortKey>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long = "language", value_name = "NAME")]
    languages: Vec<String>,
    #[arg(long, value_enum, default_value_t = ReportFormat::Markdown)]
    format: ReportFormat,
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct SnippetArgs {
    #[arg(short, long)]
    language: Option<String>,
    #[arg(long)]
    code: Option<String>,
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,
    #[arg(long, default_value_t = 10)]
    iterations: usize,
    #[arg(long, default_value_t = 1)]
    warmups: usize,
}

#[derive(Args, Debug)]
struct CommandArgs {
    #[arg(long, default_value_t = 3)]
    iterations: usize,
    #[arg(long, default_value_t = 0)]
    warmups: usize,
    #[arg(required = true, last = true)]
    command: Vec<String>,
}

#[derive(Args, Debug)]
struct ConfigArgs {
    #[arg(long, help = "Print the built-in default TOML config")]
    print_default: bool,
}

#[derive(Args, Debug)]
struct ToolsArgs {
    #[command(subcommand)]
    command: ToolsCommand,
}

#[derive(Subcommand, Debug)]
enum ToolsCommand {
    /// List file-language adapters used by the project scanner.
    Languages,
    /// List snippet runners and whether each is installed.
    Runners,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum)]
enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

pub fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let exit_code = if err.exit_code() == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
            let _ = err.print();
            return exit_code;
        }
    };

    match dispatch(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if cli.json {
                let payload = serde_json::json!({
                    "ok": false,
                    "error": err.to_string(),
                });
                let _ = emit_json(&payload);
            } else {
                eprintln!("profilr: {err:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: &Cli) -> Result<()> {
    log_event(cli, "start")?;
    match &cli.command {
        None => run_default(cli),
        Some(Commands::Run(args)) => run_profile(cli, args.clone(), cli.json),
        Some(Commands::Tui(args)) => open_tui(cli, args),
        Some(Commands::Report(args)) => run_report(cli, args),
        Some(Commands::Snippet(args)) => {
            let profile = profile_snippet(SnippetOptions {
                language: args.language.clone(),
                code: args.code.clone(),
                file: args.file.clone(),
                iterations: args.iterations,
                warmups: args.warmups,
            })?;
            emit_snippet(&profile, cli.json)
        }
        Some(Commands::Command(args)) => {
            let profile = profile_command(CommandOptions {
                command: args.command.clone(),
                iterations: args.iterations,
                warmups: args.warmups,
            })?;
            emit_command(&profile, cli.json)
        }
        Some(Commands::Doctor) => {
            let report = doctor_report(cli)?;
            if cli.json {
                emit_json(&report)
            } else {
                print_doctor(report);
                Ok(())
            }
        }
        Some(Commands::Config(args)) => {
            if args.print_default {
                println!("{}", Config::default_toml()?);
                Ok(())
            } else {
                println!("Use `profilr config --print-default` to print a starter .profilr.toml");
                Ok(())
            }
        }
        Some(Commands::Tools(args)) => match args.command {
            ToolsCommand::Languages => {
                if cli.json {
                    emit_json(&LANGUAGES)
                } else {
                    for spec in LANGUAGES {
                        println!(
                            "{:<12} extensions: {:<30} snippet: {}",
                            spec.name,
                            spec.extensions.join(","),
                            spec.snippet_runner.unwrap_or("project-only")
                        );
                    }
                    Ok(())
                }
            }
            ToolsCommand::Runners => {
                let runners = runner_statuses();
                if cli.json {
                    emit_json(&runners)
                } else {
                    for runner in runners {
                        let status = if runner.available { "ok" } else { "missing" };
                        println!(
                            "{:<12} {:<8} {}",
                            runner.language,
                            status,
                            runner.command.unwrap_or(runner.notes)
                        );
                    }
                    Ok(())
                }
            }
        },
    }
}

fn run_default(cli: &Cli) -> Result<()> {
    let loaded = load_config(cli.config.as_deref(), &cli.path)?;
    let sort = loaded.config.output.sort;
    let limit = loaded.config.output.limit;
    let profile = profile_project(&cli.path, &loaded.config, sort, &[])?;

    if cli.json {
        emit_project_profile(&profile, ReportFormat::Json, sort, limit, None)
    } else if std::io::stdout().is_terminal() {
        run_tui(profile, sort, limit)
    } else {
        emit_project_profile(&profile, ReportFormat::Pretty, sort, limit, None)
    }
}

fn run_profile(cli: &Cli, args: RunArgs, force_json: bool) -> Result<()> {
    let loaded = load_config(cli.config.as_deref(), &args.path)?;
    let sort = args.sort.unwrap_or(loaded.config.output.sort);
    let limit = args.limit.unwrap_or(loaded.config.output.limit);
    let format = if force_json {
        ReportFormat::Json
    } else {
        args.format
    };
    let profile = profile_project(&args.path, &loaded.config, sort, &args.languages)?;
    emit_project_profile(&profile, format, sort, limit, args.output)
}

fn open_tui(cli: &Cli, args: &RunArgs) -> Result<()> {
    let loaded = load_config(cli.config.as_deref(), &args.path)?;
    let sort = args.sort.unwrap_or(loaded.config.output.sort);
    let limit = args.limit.unwrap_or(loaded.config.output.limit);
    let profile = profile_project(&args.path, &loaded.config, sort, &args.languages)?;
    if cli.json {
        emit_project_profile(
            &profile,
            ReportFormat::Json,
            sort,
            limit,
            args.output.clone(),
        )
    } else {
        run_tui(profile, sort, limit)
    }
}

fn run_report(cli: &Cli, args: &ReportArgs) -> Result<()> {
    let loaded = load_config(cli.config.as_deref(), &args.path)?;
    let sort = args.sort.unwrap_or(loaded.config.output.sort);
    let limit = args.limit.unwrap_or(loaded.config.output.limit);
    let format = if cli.json {
        ReportFormat::Json
    } else {
        args.format
    };
    let profile = profile_project(&args.path, &loaded.config, sort, &args.languages)?;
    emit_project_profile(&profile, format, sort, limit, args.output.clone())
}

fn doctor_report(cli: &Cli) -> Result<DoctorReport> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let loaded = load_config(cli.config.as_deref(), &cli.path)?;
    let mut checks = Vec::new();
    checks.push(DoctorCheck {
        name: "project path".into(),
        ok: cli.path.exists(),
        details: cli.path.display().to_string(),
    });
    checks.push(DoctorCheck {
        name: "config".into(),
        ok: true,
        details: loaded
            .source
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "built-in defaults".into()),
    });
    checks.push(DoctorCheck {
        name: "languages".into(),
        ok: !language_names().is_empty(),
        details: format!("{} scanners", language_names().len()),
    });

    Ok(DoctorReport {
        binary: "profilr".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        cwd,
        config_source: loaded.source,
        auth_required: false,
        default_mode: "interactive TUI when stdout is a terminal; pretty report otherwise".into(),
        runners: runner_statuses(),
        checks,
    })
}

fn print_doctor(report: DoctorReport) {
    println!("profilr {}", report.version);
    println!("cwd: {}", report.cwd.display());
    println!(
        "config: {}",
        report
            .config_source
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "built-in defaults".into())
    );
    println!("auth required: {}", report.auth_required);
    for check in report.checks {
        println!(
            "{}: {} ({})",
            check.name,
            if check.ok { "ok" } else { "failed" },
            check.details
        );
    }
}

fn log_event(cli: &Cli, event: &str) -> Result<()> {
    let Some(path) = &cli.log_file else {
        return Ok(());
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open log file {}", path.display()))?;
    writeln!(
        file,
        "event={event} level={:?} version={} path={}",
        cli.log_level,
        env!("CARGO_PKG_VERSION"),
        cli.path.display()
    )?;
    Ok(())
}

#[allow(dead_code)]
fn path_display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
