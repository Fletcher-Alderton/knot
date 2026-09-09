use chrono::{Local, NaiveDate};
use serde_json::{Value, json};

pub const MAX_TOKENS: usize = 512;
pub const COMPACT_MAX_TOKENS: usize = 192;

/// JSON Schema used by Quick Add responses. `column_ids` is deliberately an
/// enum so constrained decoders can never invent a board column.
pub fn schema(column_ids: &[String]) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["title", "body", "column", "labels", "label_colors", "due", "start", "confidence", "warnings"],
        "properties": {
            "title": {"type": "string"},
            "body": {"type": "string"},
            "column": {"type": "string", "enum": column_ids},
            "labels": {"type": "array", "items": {"type": "string"}},
            "label_colors": {"type": "object", "additionalProperties": {"type": "string", "pattern": "^#[0-9A-Fa-f]{6}$"}},
            "due": {"type": ["string", "null"], "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"},
            "start": {"type": ["string", "null"], "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"},
            "confidence": {"type": "number", "minimum": 0, "maximum": 1},
            "warnings": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub fn compact_schema(column_ids: &[String]) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["t", "b", "c", "l", "m", "d", "s"],
        "properties": {
            "t": {"type": "string"},
            "b": {"type": "string"},
            "c": {"type": "string", "enum": column_ids},
            "l": {"type": "array", "items": {"type": "string"}},
            "m": {"type": "object", "additionalProperties": {"type": "string", "pattern": "^#[0-9A-Fa-f]{6}$"}},
            "d": {"type": ["string", "null"], "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"},
            "s": {"type": ["string", "null"], "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"}
        }
    })
}

pub fn minimal_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["t", "d", "s"],
        "properties": {
            "t": {"type": "string"},
            "d": {"type": ["string", "null"], "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"},
            "s": {"type": ["string", "null"], "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"}
        }
    })
}

pub fn minimal_prompt_at(input: &str, timestamp: &str, timezone: &str, offset: &str) -> String {
    format!(
        "Return minified JSON only: t=task title, d=due date, s=start date. Preserve wording and negation; remove scheduling phrases from t. Dates are YYYY-MM-DD or null; never invent them. Current time {timestamp} ({timezone}, UTC {offset}). Next weekday is strictly upcoming; next week is Monday; yearless date is next occurrence; vague dates are null. Input: {input}"
    )
}

pub fn expand_minimal(value: &Value) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or("Quick Add output must be an object")?;
    Ok(json!({
        "title": object.get("t").cloned().unwrap_or(Value::Null),
        "body": "",
        "column": "",
        "labels": [],
        "label_colors": {},
        "due": object.get("d").cloned().unwrap_or(Value::Null),
        "start": object.get("s").cloned().unwrap_or(Value::Null),
        "confidence": 1.0,
        "warnings": []
    }))
}

pub fn expand_compact(value: &Value) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or("Quick Add output must be an object")?;
    Ok(json!({
        "title": object.get("t").cloned().unwrap_or(Value::Null),
        "body": object.get("b").cloned().unwrap_or(Value::Null),
        "column": object.get("c").cloned().unwrap_or(Value::Null),
        "labels": object.get("l").cloned().unwrap_or(Value::Null),
        "label_colors": object.get("m").cloned().unwrap_or(Value::Null),
        "due": object.get("d").cloned().unwrap_or(Value::Null),
        "start": object.get("s").cloned().unwrap_or(Value::Null),
        "confidence": 1.0,
        "warnings": []
    }))
}

/// Keep semantic labels selected by the model, while making explicit hashtags authoritative.
/// Model labels are restricted to labels that already exist on the board; explicit labels may
/// introduce a new label. Matching is case-insensitive and board spelling is canonical.
pub fn merge_labels(
    model_labels: &[String],
    explicit_labels: &[String],
    available_labels: &[String],
) -> Vec<String> {
    let canonical = |label: &str| {
        available_labels
            .iter()
            .find(|available| available.eq_ignore_ascii_case(label.trim()))
            .cloned()
    };
    let mut merged = Vec::new();
    for label in model_labels.iter().filter_map(|label| canonical(label)) {
        if !merged
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&label))
        {
            merged.push(label);
        }
    }
    for label in explicit_labels {
        let label = canonical(label).unwrap_or_else(|| label.trim().to_string());
        if !label.is_empty()
            && !merged
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&label))
        {
            merged.push(label);
        }
    }
    merged
}

pub fn compact_prompt(input: &str, columns: &str) -> String {
    let now = Local::now();
    compact_prompt_at(
        input,
        columns,
        &now.to_rfc3339(),
        &timezone_name(),
        &now.format("%:z").to_string(),
    )
}

pub fn compact_prompt_at(
    input: &str, columns: &str, timestamp: &str, timezone: &str, offset: &str,
) -> String {
    compact_prompt_at_with_labels(input, columns, "", timestamp, timezone, offset)
}

pub fn compact_prompt_at_with_labels(
    input: &str, columns: &str, labels: &str, timestamp: &str, timezone: &str, offset: &str,
) -> String {
    format!(
        "Extract one task as minified JSON only: t=clean title, b=notes or empty, c=column id, l=matching available labels without #, m=label-to-hex-color map, d=due date, s=start date. Remove metadata (#label, in:, due:, start:) and scheduling phrases from t; preserve wording and negation. Classify the task with zero or more labels from Available labels when their meaning matches; do not omit an applicable label and do not invent labels. Markdown links in notes/body are allowed and must be preserved. Dates are YYYY-MM-DD or null. Resolve every concrete scheduling phrase: due:/due/by/on/before/this/next/tomorrow/tonight goes in d unless explicitly described as a start; start:/start/begin/from goes in s; a range from X to Y puts X in s and Y in d. Use null only when that date is absent or genuinely vague. Default to the first column. Current time {timestamp} ({timezone}, UTC {offset}). A named weekday is the strictly upcoming occurrence; next week is the coming Monday; a yearless date is its next occurrence. Columns: {columns}. Available labels and colors: {labels}. Input: {input}"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeterministicHints {
    pub model_input: String,
    pub body: String,
    pub column: String,
    pub labels: Vec<String>,
}

fn remove_ascii_case_insensitive(text: &mut String, needle: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let Some(start) = lower.find(&needle) else {
        return false;
    };
    text.replace_range(start..start + needle.len(), "");
    true
}

fn tidy_title(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-'))
        .to_string()
}

pub fn deterministic_hints(input: &str, columns: &[(String, String)]) -> DeterministicHints {
    let mut model_input = input.trim().to_string();
    let mut body = String::new();
    if let Some(index) = model_input.to_ascii_lowercase().find("; notes:") {
        body = model_input[index + 8..].trim().to_string();
        model_input.truncate(index);
    } else if model_input.to_ascii_lowercase().starts_with("once ")
        && let Some(index) = model_input.find(',')
    {
        body = model_input[..index].trim().to_string();
        model_input = model_input[index + 1..].trim().to_string();
    }

    let mut column = columns
        .first()
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| "backlog".into());
    let mut matches = columns
        .iter()
        .flat_map(|(id, name)| [(id, format!("in:{id}")), (id, format!("in:{name}"))])
        .collect::<Vec<_>>();
    matches.sort_by_key(|(_, marker)| std::cmp::Reverse(marker.len()));
    for (id, marker) in matches {
        if remove_ascii_case_insensitive(&mut model_input, &marker) {
            column = id.clone();
            break;
        }
    }
    let lower = model_input.to_ascii_lowercase();
    if lower.starts_with("already finished:") || lower.ends_with(" as done") {
        if let Some((id, _)) = columns
            .iter()
            .find(|(id, name)| id.eq_ignore_ascii_case("done") || name.eq_ignore_ascii_case("done"))
        {
            column = id.clone();
        }
        if lower.starts_with("already finished:") {
            model_input = model_input[17..].trim().to_string();
        } else {
            model_input.truncate(model_input.len() - 8);
            if model_input.to_ascii_lowercase().starts_with("mark ") {
                model_input = model_input[5..].to_string();
            }
        }
    } else if lower.contains(" in in progress") {
        if let Some((id, _)) = columns.iter().find(|(id, name)| {
            id.eq_ignore_ascii_case("doing") || name.eq_ignore_ascii_case("in progress")
        }) {
            column = id.clone();
        }
        remove_ascii_case_insensitive(&mut model_input, " in In Progress");
    } else if lower.ends_with(" to the backlog") {
        model_input.truncate(model_input.len() - 15);
    }

    let mut labels = Vec::new();
    model_input = model_input
        .split_whitespace()
        .filter_map(|token| {
            let candidate = token.trim_matches(|c: char| matches!(c, ',' | ';' | '.' | '!' | '?'));
            if let Some(label) = candidate
                .strip_prefix('#')
                .filter(|label| !label.is_empty())
            {
                if !labels.iter().any(|existing: &String| existing == label) {
                    labels.push(label.to_string());
                }
                None
            } else {
                Some(token)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    DeterministicHints {
        model_input: tidy_title(&model_input),
        body,
        column,
        labels,
    }
}

pub fn prompt(input: &str, columns: &str) -> String {
    let now = Local::now();
    prompt_at(
        input,
        columns,
        &now.to_rfc3339(),
        &timezone_name(),
        &now.format("%:z").to_string(),
    )
}

pub fn timezone_name() -> String {
    std::env::var("TZ")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            std::fs::read_link("/etc/localtime").ok().and_then(|path| {
                let mut components = path.components();
                while let Some(component) = components.next() {
                    if component.as_os_str() == "zoneinfo" {
                        let rest = components.clone().collect::<std::path::PathBuf>();
                        if !rest.as_os_str().is_empty() {
                            return Some(rest.to_string_lossy().into_owned());
                        }
                    }
                }
                None
            })
        })
        .unwrap_or_else(|| "local system timezone".into())
}

pub fn prompt_at(
    input: &str,
    columns: &str,
    timestamp: &str,
    timezone: &str,
    offset: &str,
) -> String {
    format!(
        "Return exactly one JSON object with these fields: title (string), body (string), column (one valid column), labels (array of strings), due (YYYY-MM-DD or null), start (YYYY-MM-DD or null), confidence (number 0 to 1), warnings (array of strings). Extract one task from the input. Preserve the task wording in title while removing dates, labels, and metadata. body must be an empty string unless separate notes or details are explicitly supplied; never copy the title into body. Remove hashtag tokens and scheduling directives from title. Preserve negation such as do not or never in title. Use the first valid column when no column is implied. Add labels only for explicit hashtags or label requests, stripping #. Use null for absent dates. Resolve relative dates using current local date/time {timestamp}, timezone {timezone}, UTC offset {offset}; the next named weekday means the strictly upcoming occurrence; next week means the coming Monday. A finish weekday follows its stated start date. Dates without a year use the next occurrence. Treat due: and start: as date directives and in: as a column directive. For vague or ambiguous dates use null and a warning. Never invent dates. Put ambiguity or missing details in warnings. Valid columns: {columns}. Input: {input}"
    )
}

fn valid_label_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

pub fn validate_output(value: &Value, column_ids: &[String]) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or("Quick Add output must be an object")?;
    let required = [
        "title",
        "body",
        "column",
        "labels",
        "label_colors",
        "due",
        "start",
        "confidence",
        "warnings",
    ];
    if required.iter().any(|key| !object.contains_key(*key))
        || object.keys().any(|key| !required.contains(&key.as_str()))
    {
        return Err("Quick Add output does not match the required schema".into());
    }
    if !object["title"].is_string()
        || !object["body"].is_string()
        || !object["labels"]
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_string))
        || !object["label_colors"]
            .as_object()
            .is_some_and(|m| m.iter().all(|(label, color)| !label.is_empty() && color.as_str().is_some_and(valid_label_color)))
        || !object["warnings"]
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_string))
    {
        return Err("Quick Add output has invalid field types".into());
    }
    if !column_ids
        .iter()
        .any(|id| Some(id.as_str()) == object["column"].as_str())
    {
        return Err("Quick Add output contains an invalid column".into());
    }
    for key in ["due", "start"] {
        if let Some(date) = object[key].as_str() {
            let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map_err(|_| format!("Quick Add output has invalid {key} date"))?;
            if parsed.format("%Y-%m-%d").to_string() != date {
                return Err(format!("Quick Add output has noncanonical {key} date"));
            }
        } else if !object[key].is_null() {
            return Err(format!("Quick Add output has invalid {key} type"));
        }
    }
    if !object["confidence"]
        .as_f64()
        .is_some_and(|v| (0.0..=1.0).contains(&v))
    {
        return Err("Quick Add confidence must be between 0 and 1".into());
    }
    Ok(())
}

pub fn require_fields(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or("Quick Add output must be an object")?;
    let required = [
        "title",
        "body",
        "column",
        "labels",
        "label_colors",
        "due",
        "start",
        "confidence",
        "warnings",
    ];
    if required.iter().all(|key| object.contains_key(*key)) {
        Ok(())
    } else {
        Err("Quick Add output is missing required fields".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_closed_object_and_column_enum() {
        let value = schema(&["todo".into(), "doing".into()]);
        assert_eq!(value["additionalProperties"], false);
        assert_eq!(value["properties"]["column"]["enum"][1], "doing");
        assert!(
            value["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "due")
        );
    }

    #[test]
    fn schema_converts_to_llama_grammar() {
        let value = schema(&["todo".into()]);
        let grammar = llama_cpp_2::json_schema_to_grammar(&value.to_string()).unwrap();
        assert!(grammar.contains("root ::="));
        let compact = compact_schema(&["todo".into()]);
        let grammar = llama_cpp_2::json_schema_to_grammar(&compact.to_string()).unwrap();
        assert!(grammar.contains("root ::="));
        let expanded = expand_compact(&json!({
            "t": "Call Sam", "b": "", "c": "todo", "l": [], "m": {}, "d": null, "s": null
        }))
        .unwrap();
        assert!(validate_output(&expanded, &["todo".into()]).is_ok());
    }

    #[test]
    fn deterministic_hints_strip_explicit_metadata() {
        let columns = vec![
            ("backlog".into(), "Backlog".into()),
            ("doing".into(), "In Progress".into()),
            ("done".into(), "Done".into()),
        ];
        let hints = deterministic_hints(
            "Fix login timeout #bug #urgent in:In Progress; Notes: reproduce first",
            &columns,
        );
        assert_eq!(hints.model_input, "Fix login timeout");
        assert_eq!(hints.body, "reproduce first");
        assert_eq!(hints.column, "doing");
        assert_eq!(hints.labels, vec!["bug", "urgent"]);
    }

    #[test]
    fn deterministic_hints_preserve_negation_and_unicode() {
        let columns = vec![("backlog".into(), "Backlog".into())];
        let hints = deterministic_hints("Don’t supprimer le café #local", &columns);
        assert_eq!(hints.model_input, "Don’t supprimer le café");
        assert_eq!(hints.labels, vec!["local"]);
    }

    #[test]
    fn prompt_contains_local_date_and_timezone() {
        let p = prompt("call Sam tomorrow", "todo=Todo");
        assert!(p.contains("current local date/time"));
        assert!(p.contains("UTC offset"));
    }

    #[test]
    fn compact_prompt_requires_dates_and_semantic_board_labels() {
        let p = compact_prompt_at_with_labels(
            "fix bug tomorrow", "todo=Todo", "urgent=#ff0000, normal=#00ff00",
            "2026-09-05T00:00:00+00:00", "UTC", "+00:00",
        );
        assert!(p.contains("Available labels and colors: urgent=#ff0000, normal=#00ff00"));
        assert!(p.contains("Classify the task with zero or more labels"));
        assert!(p.contains("tomorrow/tonight goes in d"));
        assert!(p.contains("a range from X to Y puts X in s and Y in d"));
    }

    #[test]
    fn model_labels_survive_and_explicit_labels_are_merged() {
        let available = vec!["Bug".into(), "urgent".into()];
        let model = vec!["bug".into(), "hallucinated".into()];
        let explicit = vec!["URGENT".into(), "customer".into(), "bug".into()];
        assert_eq!(
            merge_labels(&model, &explicit, &available),
            vec!["Bug", "urgent", "customer"]
        );
    }

    #[test]
    fn validates_exact_fields_dates_and_column_values() {
        let valid = json!({"title":"Call Sam","body":"","column":"todo","labels":[],"label_colors":{},
            "due":"2026-09-06","start":null,"confidence":0.9,"warnings":[]});
        let columns = vec!["todo".to_string()];
        assert!(validate_output(&valid, &columns).is_ok());
        for bad_date in ["2026-02-30", "2026-9-06", "tomorrow"] {
            let mut invalid = valid.clone();
            invalid["due"] = json!(bad_date);
            assert!(validate_output(&invalid, &columns).is_err());
        }
        for (field, value) in [
            ("column", json!("invented")),
            ("confidence", json!(2)),
            ("labels", json!(null)),
            ("extra", json!(true)),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(validate_output(&invalid, &columns).is_err());
        }
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove("warnings");
        assert!(validate_output(&missing, &columns).is_err());
    }

    #[test]
    fn fixed_prompt_uses_supplied_clock_instead_of_wall_clock() {
        let prompt = prompt_at(
            "Call Sam tomorrow",
            "todo=Todo",
            "2026-09-05T00:00:00+10:00",
            "Australia/Melbourne",
            "+10:00",
        );
        assert!(prompt.contains("2026-09-05T00:00:00+10:00"));
        assert!(prompt.contains("Australia/Melbourne"));
        assert!(prompt.contains("next week means the coming Monday"));
    }
}
