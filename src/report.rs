use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::Serialize;

use crate::model::{CommandProfile, ProjectProfile, SnippetProfile, SortKey};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ReportFormat {
    #[default]
    Pretty,
    Json,
    Markdown,
    Csv,
}

pub fn emit_project_profile(
    profile: &ProjectProfile,
    format: ReportFormat,
    sort: SortKey,
    limit: usize,
    output: Option<PathBuf>,
) -> Result<()> {
    let rendered = match format {
        ReportFormat::Pretty => render_pretty(profile, sort, limit),
        ReportFormat::Json => serde_json::to_string_pretty(&limited_profile(profile, sort, limit))
            .context("serialize profile JSON")?,
        ReportFormat::Markdown => render_markdown(profile, sort, limit),
        ReportFormat::Csv => render_csv(profile, sort, limit),
    };
    write_or_print(rendered, output)
}

pub fn emit_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub fn emit_snippet(profile: &SnippetProfile, json: bool) -> Result<()> {
    if json {
        return emit_json(profile);
    }
    println!(
        "snippet {language} via {runner}: mean {mean:.3} ms, median {median:.3} ms, p95 {p95:.3} ms ({iterations} iterations)",
        language = profile.language,
        runner = profile.runner,
        mean = profile.stats.mean_ms,
        median = profile.stats.median_ms,
        p95 = profile.stats.p95_ms,
        iterations = profile.stats.iterations,
    );
    Ok(())
}

pub fn emit_command(profile: &CommandProfile, json: bool) -> Result<()> {
    if json {
        return emit_json(profile);
    }
    println!(
        "command `{command}`: mean {mean:.3} ms, median {median:.3} ms, p95 {p95:.3} ms ({iterations} iterations)",
        command = profile.command.join(" "),
        mean = profile.stats.mean_ms,
        median = profile.stats.median_ms,
        p95 = profile.stats.p95_ms,
        iterations = profile.stats.iterations,
    );
    Ok(())
}

pub fn render_pretty(profile: &ProjectProfile, sort: SortKey, limit: usize) -> String {
    let mut lines = Vec::new();
    lines.push(format!("profilr profile: {}", profile.root.display()));
    lines.push(format!(
        "{} profiled files, {} lines, {} skipped, scan {} ms",
        profile.total_profiled_files,
        profile.total_lines,
        profile.skipped_files,
        profile.scan_duration_ms
    ));
    lines.push(String::new());
    lines.push(format!("top hotspots by {}", sort.label()));
    lines.push(format!(
        "{:<5} {:>8} {:<12} {:>7}  {}",
        "rank", "score", "language", "lines", "path"
    ));
    for hotspot in profile.sorted_hotspots(sort, limit) {
        lines.push(format!(
            "{:<5} {:>8.1} {:<12} {:>7}  {}",
            hotspot.rank, hotspot.score, hotspot.language, hotspot.lines, hotspot.path
        ));
    }
    if !profile.warnings.is_empty() {
        lines.push(String::new());
        lines.push(format!("warnings: {}", profile.warnings.len()));
        for warning in profile.warnings.iter().take(5) {
            lines.push(format!("- {warning}"));
        }
    }
    lines.join("\n")
}

fn render_markdown(profile: &ProjectProfile, sort: SortKey, limit: usize) -> String {
    let mut out = String::new();
    out.push_str("# Profilr Project Profile\n\n");
    out.push_str(&format!("- Root: `{}`\n", profile.root.display()));
    out.push_str(&format!(
        "- Profiled files: `{}`\n",
        profile.total_profiled_files
    ));
    out.push_str(&format!("- Lines: `{}`\n", profile.total_lines));
    out.push_str(&format!(
        "- Scan duration: `{}` ms\n\n",
        profile.scan_duration_ms
    ));
    out.push_str("| Rank | Score | Language | Lines | Path | Reasons |\n");
    out.push_str("| ---: | ---: | --- | ---: | --- | --- |\n");
    for hotspot in profile.sorted_hotspots(sort, limit) {
        out.push_str(&format!(
            "| {} | {:.1} | {} | {} | `{}` | {} |\n",
            hotspot.rank,
            hotspot.score,
            hotspot.language,
            hotspot.lines,
            hotspot.path,
            hotspot.reasons.join(", ")
        ));
    }
    out
}

fn render_csv(profile: &ProjectProfile, sort: SortKey, limit: usize) -> String {
    let mut out = String::from("rank,score,language,lines,bytes,path,reasons\n");
    for hotspot in profile.sorted_hotspots(sort, limit) {
        out.push_str(&format!(
            "{},{:.3},{},{},{},\"{}\",\"{}\"\n",
            hotspot.rank,
            hotspot.score,
            hotspot.language,
            hotspot.lines,
            hotspot.bytes,
            hotspot.path.replace('"', "\"\""),
            hotspot.reasons.join("; ").replace('"', "\"\"")
        ));
    }
    out
}

fn limited_profile(profile: &ProjectProfile, sort: SortKey, limit: usize) -> ProjectProfile {
    let mut limited = profile.clone();
    limited.hotspots = profile
        .sorted_hotspots(sort, limit)
        .into_iter()
        .cloned()
        .collect();
    limited
}

fn write_or_print(rendered: String, output: Option<PathBuf>) -> Result<()> {
    if let Some(path) = output {
        fs::write(&path, rendered).with_context(|| format!("write {}", path.display()))?;
    } else {
        println!("{rendered}");
    }
    Ok(())
}
