use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::{BenchmarkMode, SortKey};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub scan: ScanConfig,
    pub output: OutputConfig,
    pub workloads: WorkloadConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanConfig {
    pub max_file_bytes: u64,
    pub max_depth: Option<usize>,
    pub include_hidden: bool,
    pub follow_links: bool,
    pub exclude_dirs: Vec<String>,
    pub languages: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutputConfig {
    pub limit: usize,
    pub sort: SortKey,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkloadConfig {
    pub benchmark_mode: BenchmarkMode,
    pub max_detected: usize,
    pub auto_limit: usize,
    pub iterations: usize,
    pub warmups: usize,
}

#[derive(Clone, Debug)]
pub struct LoadedConfig {
    pub config: Config,
    pub source: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialConfig {
    scan: Option<PartialScanConfig>,
    output: Option<PartialOutputConfig>,
    workloads: Option<PartialWorkloadConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialScanConfig {
    max_file_bytes: Option<u64>,
    max_depth: Option<Option<usize>>,
    include_hidden: Option<bool>,
    follow_links: Option<bool>,
    exclude_dirs: Option<Vec<String>>,
    languages: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialOutputConfig {
    limit: Option<usize>,
    sort: Option<SortKey>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialWorkloadConfig {
    benchmark_mode: Option<BenchmarkMode>,
    max_detected: Option<usize>,
    auto_limit: Option<usize>,
    iterations: Option<usize>,
    warmups: Option<usize>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scan: ScanConfig {
                max_file_bytes: 768 * 1024,
                max_depth: Some(64),
                include_hidden: false,
                follow_links: false,
                exclude_dirs: vec![
                    ".git".into(),
                    ".hg".into(),
                    ".svn".into(),
                    ".idea".into(),
                    ".vscode".into(),
                    "target".into(),
                    "node_modules".into(),
                    ".venv".into(),
                    "venv".into(),
                    "__pycache__".into(),
                    ".pytest_cache".into(),
                    ".mypy_cache".into(),
                    ".ruff_cache".into(),
                    "dist".into(),
                    "build".into(),
                    ".next".into(),
                    ".turbo".into(),
                    "coverage".into(),
                    ".gradle".into(),
                    ".dart_tool".into(),
                    "vendor".into(),
                ],
                languages: Vec::new(),
            },
            output: OutputConfig {
                limit: 30,
                sort: SortKey::Score,
            },
            workloads: WorkloadConfig {
                benchmark_mode: BenchmarkMode::Off,
                max_detected: 8,
                auto_limit: 2,
                iterations: 3,
                warmups: 0,
            },
        }
    }
}

impl Config {
    pub fn default_toml() -> Result<String> {
        toml::to_string_pretty(&Self::default()).context("serialize default config")
    }

    fn merge(&mut self, partial: PartialConfig) {
        if let Some(scan) = partial.scan {
            if let Some(value) = scan.max_file_bytes {
                self.scan.max_file_bytes = value;
            }
            if let Some(value) = scan.max_depth {
                self.scan.max_depth = value;
            }
            if let Some(value) = scan.include_hidden {
                self.scan.include_hidden = value;
            }
            if let Some(value) = scan.follow_links {
                self.scan.follow_links = value;
            }
            if let Some(value) = scan.exclude_dirs {
                self.scan.exclude_dirs = value;
            }
            if let Some(value) = scan.languages {
                self.scan.languages = value;
            }
        }

        if let Some(output) = partial.output {
            if let Some(value) = output.limit {
                self.output.limit = value;
            }
            if let Some(value) = output.sort {
                self.output.sort = value;
            }
        }

        if let Some(workloads) = partial.workloads {
            if let Some(value) = workloads.benchmark_mode {
                self.workloads.benchmark_mode = value;
            }
            if let Some(value) = workloads.max_detected {
                self.workloads.max_detected = value;
            }
            if let Some(value) = workloads.auto_limit {
                self.workloads.auto_limit = value;
            }
            if let Some(value) = workloads.iterations {
                self.workloads.iterations = value.max(1);
            }
            if let Some(value) = workloads.warmups {
                self.workloads.warmups = value;
            }
        }
    }
}

pub fn load_config(explicit: Option<&Path>, root: &Path) -> Result<LoadedConfig> {
    let mut config = Config::default();
    let source = config_source(explicit, root);

    if let Some(path) = &source {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read config {}", path.display()))?;
        let partial: PartialConfig =
            toml::from_str(&raw).with_context(|| format!("parse config {}", path.display()))?;
        config.merge(partial);
    }

    Ok(LoadedConfig { config, source })
}

fn config_source(explicit: Option<&Path>, root: &Path) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }

    let project_config = root.join(".profilr.toml");
    if project_config.is_file() {
        return Some(project_config);
    }

    home_dir()
        .map(|home| home.join(".profilr").join("config.toml"))
        .filter(|path| path.is_file())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_contains_common_heavy_dirs() {
        let config = Config::default();
        assert!(config.scan.exclude_dirs.iter().any(|dir| dir == "target"));
        assert!(config
            .scan
            .exclude_dirs
            .iter()
            .any(|dir| dir == "node_modules"));
    }

    #[test]
    fn default_config_round_trips_as_toml() {
        let rendered = Config::default_toml().expect("default config renders");
        let parsed: PartialConfig = toml::from_str(&rendered).expect("rendered config parses");
        assert!(parsed.scan.is_some());
        assert!(parsed.output.is_some());
        assert!(parsed.workloads.is_some());
    }
}
