//! Accuracy and latency harness for the local Quick Add GGUF models.
//!
//! This is deliberately an example binary so it uses the same `gguf_runtime`
//! implementation as the desktop application without making the application
//! library (and its Tauri dependencies) part of the benchmark interface.

#[allow(dead_code)]
#[path = "../src/gguf_runtime.rs"]
mod gguf_runtime;
#[allow(dead_code)]
#[path = "../src/quick_add.rs"]
mod quick_add;

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    env,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Debug, Serialize)]
struct ResultLine {
    id: String,
    model: String,
    mode: String,
    input: String,
    raw_output: Option<String>,
    error: Option<String>,
    latency_ms: f64,
    schema_valid: bool,
    field_accuracy: Map<String, Value>,
    strict_core_success: bool,
    cpu_only: bool,
    model_context_length: Option<u32>,
    inference_context_length: u32,
    vocabulary_size: Option<u32>,
    prompt_sha256: String,
    schema_sha256: String,
    dataset_sha256: String,
}

struct OutputLock(PathBuf);

impl Drop for OutputLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn lock_output(path: &Path) -> io::Result<OutputLock> {
    let lock_path = PathBuf::from(format!("{}.lock", path.display()));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "cannot lock {} (another benchmark may be running): {error}",
                    path.display()
                ),
            )
        })?;
    Ok(OutputLock(lock_path))
}

fn usage() -> ! {
    eprintln!(
        "usage: quick_add_bench --model-path PATH --cases PATH [--output PATH] [--mode warm|cold|both] [--profile full|compact|hybrid|hybrid-full|minimal] [--limit N] [--offset N] [--models-root PATH]"
    );
    std::process::exit(2)
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn normalize(s: &str) -> String {
    let trimmed = s.trim().trim_end_matches(|c: char| ".,!?:;".contains(c));
    trimmed
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn value_string(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn equivalent(field: &str, a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(a), Value::String(b)) => {
            if matches!(field, "title" | "body") {
                normalize(a) == normalize(b)
            } else {
                a == b
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            let mut aa: Vec<_> = a.iter().map(|v| value_string(Some(v))).collect();
            let mut bb: Vec<_> = b.iter().map(|v| value_string(Some(v))).collect();
            aa.sort();
            bb.sort();
            aa == bb
        }
        (Value::Null, Value::Null) => true,
        _ => a == b,
    }
}

fn expected(case: &Value) -> &Map<String, Value> {
    case.get("expected")
        .or_else(|| case.get("expect"))
        .and_then(Value::as_object)
        .unwrap_or_else(|| case.as_object().expect("case must be an object"))
}

fn input(case: &Value) -> String {
    case.get("input")
        .or_else(|| case.get("text"))
        .or_else(|| case.get("prompt"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn id(case: &Value, index: usize) -> String {
    case.get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| (index + 1).to_string())
}

fn parse_json_object(raw: &str) -> Result<Map<String, Value>, String> {
    let trimmed = raw.trim();
    serde_json::from_str::<Value>(trimmed)
        .map_err(|e| format!("invalid JSON: {e}"))?
        .as_object()
        .cloned()
        .ok_or_else(|| "model JSON was not an object".into())
}

fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn sha256_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let model_path = arg(&args, "--model-path").unwrap_or_else(|| usage());
    let cases_path = arg(&args, "--cases").unwrap_or_else(|| usage());
    let output_path =
        PathBuf::from(arg(&args, "--output").unwrap_or_else(|| "quick-add-bench.jsonl".into()));
    if let Some(parent) = output_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let _output_lock = lock_output(&output_path)?;
    let mode = arg(&args, "--mode").unwrap_or_else(|| "both".into());
    let profile = arg(&args, "--profile").unwrap_or_else(|| "full".into());
    if !matches!(
        profile.as_str(),
        "full" | "compact" | "hybrid" | "hybrid-full" | "minimal"
    ) {
        usage();
    }
    let limit = arg(&args, "--limit").and_then(|x| x.parse::<usize>().ok());
    let offset = arg(&args, "--offset")
        .and_then(|x| x.parse::<usize>().ok())
        .unwrap_or(0);
    let models_root = PathBuf::from(arg(&args, "--models-root").unwrap_or_else(|| {
        PathBuf::from(env::var("HOME").unwrap_or_else(|_| ".".into()))
            .join(".config/irohmd/models")
            .display()
            .to_string()
    }));
    let modes: Vec<&str> = match mode.as_str() {
        "warm" => vec!["warm"],
        "cold" => vec!["cold"],
        "both" => vec!["warm", "cold"],
        _ => usage(),
    };
    let model_name = Path::new(&model_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&model_path)
        .to_owned();
    let model_info = gguf_runtime::inspect_model(&model_path, &models_root).ok();
    let columns = vec![
        ("backlog".to_string(), "Backlog".to_string()),
        ("doing".to_string(), "In Progress".to_string()),
        ("done".to_string(), "Done".to_string()),
    ];
    let column_ids = columns.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
    let schema = match profile.as_str() {
        "compact" | "hybrid" => quick_add::compact_schema(&column_ids),
        "minimal" => quick_add::minimal_schema(),
        _ => quick_add::schema(&column_ids),
    };
    let max_tokens = match profile.as_str() {
        "compact" | "hybrid" => quick_add::COMPACT_MAX_TOKENS,
        "minimal" => 128,
        _ => quick_add::MAX_TOKENS,
    };
    let dataset_bytes = fs::read(&cases_path)?;
    let dataset_sha256 = sha256_bytes(&dataset_bytes);
    let cases_file = File::open(&cases_path)?;
    let cases: Vec<Value> = if cases_path.ends_with(".jsonl") {
        BufReader::new(cases_file)
            .lines()
            .map(|line| Ok(serde_json::from_str(&line?)?))
            .collect::<Result<_, Box<dyn std::error::Error>>>()?
    } else {
        let value: Value = serde_json::from_reader(cases_file)?;
        value
            .get("cases")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| value.as_array().cloned().unwrap_or_default())
    };
    let mut done = HashSet::new();
    if let Ok(file) = File::open(&output_path) {
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str::<Value>(&line)
                && let (Some(id), Some(mode), Some(model), Some(dataset)) = (
                    value.get("id").and_then(Value::as_str),
                    value.get("mode").and_then(Value::as_str),
                    value.get("model").and_then(Value::as_str),
                    value.get("dataset_sha256").and_then(Value::as_str),
                )
                && model == model_name
                && dataset == dataset_sha256
            {
                done.insert(format!(
                    "{id}\0{mode}\0{model}\0{dataset}\0{}\0{}",
                    value["prompt_sha256"].as_str().unwrap_or(""),
                    value["schema_sha256"].as_str().unwrap_or("")
                ));
            }
        }
    }
    if let Some(parent) = output_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&output_path)?;
    for run_mode in modes {
        if run_mode == "warm" {
            gguf_runtime::load_model(&model_path, &models_root).map_err(io::Error::other)?;
        }
        for (index, case) in cases
            .iter()
            .enumerate()
            .skip(offset)
            .take(limit.unwrap_or(usize::MAX))
        {
            let case_id = id(case, index);
            if run_mode == "cold" {
                let _ = gguf_runtime::unload_model();
            }
            let text = input(case);
            let hints = quick_add::deterministic_hints(&text, &columns);
            let model_input = if matches!(profile.as_str(), "hybrid" | "hybrid-full" | "minimal") {
                &hints.model_input
            } else {
                &text
            };
            let prompt = if profile == "minimal" {
                quick_add::minimal_prompt_at(
                    model_input,
                    "2026-09-05T00:00:00+10:00",
                    "Australia/Melbourne",
                    "+10:00",
                )
            } else if matches!(profile.as_str(), "compact" | "hybrid") {
                quick_add::compact_prompt_at(
                    model_input,
                    "backlog=Backlog, doing=In Progress, done=Done",
                    "2026-09-05T00:00:00+10:00",
                    "Australia/Melbourne",
                    "+10:00",
                )
            } else {
                quick_add::prompt_at(
                    model_input,
                    "backlog=Backlog, doing=In Progress, done=Done",
                    "2026-09-05T00:00:00+10:00",
                    "Australia/Melbourne",
                    "+10:00",
                )
            };
            let prompt_sha256 = sha256(&prompt);
            let schema_sha256 = sha256(&schema.to_string());
            if done.contains(&format!("{case_id}\0{run_mode}\0{model_name}\0{dataset_sha256}\0{prompt_sha256}\0{schema_sha256}")) { continue; }
            let started = Instant::now();
            let generated = gguf_runtime::generate(
                &model_path,
                &models_root,
                &prompt,
                max_tokens,
                run_mode == "warm",
                &schema,
            );
            let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
            let (raw_output, error, schema_valid, field_accuracy, strict_core_success) =
                match generated {
                    Ok(raw) => match parse_json_object(&raw) {
                        Ok(actual) => {
                            let mut actual = if profile == "minimal" {
                                quick_add::expand_minimal(&Value::Object(actual))?
                                    .as_object()
                                    .cloned()
                                    .ok_or("expanded minimal output was not an object")?
                            } else if matches!(profile.as_str(), "compact" | "hybrid") {
                                quick_add::expand_compact(&Value::Object(actual))?
                                    .as_object()
                                    .cloned()
                                    .ok_or("expanded compact output was not an object")?
                            } else {
                                actual
                            };
                            if matches!(profile.as_str(), "hybrid" | "hybrid-full" | "minimal") {
                                actual.insert("body".into(), Value::String(hints.body.clone()));
                                actual.insert("column".into(), Value::String(hints.column.clone()));
                                actual
                                    .insert("labels".into(), serde_json::to_value(&hints.labels)?);
                            }
                            let exp = expected(case);
                            let fields = ["title", "body", "column", "labels", "due", "start"];
                            let mut accuracy = Map::new();
                            for field in fields {
                                let matches = actual.get(field).is_some_and(|got| {
                                    exp.get(field)
                                        .is_some_and(|want| equivalent(field, got, want))
                                        || (field == "title"
                                            && exp
                                                .get("title_any")
                                                .or_else(|| exp.get("title_variants"))
                                                .is_some_and(|variants| {
                                                    variants.as_array().is_some_and(|items| {
                                                        items.iter().any(|want| {
                                                            equivalent(field, got, want)
                                                        })
                                                    })
                                                }))
                                });
                                accuracy.insert(field.into(), Value::Bool(matches));
                            }
                            let valid = quick_add::validate_output(
                                &Value::Object(actual.clone()),
                                &column_ids,
                            )
                            .is_ok();
                            let core = valid
                                && fields.iter().all(|f| {
                                    accuracy.get(*f).and_then(Value::as_bool).unwrap_or(false)
                                });
                            (Some(raw), None, valid, accuracy, core)
                        }
                        Err(error) => (Some(raw), Some(error), false, Map::new(), false),
                    },
                    Err(error) => (None, Some(error), false, Map::new(), false),
                };
            let line = serde_json::to_string(&ResultLine {
                id: case_id,
                model: model_name.clone(),
                mode: run_mode.into(),
                input: text,
                raw_output,
                error,
                latency_ms,
                schema_valid,
                field_accuracy,
                strict_core_success,
                cpu_only: false,
                model_context_length: model_info.as_ref().map(|x| x.context_length),
                inference_context_length: 1024,
                vocabulary_size: model_info.as_ref().map(|x| x.vocabulary_size),
                prompt_sha256,
                schema_sha256,
                dataset_sha256: dataset_sha256.clone(),
            })?;
            writeln!(output, "{line}")?;
            output.flush()?;
        }
        let _ = gguf_runtime::unload_model();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(labels: Value, due: Value) -> Value {
        serde_json::json!({"title":"Call Sam", "body":"", "column":"backlog", "labels":labels,
            "due":due, "start":null, "confidence":0.9, "warnings":[]})
    }

    #[test]
    fn scorer_normalizes_title_punctuation_but_not_labels() {
        assert!(equivalent(
            "title",
            &Value::String("Call Sam!".into()),
            &Value::String("call sam".into())
        ));
        assert!(!equivalent(
            "labels",
            &Value::String("c++".into()),
            &Value::String("c".into())
        ));
        assert!(equivalent(
            "labels",
            &serde_json::json!(["urgent", "home"]),
            &serde_json::json!(["home", "urgent"])
        ));
    }

    #[test]
    fn scorer_requires_valid_iso_dates_and_nulls() {
        let ids = vec!["backlog".to_string()];
        assert!(
            quick_add::validate_output(
                &draft(serde_json::json!([]), serde_json::json!("2026-09-08")),
                &ids
            )
            .is_ok()
        );
        assert!(
            quick_add::validate_output(
                &draft(serde_json::json!([]), serde_json::json!("tomorrow")),
                &ids
            )
            .is_err()
        );
        assert!(
            quick_add::validate_output(&draft(serde_json::json!(["c++"]), Value::Null), &ids)
                .is_ok()
        );
    }
}
