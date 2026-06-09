# profilr

`profilr` is a fast Rust CLI/TUI for profiling whole projects, commands, and code snippets.

The default command profiles the current project, detects likely workloads, and opens an interactive terminal UI when stdout is a terminal:

```bash
profilr
```

The CLI is deterministic and JSON-friendly for agents:

```bash
profilr run --json --limit 20
profilr report --format markdown --output profilr-report.md
profilr run --bench auto --save profilr-baseline.json
profilr compare profilr-baseline.json profilr-head.json
profilr workloads list --json
profilr workloads run --all --json
profilr snippet --language python --code 'sum(i * i for i in range(10000))'
profilr command --iterations 5 -- cargo test
profilr doctor --json
```

## What Default Profiling Means

Universal runtime profiling is not possible without a workload. `profilr` handles that in two layers:

- fast project-wide hotspot analysis with `.gitignore` support
- inferred workloads for common project types so real commands can be benchmarked on demand or via `--bench auto` / `--bench all`

The default project pass now reports:

- top hotspot files
- directory rollups
- language summaries
- detected project kinds
- inferred workloads and any benchmark results

## TUI

Run `profilr` or `profilr tui` to open the terminal UI.

Keys:

- `1-4` or `Tab` / `Shift-Tab`: switch tabs
- `j` / `k` or arrow keys: move selection
- `/`: filter the active view
- `s`: rotate hotspot sort order
- `Enter` / `o`: open the selected file, or run the selected workload
- `r`: run the selected workload from the Workloads tab
- `Ctrl-U`: clear filter
- `?`: help
- `q`, `Esc`, or `Ctrl-C`: quit

## CLI Surface

```bash
profilr [PATH]
profilr run [PATH] [--bench off|auto|all] [--save report.json]
profilr tui [PATH]
profilr report [PATH] --format markdown --output report.md
profilr compare base.json head.json
profilr workloads list [PATH]
profilr workloads run [PATH] --name cargo-test
profilr workloads run [PATH] --all --iterations 5
profilr snippet --language python --code 'print(sum(range(1000)))'
profilr snippet --file bench.py --iterations 25 --warmups 3 --json
profilr command --iterations 5 -- cargo test
profilr doctor --json
profilr config --print-default
profilr tools languages --json
profilr tools runners
```

`--json` emits stable machine-readable output for `run`, `report`, `compare`, `workloads`, `snippet`, `command`, `doctor`, and `tools`.

Errors under `--json` use this shape:

```json
{
  "ok": false,
  "error": "message"
}
```

## Project and Language Support

Project detection currently recognizes Rust, Node, Python, Go, Java, Ruby, PHP, Swift, and Make-based repositories. It infers common build/test workloads such as `cargo check`, `cargo test`, `npm run build`, `pytest`, `go test ./...`, `swift test`, and `make check` when those targets exist.

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

The config now covers:

- scan depth, file size, hidden files, and excluded directories
- default output sort and limit
- workload benchmark mode, detection limits, iterations, and warmups

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
