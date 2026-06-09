use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};

use anyhow::{anyhow, bail, Context, Result};

use crate::{
    languages::{language_by_name, language_for_path},
    model::{AdapterStatus, CommandProfile, SnippetProfile, TimingStats},
};

#[derive(Clone, Debug)]
pub struct SnippetOptions {
    pub language: Option<String>,
    pub code: Option<String>,
    pub file: Option<PathBuf>,
    pub iterations: usize,
    pub warmups: usize,
}

#[derive(Clone, Debug)]
pub struct CommandOptions {
    pub command: Vec<String>,
    pub iterations: usize,
    pub warmups: usize,
}

#[derive(Clone, Debug)]
struct PreparedRunner {
    program: String,
    args: Vec<String>,
    temp_dir: Option<PathBuf>,
    setup_ms: f64,
}

pub fn profile_snippet(options: SnippetOptions) -> Result<SnippetProfile> {
    let (language, code) = resolve_snippet_input(&options)?;
    let started = Instant::now();
    let runner = prepare_runner(&language, &code)?;
    let setup_ms = started.elapsed().as_secs_f64() * 1000.0 + runner.setup_ms;
    let runs = run_iterations(
        &runner.program,
        &runner.args,
        options.iterations,
        options.warmups,
        setup_ms,
    )?;

    if let Some(temp_dir) = runner.temp_dir {
        let _ = fs::remove_dir_all(temp_dir);
    }

    Ok(SnippetProfile {
        language,
        runner: runner.program,
        stats: runs.stats,
        stdout_preview: runs.stdout_preview,
        stderr_preview: runs.stderr_preview,
    })
}

pub fn profile_command(options: CommandOptions) -> Result<CommandProfile> {
    if options.command.is_empty() {
        bail!("missing command to profile");
    }

    let program = options.command[0].clone();
    let args = options.command[1..].to_vec();
    let runs = run_iterations(&program, &args, options.iterations, options.warmups, 0.0)?;

    Ok(CommandProfile {
        command: options.command,
        stats: runs.stats,
        exit_code: runs.exit_code,
        stdout_preview: runs.stdout_preview,
        stderr_preview: runs.stderr_preview,
    })
}

pub fn runner_statuses() -> Vec<AdapterStatus> {
    crate::languages::LANGUAGES
        .iter()
        .map(|language| {
            let command = language.snippet_runner.and_then(first_available_command);
            AdapterStatus {
                language: language.name.to_string(),
                available: command.is_some(),
                command,
                notes: if language.snippet_runner.is_some() {
                    "snippet timing supported when runner is installed".into()
                } else {
                    "project hotspot analysis only".into()
                },
            }
        })
        .collect()
}

pub fn command_available(name: &str) -> bool {
    find_command(name).is_some()
}

fn resolve_snippet_input(options: &SnippetOptions) -> Result<(String, String)> {
    let code = match (&options.code, &options.file) {
        (Some(code), None) => code.clone(),
        (None, Some(path)) => fs::read_to_string(path)
            .with_context(|| format!("read snippet file {}", path.display()))?,
        (Some(_), Some(_)) => bail!("use --code or --file, not both"),
        (None, None) => bail!("snippet requires --code or --file"),
    };

    let language = if let Some(language) = &options.language {
        language_by_name(language)
            .map(|spec| spec.name.to_string())
            .ok_or_else(|| anyhow!("unsupported language `{language}`"))?
    } else if let Some(path) = &options.file {
        language_for_path(path)
            .map(|spec| spec.name.to_string())
            .ok_or_else(|| anyhow!("cannot infer language from {}", path.display()))?
    } else {
        bail!("snippet requires --language when --code is used");
    };

    Ok((language, code))
}

fn prepare_runner(language: &str, code: &str) -> Result<PreparedRunner> {
    match language {
        "Python" => inline_runner(&["python3", "python"], "-c", code),
        "JavaScript" => inline_runner(&["node"], "-e", code),
        "Ruby" => inline_runner(&["ruby"], "-e", code),
        "PHP" => inline_runner(&["php"], "-r", code),
        "Shell" => inline_runner(&["sh"], "-c", code),
        "R" => inline_runner(&["Rscript"], "-e", code),
        "Lua" => inline_runner(&["lua"], "-e", code),
        "Julia" => inline_runner(&["julia"], "-e", code),
        "Elixir" => inline_runner(&["elixir"], "-e", code),
        "Swift" => inline_runner(&["swift"], "-e", code),
        "Rust" => compiled_runner("rustc", "rs", rust_source(code), &["-O"]),
        "Go" => compiled_runner("go", "go", go_source(code), &["build"]),
        "TypeScript" => file_runner(&["tsx"], "ts", code),
        "Haskell" => file_runner(&["runghc"], "hs", code),
        "Scala" => file_runner(&["scala"], "scala", code),
        _ => bail!("snippet timing is not implemented for {language}; use `profilr tools runners`"),
    }
}

fn inline_runner(candidates: &[&str], flag: &str, code: &str) -> Result<PreparedRunner> {
    let program = candidates
        .iter()
        .find_map(|candidate| find_command(candidate))
        .ok_or_else(|| anyhow!("missing snippet runner; tried {}", candidates.join(", ")))?;
    Ok(PreparedRunner {
        program,
        args: vec![flag.into(), code.into()],
        temp_dir: None,
        setup_ms: 0.0,
    })
}

fn file_runner(candidates: &[&str], extension: &str, code: &str) -> Result<PreparedRunner> {
    let program = candidates
        .iter()
        .find_map(|candidate| find_command(candidate))
        .ok_or_else(|| anyhow!("missing snippet runner; tried {}", candidates.join(", ")))?;
    let temp_dir = create_temp_dir()?;
    let source = temp_dir.join(format!("snippet.{extension}"));
    fs::write(&source, code).with_context(|| format!("write {}", source.display()))?;
    Ok(PreparedRunner {
        program,
        args: vec![source.to_string_lossy().into_owned()],
        temp_dir: Some(temp_dir),
        setup_ms: 0.0,
    })
}

fn compiled_runner(
    command: &str,
    extension: &str,
    code: String,
    setup_args: &[&str],
) -> Result<PreparedRunner> {
    let program = find_command(command).ok_or_else(|| anyhow!("missing `{command}` on PATH"))?;
    let temp_dir = create_temp_dir()?;
    let source = temp_dir.join(format!("snippet.{extension}"));
    let binary = temp_dir.join("snippet-bin");
    fs::write(&source, code).with_context(|| format!("write {}", source.display()))?;

    let started = Instant::now();
    let mut compile = Command::new(&program);
    if command == "go" {
        compile.args(setup_args).arg("-o").arg(&binary).arg(&source);
    } else {
        compile.args(setup_args).arg(&source).arg("-o").arg(&binary);
    }
    let output = compile
        .output()
        .with_context(|| format!("compile snippet with {command}"))?;
    if !output.status.success() {
        let stderr = preview(&output.stderr);
        let _ = fs::remove_dir_all(&temp_dir);
        bail!("snippet compile failed with {command}: {stderr}");
    }

    Ok(PreparedRunner {
        program: binary.to_string_lossy().into_owned(),
        args: Vec::new(),
        temp_dir: Some(temp_dir),
        setup_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

fn rust_source(code: &str) -> String {
    if code.contains("fn main") {
        code.to_string()
    } else {
        format!("fn main() {{\n{code}\n}}\n")
    }
}

fn go_source(code: &str) -> String {
    if code.contains("package main") {
        code.to_string()
    } else {
        format!("package main\n\nfunc main() {{\n{code}\n}}\n")
    }
}

struct IterationRuns {
    stats: TimingStats,
    exit_code: Option<i32>,
    stdout_preview: String,
    stderr_preview: String,
}

fn run_iterations(
    program: &str,
    args: &[String],
    iterations: usize,
    warmups: usize,
    setup_ms: f64,
) -> Result<IterationRuns> {
    if iterations == 0 {
        bail!("iterations must be greater than zero");
    }

    for _ in 0..warmups {
        run_once(program, args).with_context(|| format!("warm up {program}"))?;
    }

    let mut durations = Vec::with_capacity(iterations);
    let mut last_exit_code = None;
    let mut stdout_preview = String::new();
    let mut stderr_preview = String::new();

    for _ in 0..iterations {
        let started = Instant::now();
        let output = run_once(program, args).with_context(|| format!("run {program}"))?;
        durations.push(started.elapsed().as_secs_f64() * 1000.0);
        last_exit_code = output.status.code();
        stdout_preview = preview(&output.stdout);
        stderr_preview = preview(&output.stderr);
        if !output.status.success() {
            bail!(
                "`{}` exited with status {}; stderr: {}",
                program,
                output.status,
                stderr_preview
            );
        }
    }

    Ok(IterationRuns {
        stats: timing_stats(durations, iterations, warmups, setup_ms),
        exit_code: last_exit_code,
        stdout_preview,
        stderr_preview,
    })
}

fn run_once(program: &str, args: &[String]) -> Result<std::process::Output> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("spawn {program}"))
}

fn timing_stats(
    mut durations: Vec<f64>,
    iterations: usize,
    warmups: usize,
    setup_ms: f64,
) -> TimingStats {
    durations.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let sum: f64 = durations.iter().sum();
    let mean_ms = sum / durations.len() as f64;
    let median_ms = percentile(&durations, 0.50);
    let p95_ms = percentile(&durations, 0.95);
    TimingStats {
        iterations,
        warmups,
        min_ms: durations[0],
        mean_ms,
        median_ms,
        p95_ms,
        max_ms: *durations.last().unwrap_or(&durations[0]),
        setup_ms,
    }
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn first_available_command(command: &str) -> Option<String> {
    match command {
        "python3" => ["python3", "python"]
            .iter()
            .find_map(|candidate| find_command(candidate)),
        other => find_command(other),
    }
}

fn find_command(command: &str) -> Option<String> {
    if command.contains(std::path::MAIN_SEPARATOR) {
        return Path::new(command).is_file().then(|| command.to_string());
    }

    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(command))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.to_string_lossy().into_owned())
}

fn create_temp_dir() -> Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = env::temp_dir().join(format!("profilr-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&path).with_context(|| format!("create {}", path.display()))?;
    Ok(path)
}

fn preview(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut preview = text.chars().take(512).collect::<String>();
    if text.chars().count() > 512 {
        preview.push_str("...");
    }
    preview
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_uses_sorted_duration_position() {
        let values = [1.0, 2.0, 4.0, 8.0, 16.0];
        assert_eq!(percentile(&values, 0.5), 4.0);
        assert_eq!(percentile(&values, 0.95), 16.0);
    }

    #[test]
    fn rust_snippet_is_wrapped_when_needed() {
        let wrapped = rust_source("println!(\"hi\");");
        assert!(wrapped.contains("fn main()"));
    }
}
