use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::Serialize;

use crate::model::{CommandProfile, CompareReport, ProjectProfile, SnippetProfile, SortKey};

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

pub fn emit_compare(report: &CompareReport, json: bool) -> Result<()> {
    if json {
        return emit_json(report);
    }

    println!("profilr compare");
    println!("base: {}", report.base.display());
    println!("head: {}", report.head.display());
    println!();
    println!("top hotspot deltas");
    for delta in report.hotspot_deltas.iter().take(8) {
        println!(
            "{:+8.1}  {:>8.1} -> {:>8.1}  {}",
            delta.delta_score, delta.base_score, delta.head_score, delta.path
        );
    }
    if !report.workload_deltas.is_empty() {
        println!();
        println!("workload deltas");
        for delta in report.workload_deltas.iter().take(6) {
            println!(
                "{:<18} {:>8} -> {:>8}  {:+8}",
                delta.name,
                delta
                    .base_mean_ms
                    .map(|value| format!("{value:.2}ms"))
                    .unwrap_or_else(|| "-".into()),
                delta
                    .head_mean_ms
                    .map(|value| format!("{value:.2}ms"))
                    .unwrap_or_else(|| "-".into()),
                delta
                    .delta_mean_ms
                    .map(|value| format!("{value:.2}ms"))
                    .unwrap_or_else(|| "-".into())
            );
        }
    }
    Ok(())
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

pub fn save_profile_json(profile: &ProjectProfile, output: PathBuf) -> Result<()> {
    let rendered = serde_json::to_string_pretty(profile).context("serialize saved profile JSON")?;
    fs::write(&output, rendered).with_context(|| format!("write {}", output.display()))
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
    if !profile.detected_projects.is_empty() {
        let detected = profile
            .detected_projects
            .iter()
            .map(|project| project.kind.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("detected projects: {detected}"));
    }
    lines.push(format!(
        "{} workloads detected, {} benchmarked, mean workload time {:.2} ms",
        profile.workloads.len(),
        profile.benchmarked_workloads(),
        profile.total_workload_time_ms()
    ));
    lines.push(String::new());
    lines.push(format!("top hotspots by {}", sort.label()));
    lines.push(format!(
        "{:<5} {:>8} {:<12} {:>7}  {}",
        "rank", "score", "language", "lines", "path"
    ));
    for hotspot in profile.sorted_hotspots(sort, limit) {
        let time_str = hotspot
            .runtime_ms
            .map(|t| format!(" ({:.1}ms)", t))
            .unwrap_or_default();
        lines.push(format!(
            "{:<5} {:>8.1} {:<12} {:>7}  {}{}",
            hotspot.rank, hotspot.score, hotspot.language, hotspot.lines, hotspot.path, time_str
        ));
    }
    if !profile.directories.is_empty() {
        lines.push(String::new());
        lines.push("top directories".into());
        for directory in profile.sorted_directories(limit.min(8)) {
            lines.push(format!(
                "{:>8.1} {:>4} files  {:<12}  {}",
                directory.score,
                directory.files,
                directory
                    .dominant_language
                    .clone()
                    .unwrap_or_else(|| "-".into()),
                directory.path
            ));
        }
    }
    if !profile.workloads.is_empty() {
        lines.push(String::new());
        lines.push("workloads".into());
        for workload in profile.workloads.iter().take(limit.min(8)) {
            let summary = workload
                .result
                .as_ref()
                .map(|result| format!("{:.2} ms mean", result.stats.mean_ms))
                .unwrap_or_else(|| workload.status.clone());
            lines.push(format!(
                "{:<18} {:<8} {}",
                workload.spec.name, workload.spec.kind, summary
            ));
        }
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
        "- Scan duration: `{}` ms\n",
        profile.scan_duration_ms
    ));
    out.push_str(&format!(
        "- Workloads: `{}` detected, `{}` benchmarked\n\n",
        profile.workloads.len(),
        profile.benchmarked_workloads()
    ));
    out.push_str("## Hotspots\n\n");
    out.push_str("| Rank | Score | Language | Lines | Path | Reasons |\n");
    out.push_str("| ---: | ---: | --- | ---: | --- | --- |\n");
    for hotspot in profile.sorted_hotspots(sort, limit) {
        let path_display = if let Some(ms) = hotspot.runtime_ms {
            format!("`{}` ({:.1}ms)", hotspot.path, ms)
        } else {
            format!("`{}`", hotspot.path)
        };
        out.push_str(&format!(
            "| {} | {:.1} | {} | {} | {} | {} |\n",
            hotspot.rank,
            hotspot.score,
            hotspot.language,
            hotspot.lines,
            path_display,
            hotspot.reasons.join(", ")
        ));
    }
    if !profile.directories.is_empty() {
        out.push_str("\n## Directories\n\n");
        out.push_str("| Score | Files | Language | Directory | Top Path |\n");
        out.push_str("| ---: | ---: | --- | --- | --- |\n");
        for directory in profile.sorted_directories(limit.min(10)) {
            out.push_str(&format!(
                "| {:.1} | {} | {} | `{}` | `{}` |\n",
                directory.score,
                directory.files,
                directory
                    .dominant_language
                    .clone()
                    .unwrap_or_else(|| "-".into()),
                directory.path,
                directory.top_path.clone().unwrap_or_else(|| "-".into())
            ));
        }
    }
    if !profile.workloads.is_empty() {
        out.push_str("\n## Workloads\n\n");
        out.push_str("| Name | Kind | Status | Command |\n");
        out.push_str("| --- | --- | --- | --- |\n");
        for workload in profile.workloads.iter().take(limit.min(10)) {
            let status = workload
                .result
                .as_ref()
                .map(|result| format!("{:.2} ms mean", result.stats.mean_ms))
                .unwrap_or_else(|| workload.status.clone());
            out.push_str(&format!(
                "| `{}` | {} | {} | `{}` |\n",
                workload.spec.name,
                workload.spec.kind,
                status,
                workload.spec.command.join(" ")
            ));
        }
    }
    out
}

fn render_csv(profile: &ProjectProfile, sort: SortKey, limit: usize) -> String {
    let mut out = String::from("section,rank,score,language,lines,bytes,path,reasons\n");
    for hotspot in profile.sorted_hotspots(sort, limit) {
        out.push_str(&format!(
            "hotspot,{},{:.3},{},{},{},\"{}\",\"{}\"\n",
            hotspot.rank,
            hotspot.score,
            hotspot.language,
            hotspot.lines,
            hotspot.bytes,
            hotspot.path.replace('"', "\"\""),
            hotspot.reasons.join("; ").replace('"', "\"\"")
        ));
    }
    for directory in profile.sorted_directories(limit.min(profile.directories.len())) {
        out.push_str(&format!(
            "directory,,{:.3},{},{},{},\"{}\",\"{}\"\n",
            directory.score,
            directory
                .dominant_language
                .clone()
                .unwrap_or_else(|| "-".into()),
            directory.lines,
            directory.bytes,
            directory.path.replace('"', "\"\""),
            directory
                .top_path
                .clone()
                .unwrap_or_default()
                .replace('"', "\"\"")
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
    limited.directories = profile
        .sorted_directories(limit.min(profile.directories.len()))
        .into_iter()
        .cloned()
        .collect();
    limited
        .workloads
        .truncate(limit.min(limited.workloads.len()));
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
