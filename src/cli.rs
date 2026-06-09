use std::{
    fs::OpenOptions,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::{
    compare::compare_reports,
    config::{load_config, Config},
    languages::{language_names, LANGUAGES},
    model::{BenchmarkMode, DoctorCheck, DoctorReport, SortKey},
    profile::profile_project,
    report::{
        emit_command, emit_compare, emit_json, emit_project_profile, emit_snippet,
        save_profile_json, ReportFormat,
    },
    snippet::{profile_command, profile_snippet, runner_statuses, CommandOptions, SnippetOptions},
    tui::run_tui,
    workload::{
        benchmark_single_workload, benchmark_workloads, detect_projects, detect_workloads,
        BenchmarkOptions,
    },
};

#[derive(Parser, Debug)]
#[command(
    name = "profilr",
    version,
    about = "Fast project, command, and snippet profiler",
    long_about = "profilr profiles whole projects by default with an interactive TUI, inferred workloads, saved JSON reports, and agent-friendly CLI modes."
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
    Run(ProjectArgs),
    /// Open the interactive terminal UI for a project profile.
    Tui(ProjectArgs),
    /// Profile a project and write pretty, JSON, Markdown, or CSV output.
    Report(ProjectArgs),
    /// Time a small code snippet with a language runner.
    Snippet(SnippetArgs),
    /// Time an arbitrary command.
    Command(CommandArgs),
    /// Compare two saved JSON project reports.
    Compare(CompareArgs),
    /// Detect or run inferred project workloads.
    Workloads(WorkloadsArgs),
    /// Check local setup, config, project detection, and snippet runners.
    Doctor(DoctorArgs),
    /// Print default configuration.
    Config(ConfigArgs),
    /// Inspect profilr language adapters and runner support.
    Tools(ToolsArgs),
}

#[derive(Args, Debug, Clone)]
struct ProjectArgs {
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
    #[arg(long, value_enum)]
    bench: Option<BenchmarkMode>,
    #[arg(long)]
    iterations: Option<usize>,
    #[arg(long)]
    warmups: Option<usize>,
    #[arg(long, value_name = "PATH")]
    save: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Ingest a CPU profile / trace file (V8 .cpuprofile or cProfile JSON/stats)"
    )]
    ingest_trace: Option<PathBuf>,
    #[arg(long, help = "Enable per-ecosystem profiling for benchmark workloads")]
    profile: bool,
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
    #[arg(long, value_name = "PATH")]
    cwd: Option<PathBuf>,
    #[arg(required = true, last = true)]
    command: Vec<String>,
}

#[derive(Args, Debug)]
struct CompareArgs {
    #[arg(value_name = "BASE")]
    base: PathBuf,
    #[arg(value_name = "HEAD")]
    head: PathBuf,
}

#[derive(Args, Debug)]
struct WorkloadsArgs {
    #[command(subcommand)]
    command: WorkloadsCommand,
}

#[derive(Subcommand, Debug)]
enum WorkloadsCommand {
    /// List inferred workloads for a project.
    List(WorkloadsListArgs),
    /// Run one or more inferred workloads for a project.
    Run(WorkloadsRunArgs),
}

#[derive(Args, Debug)]
struct WorkloadsListArgs {
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
}

#[derive(Args, Debug)]
struct WorkloadsRunArgs {
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    all: bool,
    #[arg(long)]
    iterations: Option<usize>,
    #[arg(long)]
    warmups: Option<usize>,
    #[arg(long, help = "Enable per-ecosystem profiling for the workload(s)")]
    profile: bool,
}

#[derive(Args, Debug)]
struct DoctorArgs {
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
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
        Some(Commands::Run(args)) => run_profile(cli, args, cli.json),
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
                cwd: args.cwd.clone(),
            })?;
            emit_command(&profile, cli.json)
        }
        Some(Commands::Compare(args)) => {
            let report = compare_reports(&args.base, &args.head)?;
            emit_compare(&report, cli.json)
        }
        Some(Commands::Workloads(args)) => run_workloads(cli, args),
        Some(Commands::Doctor(args)) => {
            let report = doctor_report(cli, &args.path)?;
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
            } else {
                println!("Use `profilr config --print-default` to print a starter .profilr.toml");
            }
            Ok(())
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
    let args = ProjectArgs {
        path: cli.path.clone(),
        sort: None,
        limit: None,
        languages: Vec::new(),
        format: ReportFormat::Pretty,
        output: None,
        bench: None,
        iterations: None,
        warmups: None,
        save: None,
        ingest_trace: None,
        profile: false,
    };
    let loaded = load_config(cli.config.as_deref(), &args.path)?;
    let sort = args.sort.unwrap_or(loaded.config.output.sort);
    let limit = args.limit.unwrap_or(loaded.config.output.limit);
    let profile = build_project_profile(&args, &loaded.config, sort)?;

    if cli.json {
        emit_project_profile(&profile, ReportFormat::Json, sort, limit, None)
    } else if std::io::stdout().is_terminal() {
        run_tui(
            profile,
            sort,
            limit,
            loaded.config.workloads.iterations,
            loaded.config.workloads.warmups,
        )
    } else {
        emit_project_profile(&profile, ReportFormat::Pretty, sort, limit, None)
    }
}

fn run_profile(cli: &Cli, args: &ProjectArgs, force_json: bool) -> Result<()> {
    let loaded = load_config(cli.config.as_deref(), &args.path)?;
    let sort = args.sort.unwrap_or(loaded.config.output.sort);
    let limit = args.limit.unwrap_or(loaded.config.output.limit);
    let format = if force_json {
        ReportFormat::Json
    } else {
        args.format
    };
    let profile = build_project_profile(args, &loaded.config, sort)?;
    if let Some(path) = &args.save {
        save_profile_json(&profile, path.clone())?;
    }
    emit_project_profile(&profile, format, sort, limit, args.output.clone())
}

fn open_tui(cli: &Cli, args: &ProjectArgs) -> Result<()> {
    let loaded = load_config(cli.config.as_deref(), &args.path)?;
    let sort = args.sort.unwrap_or(loaded.config.output.sort);
    let limit = args.limit.unwrap_or(loaded.config.output.limit);
    let profile = build_project_profile(args, &loaded.config, sort)?;
    if let Some(path) = &args.save {
        save_profile_json(&profile, path.clone())?;
    }
    if cli.json {
        emit_project_profile(
            &profile,
            ReportFormat::Json,
            sort,
            limit,
            args.output.clone(),
        )
    } else {
        run_tui(
            profile,
            sort,
            limit,
            resolved_iterations(args, &loaded.config),
            resolved_warmups(args, &loaded.config),
        )
    }
}

fn run_report(cli: &Cli, args: &ProjectArgs) -> Result<()> {
    let loaded = load_config(cli.config.as_deref(), &args.path)?;
    let sort = args.sort.unwrap_or(loaded.config.output.sort);
    let limit = args.limit.unwrap_or(loaded.config.output.limit);
    let format = if cli.json {
        ReportFormat::Json
    } else {
        args.format
    };
    let profile = build_project_profile(args, &loaded.config, sort)?;
    if let Some(path) = &args.save {
        save_profile_json(&profile, path.clone())?;
    }
    emit_project_profile(&profile, format, sort, limit, args.output.clone())
}

fn run_workloads(cli: &Cli, args: &WorkloadsArgs) -> Result<()> {
    match &args.command {
        WorkloadsCommand::List(list_args) => {
            let loaded = load_config(cli.config.as_deref(), &list_args.path)?;
            let workloads = detect_workloads(&list_args.path, loaded.config.workloads.max_detected);
            if cli.json {
                emit_json(&workloads)
            } else {
                for workload in workloads {
                    println!(
                        "{:<18} {:<8} {}",
                        workload.spec.name,
                        workload.spec.kind,
                        workload.spec.command.join(" ")
                    );
                }
                Ok(())
            }
        }
        WorkloadsCommand::Run(run_args) => {
            let loaded = load_config(cli.config.as_deref(), &run_args.path)?;
            let mut workloads =
                detect_workloads(&run_args.path, loaded.config.workloads.max_detected);
            if workloads.is_empty() {
                bail!("no workloads detected for {}", run_args.path.display());
            }
            let iterations = run_args
                .iterations
                .unwrap_or(loaded.config.workloads.iterations);
            let warmups = run_args.warmups.unwrap_or(loaded.config.workloads.warmups);

            let runtime_data = if run_args.all {
                benchmark_workloads(
                    &run_args.path,
                    &mut workloads,
                    &BenchmarkOptions {
                        mode: BenchmarkMode::All,
                        iterations,
                        warmups,
                        auto_limit: loaded.config.workloads.auto_limit,
                        profile_ecosystem: run_args.profile,
                    },
                )?
            } else if let Some(name) = &run_args.name {
                let workload = workloads
                    .iter_mut()
                    .find(|workload| workload.spec.name == *name)
                    .with_context(|| format!("unknown workload `{name}`"))?;
                benchmark_single_workload(
                    &run_args.path,
                    workload,
                    iterations,
                    warmups,
                    run_args.profile,
                )?
            } else {
                benchmark_workloads(
                    &run_args.path,
                    &mut workloads,
                    &BenchmarkOptions {
                        mode: BenchmarkMode::Auto,
                        iterations,
                        warmups,
                        auto_limit: loaded.config.workloads.auto_limit,
                        profile_ecosystem: run_args.profile,
                    },
                )?
            };

            if cli.json {
                emit_json(&workloads)
            } else {
                for workload in workloads {
                    let summary = workload
                        .result
                        .as_ref()
                        .map(|result| format!("{:.2} ms mean", result.stats.mean_ms))
                        .unwrap_or_else(|| workload.status.clone());
                    println!(
                        "{:<18} {:<8} {}",
                        workload.spec.name, workload.spec.kind, summary
                    );
                }
                if run_args.profile && !runtime_data.is_empty() {
                    println!();
                    println!("runtime attribution:");
                    let mut sorted: Vec<_> = runtime_data.iter().collect();
                    sorted
                        .sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
                    for (file, ms) in sorted.iter().take(5) {
                        println!("  {:<40} {:.2} ms", file, ms);
                    }
                }
                Ok(())
            }
        }
    }
}

fn doctor_report(cli: &Cli, path: &Path) -> Result<DoctorReport> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let loaded = load_config(cli.config.as_deref(), path)?;
    let detected_projects = detect_projects(path);
    let mut checks = Vec::new();
    checks.push(DoctorCheck {
        name: "project path".into(),
        ok: path.exists(),
        details: path.display().to_string(),
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
    checks.push(DoctorCheck {
        name: "projects".into(),
        ok: !detected_projects.is_empty(),
        details: if detected_projects.is_empty() {
            "no known project manifests detected".into()
        } else {
            detected_projects
                .iter()
                .map(|project| project.kind.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        },
    });

    Ok(DoctorReport {
        binary: "profilr".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        cwd,
        config_source: loaded.source,
        auth_required: false,
        default_mode: "interactive TUI with hotspots, directories, and inferred workloads when stdout is a terminal".into(),
        runners: runner_statuses(),
        detected_projects,
        checks,
    })
}

fn build_project_profile(
    args: &ProjectArgs,
    config: &Config,
    sort: SortKey,
) -> Result<crate::model::ProjectProfile> {
    let mut profile = profile_project(&args.path, config, sort, &args.languages)?;
    let bench_mode = args.bench.unwrap_or(config.workloads.benchmark_mode);
    if bench_mode != BenchmarkMode::Off {
        let runtime_data = benchmark_workloads(
            &args.path,
            &mut profile.workloads,
            &BenchmarkOptions {
                mode: bench_mode,
                iterations: resolved_iterations(args, config),
                warmups: resolved_warmups(args, config),
                auto_limit: config.workloads.auto_limit,
                profile_ecosystem: args.profile,
            },
        )?;
        if args.profile {
            crate::profile::merge_runtime_attribution(&mut profile, &runtime_data, sort);
        }
    }
    if let Some(ref trace_path) = args.ingest_trace {
        let trace_data = crate::collector::ingest_trace_file(trace_path, &profile.root)?;
        crate::profile::merge_runtime_attribution(&mut profile, &trace_data, sort);
    }
    Ok(profile)
}

fn resolved_iterations(args: &ProjectArgs, config: &Config) -> usize {
    args.iterations
        .unwrap_or(config.workloads.iterations)
        .max(1)
}

fn resolved_warmups(args: &ProjectArgs, config: &Config) -> usize {
    args.warmups.unwrap_or(config.workloads.warmups)
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
    if !report.detected_projects.is_empty() {
        println!(
            "detected projects: {}",
            report
                .detected_projects
                .iter()
                .map(|project| project.kind.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
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
