use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use ignore::WalkBuilder;

use crate::{
    config::Config,
    languages::language_for_path,
    model::{Hotspot, LanguageSummary, ProjectProfile, SortKey},
};

pub fn profile_project(
    root: &Path,
    config: &Config,
    sort: SortKey,
    language_filter: &[String],
) -> Result<ProjectProfile> {
    let started = Instant::now();
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let allowed_languages = allowed_languages(config, language_filter);
    let mut warnings = Vec::new();
    let mut hotspots = Vec::new();
    let mut total_files = 0usize;
    let mut skipped_files = 0usize;

    let mut builder = WalkBuilder::new(&root);
    builder
        .hidden(!config.scan.include_hidden)
        .follow_links(config.scan.follow_links)
        .git_ignore(true)
        .git_exclude(true)
        .parents(true);
    if let Some(max_depth) = config.scan.max_depth {
        builder.max_depth(Some(max_depth));
    }

    let exclude_dirs = config.scan.exclude_dirs.clone();
    builder.filter_entry(move |entry| {
        if entry.depth() == 0 {
            return true;
        }
        let name = entry.file_name().to_string_lossy();
        !exclude_dirs
            .iter()
            .any(|excluded| excluded.as_str() == name.as_ref())
    });

    for entry in builder.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                warnings.push(err.to_string());
                continue;
            }
        };

        if !entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }

        total_files += 1;
        let Some(language) = language_for_path(entry.path()) else {
            continue;
        };
        if !allowed_languages.is_empty() && !allowed_languages.contains(language.name) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                skipped_files += 1;
                warnings.push(format!("{}: {err}", entry.path().display()));
                continue;
            }
        };
        if metadata.len() > config.scan.max_file_bytes {
            skipped_files += 1;
            warnings.push(format!(
                "{} skipped: {} bytes exceeds max_file_bytes {}",
                display_path(&root, entry.path()),
                metadata.len(),
                config.scan.max_file_bytes
            ));
            continue;
        }

        let raw = match fs::read(entry.path()) {
            Ok(raw) => raw,
            Err(err) => {
                skipped_files += 1;
                warnings.push(format!("{}: {err}", entry.path().display()));
                continue;
            }
        };

        let text = String::from_utf8_lossy(&raw);
        hotspots.push(analyze_file(
            &root,
            entry.path(),
            language.name,
            &text,
            metadata.len(),
        ));
    }

    rank_hotspots(&mut hotspots, sort);
    let languages = summarize_languages(&hotspots);
    let total_lines = hotspots.iter().map(|hotspot| hotspot.lines).sum();
    let total_bytes = hotspots.iter().map(|hotspot| hotspot.bytes).sum();

    Ok(ProjectProfile {
        root,
        generated_unix_ms: now_unix_ms(),
        scan_duration_ms: started.elapsed().as_millis(),
        total_files,
        total_profiled_files: hotspots.len(),
        total_lines,
        total_bytes,
        skipped_files,
        warnings,
        languages,
        hotspots,
    })
}

fn analyze_file(root: &Path, path: &Path, language: &str, text: &str, bytes: u64) -> Hotspot {
    let lines = text.lines().count();
    let max_line_chars = text
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let functions = function_count(language, text);
    let branches = count_words(
        text,
        &[
            "if", "else", "match", "switch", "case", "catch", "except", "when", "guard",
        ],
    ) + count_patterns(text, &["&&", "||", " ? "]);
    let loops = count_words(
        text,
        &["for", "while", "loop", "foreach", "map", "filter", "reduce"],
    );
    let allocations = count_patterns(
        text,
        &[
            "clone(",
            ".clone()",
            "collect(",
            ".collect()",
            "to_vec(",
            "Vec::new",
            "String::new",
            "Box::new",
            "new ",
            "JSON.parse",
            "JSON.stringify",
            "list(",
            "dict(",
            "Array(",
        ],
    );
    let blocking_io = count_patterns(
        text,
        &[
            "read_to_string",
            "fs::read",
            "File::open",
            "std::fs",
            "subprocess",
            "requests.",
            "urllib",
            "fetch(",
            "http.",
            "sleep(",
            "thread::sleep",
            "sqlite",
            "SELECT ",
            "INSERT ",
        ],
    );
    let async_markers = count_words(text, &["async", "await", "tokio", "spawn"]);
    let test_markers = count_patterns(text, &["#[test]", "describe(", "it(", "pytest", "unittest"]);

    let signals = FileSignals {
        lines,
        functions,
        branches,
        loops,
        allocations,
        blocking_io,
        async_markers,
        max_line_chars,
    };
    let score = score_file(signals);

    Hotspot {
        rank: 0,
        path: display_path(root, path),
        language: language.to_string(),
        score,
        lines,
        bytes,
        functions,
        branches,
        loops,
        allocations,
        blocking_io,
        async_markers,
        test_markers,
        max_line_chars,
        reasons: reasons(
            lines,
            branches,
            loops,
            allocations,
            blocking_io,
            async_markers,
            max_line_chars,
        ),
    }
}

#[derive(Copy, Clone, Debug)]
struct FileSignals {
    lines: usize,
    functions: usize,
    branches: usize,
    loops: usize,
    allocations: usize,
    blocking_io: usize,
    async_markers: usize,
    max_line_chars: usize,
}

fn score_file(signals: FileSignals) -> f64 {
    (signals.lines as f64 * 0.16)
        + (signals.functions as f64 * 1.5)
        + (signals.branches as f64 * 3.5)
        + (signals.loops as f64 * 7.0)
        + (signals.allocations as f64 * 3.0)
        + (signals.blocking_io as f64 * 9.0)
        + (signals.async_markers as f64 * 2.0)
        + (signals.max_line_chars.saturating_sub(120) as f64 * 0.1)
}

fn reasons(
    lines: usize,
    branches: usize,
    loops: usize,
    allocations: usize,
    blocking_io: usize,
    async_markers: usize,
    max_line_chars: usize,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if lines >= 400 {
        reasons.push("large file".into());
    }
    if branches >= 24 {
        reasons.push("branch-heavy".into());
    }
    if loops >= 8 {
        reasons.push("loop-heavy".into());
    }
    if allocations >= 10 {
        reasons.push("allocation markers".into());
    }
    if blocking_io >= 4 {
        reasons.push("blocking I/O markers".into());
    }
    if async_markers >= 8 {
        reasons.push("async/concurrency markers".into());
    }
    if max_line_chars >= 160 {
        reasons.push("very long lines".into());
    }
    if reasons.is_empty() {
        reasons.push("baseline complexity".into());
    }
    reasons
}

fn function_count(language: &str, text: &str) -> usize {
    match language {
        "Rust" => count_words(text, &["fn"]),
        "Python" => count_words(text, &["def", "class"]),
        "JavaScript" | "TypeScript" => {
            count_words(text, &["function", "class"]) + count_patterns(text, &["=>"])
        }
        "Go" => count_words(text, &["func"]),
        "Ruby" => count_words(text, &["def", "class", "module"]),
        "PHP" => count_words(text, &["function", "class"]),
        "Shell" => count_patterns(text, &["() {", "function "]),
        _ => count_words(text, &["function", "func", "def", "class"]),
    }
}

fn count_words(text: &str, words: &[&str]) -> usize {
    words.iter().map(|word| count_word(text, word)).sum()
}

fn count_word(text: &str, word: &str) -> usize {
    text.match_indices(word)
        .filter(|(index, _)| {
            let before = text[..*index].chars().next_back();
            let after = text[*index + word.len()..].chars().next();
            is_boundary(before) && is_boundary(after)
        })
        .count()
}

fn is_boundary(ch: Option<char>) -> bool {
    ch.map(|value| !value.is_ascii_alphanumeric() && value != '_')
        .unwrap_or(true)
}

fn count_patterns(text: &str, patterns: &[&str]) -> usize {
    patterns
        .iter()
        .map(|pattern| text.matches(pattern).count())
        .sum()
}

fn allowed_languages(config: &Config, language_filter: &[String]) -> BTreeSet<&'static str> {
    config
        .scan
        .languages
        .iter()
        .chain(language_filter.iter())
        .filter_map(|name| {
            crate::languages::LANGUAGES
                .iter()
                .find(|spec| spec.name.eq_ignore_ascii_case(name))
                .map(|spec| spec.name)
        })
        .collect()
}

fn summarize_languages(hotspots: &[Hotspot]) -> Vec<LanguageSummary> {
    let mut summaries: BTreeMap<String, LanguageSummary> = BTreeMap::new();
    for hotspot in hotspots {
        let summary = summaries
            .entry(hotspot.language.clone())
            .or_insert_with(|| LanguageSummary {
                language: hotspot.language.clone(),
                files: 0,
                lines: 0,
                bytes: 0,
                score: 0.0,
                top_path: None,
            });
        summary.files += 1;
        summary.lines += hotspot.lines;
        summary.bytes += hotspot.bytes;
        summary.score += hotspot.score;
        let should_replace = summary
            .top_path
            .as_ref()
            .and_then(|path| hotspots.iter().find(|candidate| &candidate.path == path))
            .map(|current| hotspot.score > current.score)
            .unwrap_or(true);
        if should_replace {
            summary.top_path = Some(hotspot.path.clone());
        }
    }

    let mut values: Vec<LanguageSummary> = summaries.into_values().collect();
    values.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    values
}

fn rank_hotspots(hotspots: &mut [Hotspot], sort: SortKey) {
    hotspots.sort_by(|left, right| match sort {
        SortKey::Score | SortKey::Time => right
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
    });

    for (index, hotspot) in hotspots.iter_mut().enumerate() {
        hotspot.rank = index + 1;
    }
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::*;

    #[test]
    fn scores_loop_and_io_heavier_than_plain_file() {
        let plain = score_file(FileSignals {
            lines: 50,
            functions: 2,
            branches: 2,
            loops: 0,
            allocations: 0,
            blocking_io: 0,
            async_markers: 0,
            max_line_chars: 80,
        });
        let hot = score_file(FileSignals {
            lines: 50,
            functions: 2,
            branches: 2,
            loops: 5,
            allocations: 0,
            blocking_io: 3,
            async_markers: 0,
            max_line_chars: 80,
        });
        assert!(hot > plain);
    }

    #[test]
    fn project_profile_detects_rust_hotspot() {
        let root = env::temp_dir().join(format!("profilr-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).expect("create temp project");
        fs::write(
            root.join("src/main.rs"),
            "fn main() { for value in 0..10 { if value > 3 { println!(\"{}\", value); } } }",
        )
        .expect("write source");

        let profile = profile_project(&root, &Config::default(), SortKey::Score, &[])
            .expect("profile temp project");
        let _ = fs::remove_dir_all(&root);

        assert_eq!(profile.total_profiled_files, 1);
        assert_eq!(profile.hotspots[0].language, "Rust");
        assert!(profile.hotspots[0].score > 0.0);
    }
}
