use std::path::Path;

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct LanguageSpec {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub filenames: &'static [&'static str],
    pub snippet_runner: Option<&'static str>,
}

pub const LANGUAGES: &[LanguageSpec] = &[
    LanguageSpec {
        name: "Rust",
        extensions: &["rs"],
        filenames: &[],
        snippet_runner: Some("rustc"),
    },
    LanguageSpec {
        name: "Python",
        extensions: &["py", "pyw"],
        filenames: &[],
        snippet_runner: Some("python3"),
    },
    LanguageSpec {
        name: "JavaScript",
        extensions: &["js", "mjs", "cjs", "jsx"],
        filenames: &[],
        snippet_runner: Some("node"),
    },
    LanguageSpec {
        name: "TypeScript",
        extensions: &["ts", "tsx", "mts", "cts"],
        filenames: &[],
        snippet_runner: Some("tsx"),
    },
    LanguageSpec {
        name: "Go",
        extensions: &["go"],
        filenames: &[],
        snippet_runner: Some("go"),
    },
    LanguageSpec {
        name: "Java",
        extensions: &["java"],
        filenames: &[],
        snippet_runner: Some("jshell"),
    },
    LanguageSpec {
        name: "Kotlin",
        extensions: &["kt", "kts"],
        filenames: &[],
        snippet_runner: Some("kotlinc"),
    },
    LanguageSpec {
        name: "Swift",
        extensions: &["swift"],
        filenames: &[],
        snippet_runner: Some("swift"),
    },
    LanguageSpec {
        name: "C",
        extensions: &["c", "h"],
        filenames: &[],
        snippet_runner: Some("cc"),
    },
    LanguageSpec {
        name: "C++",
        extensions: &["cc", "cpp", "cxx", "hpp", "hh", "hxx"],
        filenames: &[],
        snippet_runner: Some("c++"),
    },
    LanguageSpec {
        name: "C#",
        extensions: &["cs"],
        filenames: &[],
        snippet_runner: Some("dotnet-script"),
    },
    LanguageSpec {
        name: "Ruby",
        extensions: &["rb"],
        filenames: &["Gemfile", "Rakefile"],
        snippet_runner: Some("ruby"),
    },
    LanguageSpec {
        name: "PHP",
        extensions: &["php"],
        filenames: &[],
        snippet_runner: Some("php"),
    },
    LanguageSpec {
        name: "Shell",
        extensions: &["sh", "bash", "zsh", "fish"],
        filenames: &["Makefile"],
        snippet_runner: Some("sh"),
    },
    LanguageSpec {
        name: "SQL",
        extensions: &["sql"],
        filenames: &[],
        snippet_runner: None,
    },
    LanguageSpec {
        name: "R",
        extensions: &["r", "R"],
        filenames: &[],
        snippet_runner: Some("Rscript"),
    },
    LanguageSpec {
        name: "Scala",
        extensions: &["scala", "sc"],
        filenames: &[],
        snippet_runner: Some("scala"),
    },
    LanguageSpec {
        name: "Dart",
        extensions: &["dart"],
        filenames: &[],
        snippet_runner: Some("dart"),
    },
    LanguageSpec {
        name: "Lua",
        extensions: &["lua"],
        filenames: &[],
        snippet_runner: Some("lua"),
    },
    LanguageSpec {
        name: "Julia",
        extensions: &["jl"],
        filenames: &[],
        snippet_runner: Some("julia"),
    },
    LanguageSpec {
        name: "Elixir",
        extensions: &["ex", "exs"],
        filenames: &[],
        snippet_runner: Some("elixir"),
    },
    LanguageSpec {
        name: "Erlang",
        extensions: &["erl", "hrl"],
        filenames: &[],
        snippet_runner: Some("erl"),
    },
    LanguageSpec {
        name: "Haskell",
        extensions: &["hs", "lhs"],
        filenames: &[],
        snippet_runner: Some("runghc"),
    },
];

pub fn language_for_path(path: &Path) -> Option<&'static LanguageSpec> {
    let file_name = path.file_name()?.to_string_lossy();
    if let Some(spec) = LANGUAGES
        .iter()
        .find(|spec| spec.filenames.iter().any(|name| *name == file_name))
    {
        return Some(spec);
    }

    let extension = path.extension()?.to_string_lossy();
    LANGUAGES.iter().find(|spec| {
        spec.extensions
            .iter()
            .any(|candidate| *candidate == extension)
    })
}

pub fn language_by_name(name: &str) -> Option<&'static LanguageSpec> {
    LANGUAGES
        .iter()
        .find(|spec| spec.name.eq_ignore_ascii_case(name))
}

pub fn language_names() -> Vec<&'static str> {
    LANGUAGES.iter().map(|spec| spec.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_languages() {
        assert_eq!(
            language_for_path(Path::new("src/main.rs")).map(|spec| spec.name),
            Some("Rust")
        );
        assert_eq!(
            language_for_path(Path::new("app/server.py")).map(|spec| spec.name),
            Some("Python")
        );
        assert_eq!(
            language_for_path(Path::new("Makefile")).map(|spec| spec.name),
            Some("Shell")
        );
    }
}
