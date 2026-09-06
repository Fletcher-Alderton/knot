# Quick Add evaluation

100 agent-authored, manually reviewed task inputs with expectations frozen before model evaluation. These are synthetic examples, not user telemetry or a representative production sample. The suite covers plain tasks, directives, natural-language columns, relative and absolute dates, month/year transitions, separate start/due dates, labels, explicit notes, negation, ambiguity and Unicode.

Each case has `id`, `input`, and `expected`. Expected fields are `title`, `body`, `column`, `labels`, `due`, `start`, plus `category` for analysis. Some titles have a predefined `title_any` array of acceptable variants. Title/body scoring ignores case, whitespace and terminal punctuation; labels are compared exactly but without regard to order. Dates and column IDs require exact matches. All-field success requires all six semantic fields and valid output. Confidence and warning wording are checked for schema validity, not semantic correctness.

The fixed clock is Saturday **2026-09-05T00:00:00+10:00**, **Australia/Melbourne**. Columns, in default order: `backlog=Backlog`, `doing=In Progress`, `done=Done`.

Conventions shared with the application prompt:

- The next named weekday is strictly upcoming (`next Friday` = September 11).
- `next week` means the coming Monday (September 7).
- A finish weekday follows its specified start date.
- Dates without a year mean their next occurrence.
- Missing or vague dates are null; unsupported precision is not invented.
- The first column is the default. Explicit hashtags and label requests populate labels.
- Task wording is preserved after metadata removal; explicit notes populate the body.

## Run

```sh
cargo build --release -p desktop --example quick_add_bench
target/release/examples/quick_add_bench \
  --model-path "$HOME/.config/irohmd/models/Qwen3.5-4B-Q4_K_M.gguf" \
  --cases benchmarks/quick-add/cases.json \
  --mode warm --profile hybrid \
  --output benchmarks/quick-add/results/qwen35-4b-hybrid.jsonl
```

Run models sequentially and always give each model/profile a fresh output file. The harness creates a sidecar lock and refuses concurrent writers. `--profile` accepts `full`, `compact`, `hybrid`, `hybrid-full`, or `minimal`; `hybrid` is the production pipeline. `--limit` and `--offset` support smoke checks.

The example imports the application's GGUF runtime and shared prompt/schema/validation module. Current runs use full Metal offload on Apple Silicon, greedy schema-constrained sampling, a dynamically sized context (normally 1024 tokens), and loaded model weights in warm mode. The production hybrid profile deterministically extracts labels, body notes, and columns, then asks the model for compact JSON using a 192-token cap. Each request still creates a fresh inference context. Timings exclude Tauri IPC and card persistence.

## Measured M3 Pro results

All runs below are Q4_K_M, 100 warm requests, Metal, and the fixed dataset clock.

| Model / profile | Strict six-field accuracy | Median | p95 |
|---|---:|---:|---:|
| Qwen3.5 4B / hybrid | **74%** | 2.17s | 2.82s |
| Qwen3.5 4B / full | 63% | 4.48s | 5.85s |
| Qwen3.5 4B / minimal | 67% | **1.47s** | 1.99s |
| NuExtract3 4B / hybrid | 66% | 2.80s | 4.24s |
| NuExtract3 4B / full | 60% | 3.53s | 4.25s |
| Qwen3 1.7B / hybrid | 40% | 1.31s | 1.53s |
| Qwen3 1.7B / full | 28% | 1.88s | 3.05s |
| Qwen2.5 3B / hybrid | 29% | 1.94s | 3.22s |
| Phi-4 Mini 3.8B / full | 11% | 3.31s | 3.96s |

The compact hybrid was selected because it more than halved Qwen3.5 latency while improving strict accuracy by 11 points. The three-field minimal schema is faster, but loses seven strict-accuracy points. JSON remains useful because llama.cpp and Ollama can constrain it with the same schema; compact keys reduce decoding work while deterministic code owns fields that do not need a model.

Raw JSONL records include individual latencies, output/error, field scores, and configuration hashes. Accuracy is strict predefined field matching, not a model-judged semantic score; valid paraphrases outside accepted variants can count as failures. Inspect failures alongside aggregates.
