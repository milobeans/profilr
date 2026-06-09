use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::{
    model::{BenchmarkMode, ProjectKindSummary, WorkloadProfile, WorkloadSpec},
    snippet::{profile_command, CommandOptions},
};

#[derive(Clone, Debug)]
pub struct BenchmarkOptions {
    pub mode: BenchmarkMode,
    pub iterations: usize,
    pub warmups: usize,
    pub auto_limit: usize,
}

pub fn detect_projects(root: &Path) -> Vec<ProjectKindSummary> {
    let candidates = [
        ("Rust", "Cargo.toml", Some("cargo")),
        ("Node", "package.json", Some("node")),
        ("Python", "pyproject.toml", Some("python3")),
        ("Python", "requirements.txt", Some("python3")),
        ("Go", "go.mod", Some("go")),
        ("Java", "pom.xml", Some("mvn")),
        ("Java", "build.gradle", Some("gradle")),
        ("Java", "build.gradle.kts", Some("gradle")),
        ("Ruby", "Gemfile", Some("ruby")),
        ("PHP", "composer.json", Some("php")),
        ("Swift", "Package.swift", Some("swift")),
        ("Make", "Makefile", Some("make")),
    ];

    let mut projects = Vec::new();
    for (kind, evidence, runner) in candidates {
        if root.join(evidence).is_file() {
            projects.push(ProjectKindSummary {
                kind: kind.to_string(),
                evidence: evidence.to_string(),
                default_runner: runner.map(str::to_string),
            });
        }
    }
    projects
}

pub fn detect_workloads(root: &Path, max_detected: usize) -> Vec<WorkloadProfile> {
    let mut workloads = Vec::new();

    if root.join("Cargo.toml").is_file() {
        push_workload(
            &mut workloads,
            "cargo-check",
            "build",
            ["cargo", "check"],
            "Rust dependency and compile check",
            "Cargo.toml",
            0,
        );
        push_workload(
            &mut workloads,
            "cargo-test",
            "test",
            ["cargo", "test"],
            "Rust test suite timing",
            "Cargo.toml",
            2,
        );
        if root.join("benches").is_dir() {
            push_workload(
                &mut workloads,
                "cargo-bench",
                "bench",
                ["cargo", "bench"],
                "Rust benchmark target",
                "benches/",
                4,
            );
        }
    }

    if let Some(package_json) = read_json(root.join("package.json")) {
        let scripts = package_json
            .get("scripts")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (script, priority) in [("build", 0), ("test", 2), ("lint", 3), ("start", 4)] {
            if scripts.contains_key(script) {
                let manager = package_manager(root);
                push_workload(
                    &mut workloads,
                    &format!("{manager}-{script}"),
                    script,
                    [manager.as_str(), "run", script],
                    &format!("Node `{script}` script"),
                    "package.json",
                    priority,
                );
            }
        }
    }

    if root.join("pyproject.toml").is_file() || root.join("requirements.txt").is_file() {
        if root.join("tests").is_dir() || root.join("test").is_dir() {
            push_workload(
                &mut workloads,
                "pytest",
                "test",
                ["python3", "-m", "pytest"],
                "Python test suite timing",
                "pyproject.toml",
                2,
            );
        }
        push_workload(
            &mut workloads,
            "python-import",
            "smoke",
            ["python3", "-m", "compileall", "."],
            "Python compile/import smoke timing",
            "pyproject.toml",
            0,
        );
    }

    if root.join("go.mod").is_file() {
        push_workload(
            &mut workloads,
            "go-build",
            "build",
            ["go", "build", "./..."],
            "Go build timing",
            "go.mod",
            0,
        );
        push_workload(
            &mut workloads,
            "go-test",
            "test",
            ["go", "test", "./..."],
            "Go test timing",
            "go.mod",
            2,
        );
    }

    if root.join("pom.xml").is_file() {
        push_workload(
            &mut workloads,
            "maven-test",
            "test",
            ["mvn", "-q", "test"],
            "Maven test timing",
            "pom.xml",
            2,
        );
    }

    if root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file() {
        let gradle = if root.join("gradlew").is_file() {
            "./gradlew"
        } else {
            "gradle"
        };
        push_workload(
            &mut workloads,
            "gradle-test",
            "test",
            [gradle, "test"],
            "Gradle test timing",
            "build.gradle",
            2,
        );
    }

    if root.join("Gemfile").is_file() {
        push_workload(
            &mut workloads,
            "bundle-rspec",
            "test",
            ["bundle", "exec", "rspec"],
            "Ruby test timing",
            "Gemfile",
            2,
        );
    }

    if root.join("composer.json").is_file() {
        push_workload(
            &mut workloads,
            "composer-test",
            "test",
            ["composer", "test"],
            "PHP Composer test timing",
            "composer.json",
            2,
        );
    }

    if root.join("Package.swift").is_file() {
        push_workload(
            &mut workloads,
            "swift-build",
            "build",
            ["swift", "build"],
            "Swift build timing",
            "Package.swift",
            0,
        );
        push_workload(
            &mut workloads,
            "swift-test",
            "test",
            ["swift", "test"],
            "Swift test timing",
            "Package.swift",
            2,
        );
    }

    if root.join("Makefile").is_file() {
        let makefile = fs::read_to_string(root.join("Makefile")).unwrap_or_default();
        for (target, priority) in [("check", 3), ("test", 4), ("bench", 5)] {
            if make_target_exists(&makefile, target) {
                push_workload(
                    &mut workloads,
                    &format!("make-{target}"),
                    target,
                    ["make", target],
                    &format!("Make `{target}` target timing"),
                    "Makefile",
                    priority,
                );
            }
        }
    }
    workloads.sort_by(|left, right| {
        left.spec
            .priority
            .cmp(&right.spec.priority)
            .then_with(|| left.spec.name.cmp(&right.spec.name))
    });
    workloads.truncate(max_detected.min(workloads.len()));
    workloads
}

pub fn benchmark_workloads(
    root: &Path,
    workloads: &mut [WorkloadProfile],
    options: &BenchmarkOptions,
) -> Result<()> {
    let selected_indexes = select_workloads(workloads, options.mode, options.auto_limit);
    for index in selected_indexes {
        let spec = workloads[index].spec.clone();
        workloads[index].status = "running".into();
        let command = profile_command(CommandOptions {
            command: spec.command.clone(),
            iterations: options.iterations,
            warmups: options.warmups,
            cwd: Some(spec.cwd.clone().unwrap_or_else(|| root.to_path_buf())),
        })
        .with_context(|| format!("benchmark workload {}", spec.name))?;
        workloads[index].status = "benchmarked".into();
        workloads[index].result = Some(command);
    }
    Ok(())
}

pub fn benchmark_single_workload(
    root: &Path,
    workload: &mut WorkloadProfile,
    iterations: usize,
    warmups: usize,
) -> Result<()> {
    let spec = workload.spec.clone();
    workload.status = "running".into();
    let command = profile_command(CommandOptions {
        command: spec.command.clone(),
        iterations,
        warmups,
        cwd: Some(spec.cwd.clone().unwrap_or_else(|| root.to_path_buf())),
    })
    .with_context(|| format!("benchmark workload {}", spec.name))?;
    workload.status = "benchmarked".into();
    workload.result = Some(command);
    Ok(())
}

fn push_workload<const N: usize>(
    workloads: &mut Vec<WorkloadProfile>,
    name: &str,
    kind: &str,
    command: [&str; N],
    description: &str,
    detected_from: &str,
    priority: usize,
) {
    workloads.push(WorkloadProfile {
        spec: WorkloadSpec {
            name: name.to_string(),
            kind: kind.to_string(),
            command: command.into_iter().map(str::to_string).collect(),
            cwd: None,
            description: description.to_string(),
            detected_from: detected_from.to_string(),
            priority,
        },
        result: None,
        status: "detected".into(),
    });
}

fn read_json(path: PathBuf) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn package_manager(root: &Path) -> String {
    if root.join("pnpm-lock.yaml").is_file() {
        "pnpm".into()
    } else if root.join("yarn.lock").is_file() {
        "yarn".into()
    } else {
        "npm".into()
    }
}

fn make_target_exists(makefile: &str, target: &str) -> bool {
    let prefix = format!("{target}:");
    makefile
        .lines()
        .any(|line| line.trim_start().starts_with(&prefix))
}

fn select_workloads(
    workloads: &[WorkloadProfile],
    mode: BenchmarkMode,
    auto_limit: usize,
) -> Vec<usize> {
    match mode {
        BenchmarkMode::Off => Vec::new(),
        BenchmarkMode::All => (0..workloads.len()).collect(),
        BenchmarkMode::Auto => workloads
            .iter()
            .enumerate()
            .filter(|(_, workload)| workload.spec.priority == 0)
            .map(|(index, _)| index)
            .take(auto_limit.max(1))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::*;

    #[test]
    fn detects_rust_workloads() {
        let root = env::temp_dir().join(format!("profilr-workload-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create root");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .expect("write manifest");
        let workloads = detect_workloads(&root, 8);
        let _ = fs::remove_dir_all(&root);
        assert!(workloads
            .iter()
            .any(|workload| workload.spec.name == "cargo-check"));
        assert!(workloads
            .iter()
            .any(|workload| workload.spec.name == "cargo-test"));
    }
}
