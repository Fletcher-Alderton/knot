# Local NLP Model Architecture

## Scope

The local language model is used only to parse quick-add text into a validated card draft. It never writes Markdown, performs card mutations, or participates in sync.

## Providers

- Hugging Face GGUF: download a user-selected GGUF file from a Hugging Face repository and run it with llama-cpp-2.
- Ollama: call the user’s local Ollama HTTP API; no model files are copied into IrohMD’s model directory.

Hugging Face support means GGUF-compatible models. Arbitrary SafeTensors repositories are not interchangeable with llama.cpp.

## Storage

Models live under the platform application-data directory in models/. Provider configuration is stored separately from board data. Each local model has an id, provider, display name, source, local path, size, and status.

The model manager must support listing models, selecting the active provider/model, downloading/importing a model, deleting one model, and deleting all locally-owned models. Deletion is confined to the application model directory and must never follow paths outside it.

## Inference contract

Rust exposes one Tauri command: parse_quick_add(path, text, default_column) -> ParsedCardDraft.

The prompt includes only the input text, current date/time, and valid board columns. The response is constrained to JSON and validated in Rust: { title, body, column, labels, due, start, confidence, warnings }.

Dates are normalized to ISO-8601 dates. The selected column must match an existing column id/name. Invalid or low-confidence output is returned as a warning for UI review.

After confirmation the frontend passes the resulting fields to the existing add_card command. The model cannot bypass the existing revision and Markdown mutation pipeline.

## First-run behavior

On first quick-add, if no active model exists, show model setup. The default recommendation is a small quantized instruct model. Users can choose Hugging Face GGUF or Ollama instead. Model downloads run asynchronously with progress and can be cancelled.

## Security and reliability

- No remote inference endpoint is used.
- Hugging Face downloads require explicit user action.
- Ollama URLs are limited to localhost by default.
- Model output is treated as untrusted data.
- Tests use a fake provider; real-model tests are opt-in and never required for CI.
