use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct V8CpuProfile {
    pub nodes: Vec<V8ProfileNode>,
    #[serde(rename = "startTime")]
    pub start_time: f64,
    #[serde(rename = "endTime")]
    pub end_time: f64,
    pub samples: Option<Vec<u32>>,
    #[serde(rename = "timeDeltas")]
    pub time_deltas: Option<Vec<f64>>,
}

#[derive(Deserialize, Debug)]
pub struct V8ProfileNode {
    pub id: u32,
    #[serde(rename = "callFrame")]
    pub call_frame: V8CallFrame,
    #[serde(rename = "hitCount")]
    pub hit_count: u32,
    pub children: Option<Vec<u32>>,
}

#[derive(Deserialize, Debug)]
pub struct V8CallFrame {
    #[serde(rename = "functionName")]
    pub function_name: String,
    pub url: String,
    #[serde(rename = "lineNumber")]
    pub line_number: i32,
    #[serde(rename = "columnNumber")]
    pub column_number: i32,
}

#[derive(Deserialize, Debug)]
pub struct PythonProfileRecord {
    pub file: String,
    pub line: usize,
    pub func: String,
    pub calls: usize,
    pub total_time: f64, // self time in seconds
    pub cum_time: f64,   // cumulative time in seconds
}

/// Clean profile paths to make them relative to the project root.
pub fn clean_profile_path(url: &str, root: &Path) -> Option<String> {
    if url.is_empty()
        || url.starts_with("node:")
        || url.starts_with("<built-in>")
        || url.starts_with("~")
        || url.contains("node_modules")
        || url.contains("lib/python")
    {
        return None;
    }

    let clean_url = url.trim_start_matches("file://").to_string();
    let path = Path::new(&clean_url);

    if path.is_absolute() {
        if let Ok(rel) = path.strip_prefix(root) {
            return Some(rel.to_string_lossy().replace('\\', "/"));
        }
    } else {
        let resolved = root.join(path);
        if resolved.exists() {
            if let Ok(rel) = resolved.strip_prefix(root) {
                return Some(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    let root_str = root.to_string_lossy().replace('\\', "/");
    let url_normalized = clean_url.replace('\\', "/");
    if url_normalized.starts_with(&root_str) {
        let rel = &url_normalized[root_str.len()..];
        return Some(rel.trim_start_matches('/').to_string());
    }

    if root.join(&url_normalized).is_file() {
        return Some(url_normalized);
    }

    None
}

/// Parse a V8 .cpuprofile file and attribute self-durations (in milliseconds) to files.
pub fn parse_v8_profile(path: &Path, root: &Path) -> Result<HashMap<String, f64>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("read V8 profile {}", path.display()))?;
    let profile: V8CpuProfile =
        serde_json::from_str(&content).context("parse V8 cpuprofile JSON")?;

    let mut node_durations = HashMap::new();
    let total_duration_us = profile.end_time - profile.start_time;
    let total_duration_ms = total_duration_us / 1000.0;

    if let (Some(samples), Some(deltas)) = (&profile.samples, &profile.time_deltas) {
        for (sample_idx, &node_id) in samples.iter().enumerate() {
            let delta_us = deltas.get(sample_idx).copied().unwrap_or(0.0);
            *node_durations.entry(node_id).or_insert(0.0) += delta_us / 1000.0;
        }
    } else {
        let total_hits: u32 = profile.nodes.iter().map(|n| n.hit_count).sum();
        if total_hits > 0 {
            let ms_per_hit = total_duration_ms / total_hits as f64;
            for node in &profile.nodes {
                node_durations.insert(node.id, node.hit_count as f64 * ms_per_hit);
            }
        }
    }

    let mut file_durations = HashMap::new();
    for node in &profile.nodes {
        if let Some(&duration) = node_durations.get(&node.id) {
            if duration <= 0.0 {
                continue;
            }
            if let Some(clean_path) = clean_profile_path(&node.call_frame.url, root) {
                *file_durations.entry(clean_path).or_insert(0.0) += duration;
            }
        }
    }
    Ok(file_durations)
}

/// Parse Python cProfile JSON dump and attribute self-durations (in milliseconds) to files.
pub fn parse_python_json(content: &str, root: &Path) -> Result<HashMap<String, f64>> {
    let records: Vec<PythonProfileRecord> =
        serde_json::from_str(content).context("parse python cProfile stats JSON")?;
    let mut file_durations = HashMap::new();
    for record in records {
        if record.total_time <= 0.0 {
            continue;
        }
        if let Some(clean_path) = clean_profile_path(&record.file, root) {
            let ms = record.total_time * 1000.0;
            *file_durations.entry(clean_path).or_insert(0.0) += ms;
        }
    }
    Ok(file_durations)
}

/// Helper command to run python to convert binary pstats format to JSON.
pub fn convert_pstats_to_json(pstats_path: &Path) -> Result<String> {
    let python_exec = if crate::snippet::command_available("python3") {
        "python3"
    } else if crate::snippet::command_available("python") {
        "python"
    } else {
        anyhow::bail!("python3 or python is required to parse cProfile traces");
    };

    let script = "import pstats, json, sys; stats = pstats.Stats(sys.argv[1]); print(json.dumps([{'file': k[0], 'line': k[1], 'func': k[2], 'calls': v[1], 'total_time': v[2], 'cum_time': v[3]} for k, v in stats.stats.items()]))";

    let output = Command::new(python_exec)
        .arg("-c")
        .arg(script)
        .arg(pstats_path)
        .output()
        .context("run python helper to parse pstats")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to parse python profile: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Ingest any generic trace file (V8 CPU profile or Python JSON/pstats) and return runtime attribution.
pub fn ingest_trace_file(path: &Path, root: &Path) -> Result<HashMap<String, f64>> {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    if extension == "cpuprofile" {
        return parse_v8_profile(path, root);
    }

    // Attempt to read as text JSON
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(v8) = parse_v8_profile(path, root) {
            return Ok(v8);
        }
        if let Ok(python) = parse_python_json(&content, root) {
            return Ok(python);
        }
    }

    // Try treating as a python binary pstats file
    if let Ok(json_str) = convert_pstats_to_json(path) {
        if let Ok(python) = parse_python_json(&json_str, root) {
            return Ok(python);
        }
    }

    anyhow::bail!("unsupported trace file format: {}", path.display())
}

/// Profiles a workload command under ecosystem-specific collectors and returns runtime file attribution.
pub fn profile_command_ecosystem(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    root: &Path,
) -> Result<HashMap<String, f64>> {
    let prog_lower = program.to_lowercase();
    let is_python = prog_lower.contains("python")
        || prog_lower.contains("pytest")
        || root.join("pyproject.toml").is_file()
        || root.join("requirements.txt").is_file();
    let is_node = prog_lower.contains("node")
        || prog_lower.contains("npm")
        || prog_lower.contains("pnpm")
        || prog_lower.contains("yarn")
        || prog_lower.contains("tsx")
        || root.join("package.json").is_file();

    if is_node {
        let temp_dir = tempfile_dir()?;
        let mut child = Command::new(program);
        child.args(args);
        if let Some(c) = cwd {
            child.current_dir(c);
        }
        child.env(
            "NODE_OPTIONS",
            format!("--cpu-prof --cpu-prof-dir={}", temp_dir.display()),
        );
        child.stdin(Stdio::null());

        let output = child
            .output()
            .context("execute Node.js command with cpu profiling")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "Node workload exited with warning/error (code {:?}): {}",
                output.status.code(),
                stderr
            );
        }

        // Search the directory for cpuprofile files
        let mut profile_file = None;
        if let Ok(entries) = fs::read_dir(&temp_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "cpuprofile") {
                    profile_file = Some(path);
                    break;
                }
            }
        }

        let result = if let Some(ref path) = profile_file {
            parse_v8_profile(path, root)
        } else {
            Ok(HashMap::new())
        };

        let _ = fs::remove_dir_all(temp_dir);
        result
    } else if is_python {
        let temp_dir = tempfile_dir()?;
        let pstats_file = temp_dir.join("profile.pstats");

        // Transform the command to run cProfile
        let final_program;
        let mut final_args = Vec::new();

        let python_executable = if crate::snippet::command_available("python3") {
            "python3"
        } else {
            "python"
        };

        if prog_lower.contains("python") {
            final_program = program.to_string();
            // insert -m cProfile -o <file> after executable
            final_args.push("-m".to_string());
            final_args.push("cProfile".to_string());
            final_args.push("-o".to_string());
            final_args.push(pstats_file.to_string_lossy().into_owned());
            final_args.extend(args.iter().cloned());
        } else if prog_lower.contains("pytest") {
            final_program = python_executable.to_string();
            final_args.push("-m".to_string());
            final_args.push("cProfile".to_string());
            final_args.push("-o".to_string());
            final_args.push(pstats_file.to_string_lossy().into_owned());
            final_args.push("-m".to_string());
            final_args.push("pytest".to_string());
            final_args.extend(args.iter().cloned());
        } else {
            // It was matched due to project shape. Use python runner.
            final_program = python_executable.to_string();
            final_args.push("-m".to_string());
            final_args.push("cProfile".to_string());
            final_args.push("-o".to_string());
            final_args.push(pstats_file.to_string_lossy().into_owned());

            // if we are running something like "pytest", we want to run it via module
            if program == "pytest" {
                final_args.push("-m".to_string());
                final_args.push("pytest".to_string());
                final_args.extend(args.iter().cloned());
            } else {
                final_args.push(program.to_string());
                final_args.extend(args.iter().cloned());
            }
        }

        let mut child = Command::new(&final_program);
        child.args(&final_args);
        if let Some(c) = cwd {
            child.current_dir(c);
        }
        child.stdin(Stdio::null());

        let output = child
            .output()
            .context("execute Python command with cProfile")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "Python workload exited with warning/error (code {:?}): {}",
                output.status.code(),
                stderr
            );
        }

        let result = if pstats_file.exists() {
            let json_str = convert_pstats_to_json(&pstats_file)?;
            parse_python_json(&json_str, root)
        } else {
            Ok(HashMap::new())
        };

        let _ = fs::remove_dir_all(temp_dir);
        result
    } else {
        // Fallback for non-supported ecosystems: just run the command normally without profiling.
        let mut child = Command::new(program);
        child.args(args);
        if let Some(c) = cwd {
            child.current_dir(c);
        }
        child.stdin(Stdio::null());
        let _ = child.output();
        Ok(HashMap::new())
    }
}

fn tempfile_dir() -> Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("profilr-collector-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&path).with_context(|| format!("create temp dir {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_clean_profile_path() {
        let root = Path::new("/Users/test/project");
        assert_eq!(
            clean_profile_path("file:///Users/test/project/src/main.rs", root),
            Some("src/main.rs".to_string())
        );
        assert_eq!(
            clean_profile_path("/Users/test/project/src/main.rs", root),
            Some("src/main.rs".to_string())
        );
        assert_eq!(
            clean_profile_path("node:internal/modules/cjs/loader", root),
            None
        );
        assert_eq!(
            clean_profile_path("/Users/test/project/node_modules/lodash/index.js", root),
            None
        );
    }

    #[test]
    fn test_parse_v8_profile() {
        let temp_dir = std::env::temp_dir().join(format!("profilr-test-v8-{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let profile_json = r#"{
            "nodes": [
                {
                    "id": 1,
                    "callFrame": {
                        "functionName": "foo",
                        "url": "file:///Users/test/project/src/foo.js",
                        "lineNumber": 10,
                        "columnNumber": 5
                    },
                    "hitCount": 5
                },
                {
                    "id": 2,
                    "callFrame": {
                        "functionName": "bar",
                        "url": "file:///Users/test/project/src/bar.js",
                        "lineNumber": 20,
                        "columnNumber": 2
                    },
                    "hitCount": 10
                }
            ],
            "startTime": 1000.0,
            "endTime": 2000.0
        }"#;
        let file_path = temp_dir.join("v8.cpuprofile");
        fs::write(&file_path, profile_json).unwrap();

        let root = Path::new("/Users/test/project");
        let results = parse_v8_profile(&file_path, root).unwrap();

        assert_eq!(results.len(), 2);
        assert!((results.get("src/foo.js").unwrap() - 0.333).abs() < 0.05);
        assert!((results.get("src/bar.js").unwrap() - 0.666).abs() < 0.05);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_parse_python_json() {
        let content = r#"[
            {
                "file": "/Users/test/project/app/server.py",
                "line": 15,
                "func": "handle",
                "calls": 2,
                "total_time": 0.12,
                "cum_time": 0.5
            },
            {
                "file": "/Users/test/project/app/db.py",
                "line": 30,
                "func": "query",
                "calls": 5,
                "total_time": 0.05,
                "cum_time": 0.05
            }
        ]"#;
        let root = Path::new("/Users/test/project");
        let results = parse_python_json(content, root).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results.get("app/server.py").unwrap(), &120.0);
        assert_eq!(results.get("app/db.py").unwrap(), &50.0);
    }
}
