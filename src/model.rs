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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectProfile {
    pub root: PathBuf,
    pub generated_unix_ms: u128,
    pub scan_duration_ms: u128,
    pub total_files: usize,
    pub total_profiled_files: usize,
    pub total_lines: usize,
    pub total_bytes: u64,
    pub skipped_files: usize,
    pub warnings: Vec<String>,
    pub languages: Vec<LanguageSummary>,
    pub hotspots: Vec<Hotspot>,
}

impl ProjectProfile {
    pub fn sorted_hotspots(&self, sort: SortKey, limit: usize) -> Vec<&Hotspot> {
        let mut hotspots: Vec<&Hotspot> = self.hotspots.iter().collect();
        sort_hotspots(&mut hotspots, sort);
        hotspots.truncate(limit.min(hotspots.len()));
        hotspots
    }
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
    pub stats: TimingStats,
    pub exit_code: Option<i32>,
    pub stdout_preview: String,
    pub stderr_preview: String,
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
    pub checks: Vec<DoctorCheck>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    pub details: String,
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
