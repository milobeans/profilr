use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};

use crate::model::{CompareReport, DirectoryDelta, HotspotDelta, ProjectProfile, WorkloadDelta};

pub fn load_report(path: &Path) -> Result<ProjectProfile> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

pub fn compare_reports(base: &Path, head: &Path) -> Result<CompareReport> {
    let base_report = load_report(base)?;
    let head_report = load_report(head)?;

    Ok(CompareReport {
        base: base.to_path_buf(),
        head: head.to_path_buf(),
        hotspot_deltas: compare_hotspots(&base_report, &head_report),
        directory_deltas: compare_directories(&base_report, &head_report),
        workload_deltas: compare_workloads(&base_report, &head_report),
    })
}

fn compare_hotspots(base: &ProjectProfile, head: &ProjectProfile) -> Vec<HotspotDelta> {
    let base_map: BTreeMap<&str, f64> = base
        .hotspots
        .iter()
        .map(|hotspot| (hotspot.path.as_str(), hotspot.score))
        .collect();
    let mut deltas = Vec::new();
    for hotspot in &head.hotspots {
        let base_score = base_map.get(hotspot.path.as_str()).copied().unwrap_or(0.0);
        deltas.push(HotspotDelta {
            path: hotspot.path.clone(),
            base_score,
            head_score: hotspot.score,
            delta_score: hotspot.score - base_score,
        });
    }
    deltas.sort_by(|left, right| {
        right
            .delta_score
            .abs()
            .partial_cmp(&left.delta_score.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    deltas
}

fn compare_directories(base: &ProjectProfile, head: &ProjectProfile) -> Vec<DirectoryDelta> {
    let base_map: BTreeMap<&str, f64> = base
        .directories
        .iter()
        .map(|directory| (directory.path.as_str(), directory.score))
        .collect();
    let mut deltas = Vec::new();
    for directory in &head.directories {
        let base_score = base_map
            .get(directory.path.as_str())
            .copied()
            .unwrap_or(0.0);
        deltas.push(DirectoryDelta {
            path: directory.path.clone(),
            base_score,
            head_score: directory.score,
            delta_score: directory.score - base_score,
        });
    }
    deltas.sort_by(|left, right| {
        right
            .delta_score
            .abs()
            .partial_cmp(&left.delta_score.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    deltas
}

fn compare_workloads(base: &ProjectProfile, head: &ProjectProfile) -> Vec<WorkloadDelta> {
    let base_map: BTreeMap<&str, Option<f64>> = base
        .workloads
        .iter()
        .map(|workload| {
            (
                workload.spec.name.as_str(),
                workload.result.as_ref().map(|result| result.stats.mean_ms),
            )
        })
        .collect();
    let mut deltas = Vec::new();
    for workload in &head.workloads {
        let head_mean = workload.result.as_ref().map(|result| result.stats.mean_ms);
        let base_mean = base_map
            .get(workload.spec.name.as_str())
            .copied()
            .unwrap_or(None);
        deltas.push(WorkloadDelta {
            name: workload.spec.name.clone(),
            base_mean_ms: base_mean,
            head_mean_ms: head_mean,
            delta_mean_ms: match (base_mean, head_mean) {
                (Some(base_mean), Some(head_mean)) => Some(head_mean - base_mean),
                _ => None,
            },
        });
    }
    deltas.sort_by(|left, right| {
        right
            .delta_mean_ms
            .unwrap_or(0.0)
            .abs()
            .partial_cmp(&left.delta_mean_ms.unwrap_or(0.0).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    deltas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        BenchmarkMode, CommandProfile, DirectorySummary, ProjectKindSummary, TimingStats,
        WorkloadProfile, WorkloadSpec,
    };
    use std::path::PathBuf;

    fn sample_profile(score: f64) -> ProjectProfile {
        ProjectProfile {
            schema_version: "2".into(),
            root: PathBuf::from("/tmp/demo"),
            generated_unix_ms: 0,
            scan_duration_ms: 0,
            total_files: 1,
            total_profiled_files: 1,
            total_lines: 10,
            total_bytes: 100,
            skipped_files: 0,
            warnings: Vec::new(),
            detected_projects: vec![ProjectKindSummary {
                kind: "Rust".into(),
                evidence: "Cargo.toml".into(),
                default_runner: Some("cargo".into()),
            }],
            languages: Vec::new(),
            directories: vec![DirectorySummary {
                path: "src".into(),
                files: 1,
                lines: 10,
                bytes: 100,
                score,
                dominant_language: Some("Rust".into()),
                top_path: Some("src/main.rs".into()),
            }],
            hotspots: vec![crate::model::Hotspot {
                rank: 1,
                path: "src/main.rs".into(),
                language: "Rust".into(),
                score,
                lines: 10,
                bytes: 100,
                functions: 1,
                branches: 0,
                loops: 0,
                allocations: 0,
                blocking_io: 0,
                async_markers: 0,
                test_markers: 0,
                max_line_chars: 20,
                reasons: vec!["baseline complexity".into()],
            }],
            workloads: vec![WorkloadProfile {
                spec: WorkloadSpec {
                    name: "cargo-check".into(),
                    kind: "build".into(),
                    command: vec!["cargo".into(), "check".into()],
                    cwd: None,
                    description: "test".into(),
                    detected_from: "Cargo.toml".into(),
                    priority: 0,
                },
                result: Some(CommandProfile {
                    command: vec!["cargo".into(), "check".into()],
                    cwd: None,
                    stats: TimingStats {
                        iterations: 1,
                        warmups: 0,
                        min_ms: score,
                        mean_ms: score,
                        median_ms: score,
                        p95_ms: score,
                        max_ms: score,
                        setup_ms: 0.0,
                    },
                    exit_code: Some(0),
                    stdout_preview: String::new(),
                    stderr_preview: String::new(),
                }),
                status: BenchmarkMode::Off.label().into(),
            }],
        }
    }

    #[test]
    fn compare_report_orders_largest_delta_first() {
        let base = sample_profile(10.0);
        let head = sample_profile(30.0);
        let deltas = compare_hotspots(&base, &head);
        assert_eq!(deltas[0].path, "src/main.rs");
        assert_eq!(deltas[0].delta_score, 20.0);
    }
}
