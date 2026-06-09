# profilr

`profilr` is a fast Rust CLI/TUI for profiling projects, commands, and code snippets.

The default command profiles the current project and opens an interactive terminal UI when stdout is a terminal:

```bash
profilr
```

The CLI commands are deterministic and JSON-friendly for agents:

```bash
profilr run --json
profilr report --format markdown --output profilr-report.md
profilr snippet --language python --code 'sum(i * i for i in range(10000))'
profilr command --iterations 5 -- cargo test
profilr doctor --json
```

## What Default Profiling Means

Universal runtime profiling is not possible without a workload. The default project profiler is a fast hotspot analysis that walks the codebase with `.gitignore` support and ranks files by language-aware signals:

- file size and line count
- branch, loop, function, allocation, async, and blocking I/O markers
- language summaries and top ranked hotspot files

Use `profilr command -- ...` when you have a real workload to time. Use `profilr snippet` for small code experiments.

## TUI

Run `profilr` or `profilr tui` to open the terminal UI.

Keys:

- `j` / `k` or arrow keys: move selection
- `/`: filter by path, language, or reason
- `s`: rotate sort order
- `Enter` / `o`: open the selected file in `$VISUAL` or `$EDITOR`
- `Ctrl-U`: clear filter
- `?`: help
- `q`, `Esc`, or `Ctrl-C`: quit

## CLI Surface

```bash
profilr [PATH]
profilr run [PATH] [--json] [--format pretty|json|markdown|csv]
profilr tui [PATH]
profilr report [PATH] --format markdown --output report.md
profilr snippet --language python --code 'print(sum(range(1000)))'
profilr snippet --file bench.py --iterations 25 --warmups 3 --json
profilr command --iterations 5 -- cargo test
profilr doctor --json
profilr config --print-default
profilr tools languages --json
profilr tools runners
```

`--json` emits stable machine-readable output for `run`, `report`, `snippet`, `command`, `doctor`, and `tools`.

Errors under `--json` use this shape:

```json
{
  "ok": false,
  "error": "message"
}
```

## Language Support

Project hotspot analysis recognizes Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, Swift, C, C++, C#, Ruby, PHP, Shell, SQL, R, Scala, Dart, Lua, Julia, Elixir, Erlang, and Haskell.

Snippet timing is available when the matching local runner is installed. Check support with:

```bash
profilr tools runners
```

## Config

`profilr` reads config in this order:

1. `--config PATH`
2. `.profilr.toml` in the profiled project
3. `~/.profilr/config.toml`
4. built-in defaults

Create a starter config:

```bash
profilr config --print-default > .profilr.toml
```

## Install

```bash
cargo install profilr
```

For local development:

```bash
make install-local
profilr doctor
```

## Development

```bash
make check
```

The check target runs formatting, clippy, tests, release build, and package validation.
