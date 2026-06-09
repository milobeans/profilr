use std::path::PathBuf;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(
    Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum SortKey {
    #[default]
    Score,
    Time,
    Complexity,
    Size,
    Lines,
    Language,
    Path,
}

impl SortKey {
    pub fn label(self) -> &'static str {
        match self {
            Self::Score => "score",
            Self::Time => "time",
            Self::Complexity => "complexity",
            Self::Size => "size",
            Self::Lines => "lines",
            Self::Language => "language",
            Self::Path => "path",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkMode {
    #[default]
    Off,
    Auto,
    All,
}

impl BenchmarkMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::All => "all",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectProfile {
    pub schema_version: String,
    pub root: PathBuf,
    pub generated_unix_ms: u128,
    pub scan_duration_ms: u128,
    pub total_files: usize,
    pub total_profiled_files: usize,
    pub total_lines: usize,
    pub total_bytes: u64,
    pub skipped_files: usize,
    pub warnings: Vec<String>,
    pub detected_projects: Vec<ProjectKindSummary>,
    pub languages: Vec<LanguageSummary>,
    pub directories: Vec<DirectorySummary>,
    pub hotspots: Vec<Hotspot>,
    pub workloads: Vec<WorkloadProfile>,
}

impl ProjectProfile {
    pub fn sorted_hotspots(&self, sort: SortKey, limit: usize) -> Vec<&Hotspot> {
        let mut hotspots: Vec<&Hotspot> = self.hotspots.iter().collect();
        sort_hotspots(&mut hotspots, sort);
        hotspots.truncate(limit.min(hotspots.len()));
        hotspots
    }

    pub fn sorted_directories(&self, limit: usize) -> Vec<&DirectorySummary> {
        let mut directories: Vec<&DirectorySummary> = self.directories.iter().collect();
        directories.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.path.cmp(&right.path))
        });
        directories.truncate(limit.min(directories.len()));
        directories
    }

    pub fn benchmarked_workloads(&self) -> usize {
        self.workloads
            .iter()
            .filter(|workload| workload.result.is_some())
            .count()
    }

    pub fn total_workload_time_ms(&self) -> f64 {
        self.workloads
            .iter()
            .filter_map(|workload| workload.result.as_ref())
            .map(|result| result.stats.mean_ms)
            .sum()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectKindSummary {
    pub kind: String,
    pub evidence: String,
    pub default_runner: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LanguageSummary {
    pub language: String,
    pub files: usize,
    pub lines: usize,
    pub bytes: u64,
    pub score: f64,
    pub top_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirectorySummary {
    pub path: String,
    pub files: usize,
    pub lines: usize,
    pub bytes: u64,
    pub score: f64,
    pub dominant_language: Option<String>,
    pub top_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hotspot {
    pub rank: usize,
    pub path: String,
    pub language: String,
    pub score: f64,
    pub lines: usize,
    pub bytes: u64,
    pub functions: usize,
    pub branches: usize,
    pub loops: usize,
    pub allocations: usize,
    pub blocking_io: usize,
    pub async_markers: usize,
    pub test_markers: usize,
    pub max_line_chars: usize,
    pub reasons: Vec<String>,
}

impl Hotspot {
    pub fn complexity_markers(&self) -> usize {
        self.functions + self.branches + (self.loops * 2)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdapterStatus {
    pub language: String,
    pub available: bool,
    pub command: Option<String>,
    pub notes: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimingStats {
    pub iterations: usize,
    pub warmups: usize,
    pub min_ms: f64,
    pub mean_ms: f64,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    pub setup_ms: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnippetProfile {
    pub language: String,
    pub runner: String,
    pub stats: TimingStats,
    pub stdout_preview: String,
    pub stderr_preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandProfile {
    pub command: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub stats: TimingStats,
    pub exit_code: Option<i32>,
    pub stdout_preview: String,
    pub stderr_preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkloadSpec {
    pub name: String,
    pub kind: String,
    pub command: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub description: String,
    pub detected_from: String,
    pub priority: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkloadProfile {
    pub spec: WorkloadSpec,
    pub result: Option<CommandProfile>,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DoctorReport {
    pub binary: String,
    pub version: String,
    pub cwd: PathBuf,
    pub config_source: Option<PathBuf>,
    pub auth_required: bool,
    pub default_mode: String,
    pub runners: Vec<AdapterStatus>,
    pub detected_projects: Vec<ProjectKindSummary>,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    pub details: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompareReport {
    pub base: PathBuf,
    pub head: PathBuf,
    pub hotspot_deltas: Vec<HotspotDelta>,
    pub directory_deltas: Vec<DirectoryDelta>,
    pub workload_deltas: Vec<WorkloadDelta>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HotspotDelta {
    pub path: String,
    pub base_score: f64,
    pub head_score: f64,
    pub delta_score: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirectoryDelta {
    pub path: String,
    pub base_score: f64,
    pub head_score: f64,
    pub delta_score: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkloadDelta {
    pub name: String,
    pub base_mean_ms: Option<f64>,
    pub head_mean_ms: Option<f64>,
    pub delta_mean_ms: Option<f64>,
}

pub fn sort_hotspots(hotspots: &mut [&Hotspot], sort: SortKey) {
    hotspots.sort_by(|left, right| compare_hotspots(left, right, sort));
}

fn compare_hotspots(left: &Hotspot, right: &Hotspot, sort: SortKey) -> std::cmp::Ordering {
    match sort {
        SortKey::Score => right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.path.cmp(&right.path)),
        SortKey::Time => right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.path.cmp(&right.path)),
        SortKey::Complexity => right
            .complexity_markers()
            .cmp(&left.complexity_markers())
            .then_with(|| left.path.cmp(&right.path)),
        SortKey::Size => right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.path.cmp(&right.path)),
        SortKey::Lines => right
            .lines
            .cmp(&left.lines)
            .then_with(|| left.path.cmp(&right.path)),
        SortKey::Language => left.language.cmp(&right.language).then_with(|| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        SortKey::Path => left.path.cmp(&right.path),
    }
}
