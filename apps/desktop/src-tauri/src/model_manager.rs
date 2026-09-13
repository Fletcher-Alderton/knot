#[cfg(feature = "local-ai")]
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[cfg(feature = "local-ai")]
use std::{
    path::Component,
    time::{Duration, Instant},
};
#[cfg(feature = "local-ai")]
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelProvider {
    HuggingFace,
    Ollama,
    OpenAI,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalModel {
    pub id: String,
    pub provider: ModelProvider,
    pub name: String,
    pub source: String,
    pub path: Option<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSettings {
    pub provider: Option<ModelProvider>,
    pub model_id: Option<String>,
    pub ollama_url: Option<String>,
    pub openai_base_url: Option<String>,
    pub openai_api_key: Option<String>,
    #[serde(default = "default_keep_model_loaded")]
    pub keep_model_loaded: bool,
}

fn default_keep_model_loaded() -> bool {
    true
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            provider: None,
            model_id: None,
            ollama_url: None,
            openai_base_url: None,
            openai_api_key: None,
            keep_model_loaded: true,
        }
    }
}

pub fn models_root() -> PathBuf {
    crate::app_paths::models_root()
}
pub fn require_local_ai() -> Result<(), String> {
    if cfg!(feature = "local-ai") {
        Ok(())
    } else {
        Err(
            "local AI is disabled; rebuild with the local-ai feature to use local model operations"
                .into(),
        )
    }
}

pub fn require_remote_ai() -> Result<(), String> {
    if cfg!(feature = "remote-ai") {
        Ok(())
    } else {
        Err(
            "remote AI is disabled; rebuild with the remote-ai feature to use Ollama or OpenAI"
                .into(),
        )
    }
}

pub fn require_any_ai() -> Result<(), String> {
    if cfg!(any(feature = "local-ai", feature = "remote-ai")) {
        Ok(())
    } else {
        Err("AI is disabled in this build; enable the local-ai or remote-ai feature".into())
    }
}

pub fn require_provider(provider: &ModelProvider) -> Result<(), String> {
    match provider {
        ModelProvider::HuggingFace => require_local_ai(),
        ModelProvider::Ollama | ModelProvider::OpenAI => require_remote_ai(),
    }
}

fn settings_path() -> PathBuf {
    crate::app_paths::model_settings_path()
}

pub fn load_settings() -> ModelSettings {
    fs::read(settings_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_settings(settings: &ModelSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporary = path.with_file_name(format!(".model-settings.{}.tmp", ulid::Ulid::new()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    #[cfg(windows)]
    if path.exists() {
        let backup = path.with_file_name(format!(".model-settings.backup.{}", ulid::Ulid::new()));
        fs::copy(&path, &backup).map_err(|e| e.to_string())?;
        if let Err(error) = fs::remove_file(&path) {
            let _ = fs::remove_file(&backup);
            let _ = fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        return match fs::rename(&temporary, &path) {
            Ok(()) => {
                let _ = fs::remove_file(&backup);
                Ok(())
            }
            Err(error) => {
                let restore = fs::rename(&backup, &path);
                let _ = fs::remove_file(&temporary);
                match restore {
                    Ok(()) => Err(error.to_string()),
                    Err(restore_error) => Err(format!(
                        "replacement failed: {error}; restore failed: {restore_error}"
                    )),
                }
            }
        };
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(())
}

fn is_gguf_model_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
}

pub fn list_local_models() -> Result<Vec<LocalModel>, String> {
    require_local_ai()?;
    let root = models_root();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.file_type().is_file() || !is_gguf_model_file(&path) {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or("model")
            .to_owned();
        out.push(LocalModel {
            id: name.clone(),
            provider: ModelProvider::HuggingFace,
            name,
            source: "local".into(),
            path: Some(path.to_string_lossy().into_owned()),
            size_bytes: metadata.len(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn delete_local_model(id: &str) -> Result<bool, String> {
    require_local_ai()?;
    let root = models_root()
        .canonicalize()
        .unwrap_or_else(|_| models_root());
    let path = root.join(id);
    if !path.starts_with(&root)
        || id.is_empty()
        || id == "model-settings.json"
        || id.starts_with("../")
        || id.starts_with("..\\")
        || id.contains("/")
        || id.contains("\\")
        || !is_gguf_model_file(Path::new(id))
    {
        return Err("invalid model id".into());
    }
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_file() => fs::remove_file(path)
            .map(|()| true)
            .map_err(|error| error.to_string()),
        Ok(_) => Err("model path is not a regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

pub fn delete_all_local_models() -> Result<usize, String> {
    require_local_ai()?;
    let root = models_root();
    if !root.is_dir() {
        return Ok(0);
    }
    let mut deleted = 0;
    for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_file() && is_gguf_model_file(&path) {
            fs::remove_file(path).map_err(|e| e.to_string())?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OllamaModel {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

#[cfg(any(feature = "remote-ai", test))]
pub fn validate_ollama_url(raw: &str) -> Result<String, String> {
    let url = raw.trim().trim_end_matches('/');
    if url != "http://127.0.0.1:11434" && url != "http://localhost:11434" {
        return Err("Ollama must use the local http://localhost:11434 endpoint".into());
    }
    Ok(url.into())
}

#[cfg(feature = "remote-ai")]
pub async fn list_ollama_models(raw_url: &str) -> Result<Vec<OllamaModel>, String> {
    let url = validate_ollama_url(raw_url)?;
    let response = reqwest::Client::new()
        .get(format!("{url}/api/tags"))
        .send()
        .await
        .map_err(|e| format!("Ollama is unavailable: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("invalid Ollama response: {e}"))?;
    let models = response
        .get("models")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(models
        .into_iter()
        .filter_map(|model| {
            Some(OllamaModel {
                name: model.get("name")?.as_str()?.into(),
                size_bytes: model.get("size").and_then(|x| x.as_u64()).unwrap_or(0),
                modified_at: model
                    .get("modified_at")
                    .and_then(|x| x.as_str())
                    .map(Into::into),
            })
        })
        .collect())
}

#[cfg(feature = "remote-ai")]
pub async fn ollama_generate(
    raw_url: &str,
    model: &str,
    prompt: &str,
    schema: &serde_json::Value,
    keep_loaded: bool,
) -> Result<String, String> {
    let url = validate_ollama_url(raw_url)?;
    if model.trim().is_empty() {
        return Err("an Ollama model is required".into());
    }
    let response = reqwest::Client::new().post(format!("{url}/api/generate"))
        .json(&serde_json::json!({"model": model, "prompt": prompt, "stream": false, "format": schema, "keep_alive": if keep_loaded { -1 } else { 0 }, "options": {"temperature": 0, "num_predict": crate::quick_add::COMPACT_MAX_TOKENS }}))
        .send().await.map_err(|e| format!("Ollama is unavailable: {e}"))?
        .error_for_status().map_err(|e| e.to_string())?
        .json::<serde_json::Value>().await.map_err(|e| format!("invalid Ollama response: {e}"))?;
    response
        .get("response")
        .and_then(|x| x.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Ollama returned no generated response".into())
}

pub const DEFAULT_OPENAI_BASE_URL: &str = "https://openrouter.ai/api/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAIModel {
    pub id: String,
    pub created: Option<u64>,
    pub owned_by: Option<String>,
}

#[cfg(any(feature = "remote-ai", test))]
pub fn validate_openai_base_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("an OpenAI-compatible base URL is required".into());
    }
    if !(raw.starts_with("http://") || raw.starts_with("https://"))
        || raw.contains(char::is_whitespace)
        || raw.contains('?')
        || raw.contains('#')
    {
        return Err("the base URL is malformed".into());
    }
    let authority = raw.split_once("://").map(|(_, rest)| rest).unwrap_or("");
    let host = authority.split('/').next().unwrap_or("");
    if host.is_empty() || host.contains('@') || host.starts_with(':') {
        return Err("the base URL must include a host and must not contain credentials".into());
    }
    Ok(raw.trim_end_matches('/').into())
}

#[cfg(any(feature = "remote-ai", test))]
pub fn openai_chat_request(
    model: &str,
    prompt: &str,
    schema: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0,
        "max_tokens": crate::quick_add::COMPACT_MAX_TOKENS,
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "quick_add",
                "strict": true,
                "schema": schema,
            }
        }
    })
}

#[cfg(any(feature = "remote-ai", test))]
pub fn openai_extract_text(response: &serde_json::Value) -> Result<String, String> {
    response
        .get("choices")
        .and_then(|choices| choices.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "OpenAI API returned no message content".into())
}

#[cfg(any(feature = "remote-ai", test))]
pub fn parse_openai_models(response: &serde_json::Value) -> Vec<OpenAIModel> {
    response
        .get("data")
        .and_then(|data| data.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|model| {
            Some(OpenAIModel {
                id: model.get("id")?.as_str()?.to_owned(),
                created: model.get("created").and_then(|v| v.as_u64()),
                owned_by: model
                    .get("owned_by")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
            })
        })
        .collect()
}

#[cfg(feature = "remote-ai")]
pub async fn list_openai_models(
    raw_base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<OpenAIModel>, String> {
    let base_url = validate_openai_base_url(raw_base_url)?;
    let mut request = reqwest::Client::new().get(format!("{base_url}/models"));
    if let Some(key) = api_key.filter(|key| !key.trim().is_empty()) {
        request = request.bearer_auth(key);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("OpenAI API is unavailable: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("invalid OpenAI API response: {e}"))?;
    Ok(parse_openai_models(&response))
}

#[cfg(any(feature = "remote-ai", test))]
pub fn openai_access_check_url(raw_base_url: &str) -> Result<String, String> {
    let base_url = validate_openai_base_url(raw_base_url)?;
    let authority = base_url
        .split_once("://")
        .map(|(_, authority)| authority)
        .unwrap_or_default();
    let host = authority
        .split('/')
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    let path = if host.eq_ignore_ascii_case("openrouter.ai") {
        "auth/key"
    } else {
        "models"
    };
    Ok(format!("{base_url}/{path}"))
}

#[cfg(feature = "remote-ai")]
pub async fn check_openai_access(raw_base_url: &str, api_key: Option<&str>) -> Result<(), String> {
    let key = api_key
        .filter(|key| !key.trim().is_empty())
        .ok_or("an API key is required to check access")?;
    let url = openai_access_check_url(raw_base_url)?;
    reqwest::Client::new()
        .get(url)
        .bearer_auth(key)
        .send()
        .await
        .map_err(|e| format!("OpenAI API is unavailable: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(feature = "remote-ai")]
pub async fn openai_chat_completion(
    raw_base_url: &str,
    api_key: Option<&str>,
    model: &str,
    prompt: &str,
    schema: &serde_json::Value,
) -> Result<String, String> {
    let base_url = validate_openai_base_url(raw_base_url)?;
    if model.trim().is_empty() {
        return Err("an OpenAI-compatible model is required".into());
    }
    let mut request = reqwest::Client::new()
        .post(format!("{base_url}/chat/completions"))
        .json(&openai_chat_request(model, prompt, schema));
    if let Some(key) = api_key.filter(|key| !key.trim().is_empty()) {
        request = request.bearer_auth(key);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("OpenAI API is unavailable: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("invalid OpenAI API response: {e}"))?;
    openai_extract_text(&response)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HuggingFaceModel {
    pub id: String,
    pub downloads: u64,
    pub likes: u64,
    pub last_modified: Option<String>,
}

#[cfg(feature = "local-ai")]
pub async fn search_huggingface_models(
    query: &str,
    limit: usize,
) -> Result<Vec<HuggingFaceModel>, String> {
    require_local_ai()?;
    let limit = limit.clamp(1, 50);
    let response = reqwest::Client::new()
        .get("https://huggingface.co/api/models")
        .query(&[
            ("search", query.trim()),
            ("pipeline_tag", "text-generation"),
            ("sort", "downloads"),
            ("direction", "-1"),
            ("limit", &limit.to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("Hugging Face is unavailable: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<Vec<serde_json::Value>>()
        .await
        .map_err(|e| format!("invalid Hugging Face response: {e}"))?;
    Ok(response
        .into_iter()
        .filter_map(|m| {
            Some(HuggingFaceModel {
                id: m.get("id")?.as_str()?.to_owned(),
                downloads: m.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
                likes: m.get("likes").and_then(|v| v.as_u64()).unwrap_or(0),
                last_modified: m
                    .get("lastModified")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
            })
        })
        .collect())
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub filename: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub bytes_per_second: f64,
}

#[cfg(feature = "local-ai")]
pub async fn download_huggingface_gguf<F>(
    repo_id: &str,
    filename: &str,
    mut on_progress: F,
) -> Result<LocalModel, String>
where
    F: FnMut(DownloadProgress),
{
    let filename_path = Path::new(filename);
    let safe_filename = filename_path.components().count() == 1
        && matches!(
            filename_path.components().next(),
            Some(Component::Normal(_))
        )
        && !filename.contains('/')
        && !filename.contains('\\');
    if !repo_id.contains('/') || !safe_filename || !filename.to_ascii_lowercase().ends_with(".gguf")
    {
        return Err("expected a Hugging Face repo id and a .gguf filename".into());
    }
    require_local_ai()?;
    let root = models_root();
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let path = root.join(filename);
    let partial_path = root.join(format!("{filename}.part"));
    let url = format!("https://huggingface.co/{repo_id}/resolve/main/{filename}");
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Hugging Face download failed: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let total_bytes = response.content_length();
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(&partial_path)
        .await
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    let mut last_update = started;
    let mut downloaded_bytes = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        downloaded_bytes += chunk.len() as u64;
        let now = Instant::now();
        if now.duration_since(last_update) >= Duration::from_millis(100) {
            let elapsed = now.duration_since(started).as_secs_f64().max(0.001);
            on_progress(DownloadProgress {
                filename: filename.into(),
                downloaded_bytes,
                total_bytes,
                bytes_per_second: downloaded_bytes as f64 / elapsed,
            });
            last_update = now;
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    tokio::fs::rename(&partial_path, &path)
        .await
        .map_err(|e| e.to_string())?;
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    on_progress(DownloadProgress {
        filename: filename.into(),
        downloaded_bytes,
        total_bytes: total_bytes.or(Some(downloaded_bytes)),
        bytes_per_second: downloaded_bytes as f64 / elapsed,
    });
    let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
    Ok(LocalModel {
        id: filename.into(),
        provider: ModelProvider::HuggingFace,
        name: filename.into(),
        source: repo_id.into(),
        path: Some(path.to_string_lossy().into_owned()),
        size_bytes: metadata.len(),
    })
}

#[cfg(not(feature = "remote-ai"))]
pub async fn list_ollama_models(_: &str) -> Result<Vec<OllamaModel>, String> {
    Err("remote AI is disabled; rebuild with the remote-ai feature".into())
}
#[cfg(not(feature = "remote-ai"))]
pub async fn ollama_generate(
    _: &str,
    _: &str,
    _: &str,
    _: &serde_json::Value,
    _: bool,
) -> Result<String, String> {
    Err("remote AI is disabled; rebuild with the remote-ai feature".into())
}
#[cfg(not(feature = "remote-ai"))]
pub async fn list_openai_models(_: &str, _: Option<&str>) -> Result<Vec<OpenAIModel>, String> {
    Err("remote AI is disabled; rebuild with the remote-ai feature".into())
}
#[cfg(not(feature = "remote-ai"))]
pub async fn check_openai_access(_: &str, _: Option<&str>) -> Result<(), String> {
    Err("remote AI is disabled; rebuild with the remote-ai feature".into())
}
#[cfg(not(feature = "remote-ai"))]
pub async fn openai_chat_completion(
    _: &str,
    _: Option<&str>,
    _: &str,
    _: &str,
    _: &serde_json::Value,
) -> Result<String, String> {
    Err("remote AI is disabled; rebuild with the remote-ai feature".into())
}
#[cfg(not(feature = "local-ai"))]
pub async fn search_huggingface_models(_: &str, _: usize) -> Result<Vec<HuggingFaceModel>, String> {
    Err("local AI is disabled; rebuild with the local-ai feature".into())
}
#[cfg(not(feature = "local-ai"))]
pub async fn download_huggingface_gguf<F>(_: &str, _: &str, _: F) -> Result<LocalModel, String>
where
    F: FnMut(DownloadProgress),
{
    Err("local AI is disabled; rebuild with the local-ai feature".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn openai_provider_serializes_as_openai() {
        let json = serde_json::to_string(&ModelProvider::OpenAI).unwrap();
        assert_eq!(json, r#""openai""#);
        let parsed: ModelProvider = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ModelProvider::OpenAI);
    }
    #[test]
    fn openai_settings_default_and_backwards_compatible() {
        let settings = ModelSettings::default();
        assert_eq!(settings.openai_base_url, None);
        assert_eq!(settings.openai_api_key, None);
        // Settings files written before the OpenAI provider existed lack these fields.
        let legacy = r#"{"provider":"ollama","model_id":"llama3","ollama_url":"http://127.0.0.1:11434","keep_model_loaded":false}"#;
        let parsed: ModelSettings = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.provider, Some(ModelProvider::Ollama));
        assert_eq!(parsed.openai_base_url, None);
        assert_eq!(parsed.openai_api_key, None);
        assert!(!parsed.keep_model_loaded);
        // An OpenAI-configured settings file round-trips.
        let saved = serde_json::to_string(&ModelSettings {
            provider: Some(ModelProvider::OpenAI),
            model_id: Some("openai/gpt-4o-mini".into()),
            openai_base_url: Some("https://openrouter.ai/api/v1".into()),
            openai_api_key: Some("sk-or-test".into()),
            ..ModelSettings::default()
        })
        .unwrap();
        let round_trip: ModelSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!(round_trip.provider, Some(ModelProvider::OpenAI));
        assert_eq!(
            round_trip.openai_base_url.as_deref(),
            Some("https://openrouter.ai/api/v1")
        );
        assert_eq!(round_trip.openai_api_key.as_deref(), Some("sk-or-test"));
    }
    #[test]
    fn validates_openai_base_urls() {
        assert_eq!(
            validate_openai_base_url("https://openrouter.ai/api/v1").unwrap(),
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(
            validate_openai_base_url("  https://api.openai.com/v1/  ").unwrap(),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            validate_openai_base_url("http://localhost:8080/v1").unwrap(),
            "http://localhost:8080/v1"
        );
        assert!(validate_openai_base_url("").is_err());
        assert!(validate_openai_base_url("   ").is_err());
        assert!(validate_openai_base_url("openrouter.ai/api/v1").is_err());
        assert!(validate_openai_base_url("ftp://example.com/v1").is_err());
        assert!(validate_openai_base_url("https://").is_err());
        assert!(validate_openai_base_url("https://exa mple.com").is_err());
        assert!(validate_openai_base_url("https://example.com/v1?tenant=a").is_err());
        assert!(validate_openai_base_url("https://example.com/v1#models").is_err());
        assert!(validate_openai_base_url("https://user:pass@example.com/v1").is_err());
    }
    #[test]
    fn openai_chat_request_has_exact_shape() {
        let schema = serde_json::json!({"type": "object"});
        let body = openai_chat_request("openai/gpt-4o-mini", "make a card", &schema);
        assert_eq!(
            body,
            serde_json::json!({
                "model": "openai/gpt-4o-mini",
                "messages": [{"role": "user", "content": "make a card"}],
                "temperature": 0,
                "max_tokens": crate::quick_add::COMPACT_MAX_TOKENS,
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": "quick_add",
                        "strict": true,
                        "schema": {"type": "object"}
                    }
                }
            })
        );
    }
    #[test]
    fn openai_extract_text_reads_choice_content() {
        let response = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": r#"{"title":"T"}"#}}]
        });
        assert_eq!(openai_extract_text(&response).unwrap(), r#"{"title":"T"}"#);
        assert!(openai_extract_text(&serde_json::json!({})).is_err());
        assert!(openai_extract_text(&serde_json::json!({"choices": []})).is_err());
        assert!(openai_extract_text(&serde_json::json!({"choices": [{}]})).is_err());
        assert!(
            openai_extract_text(&serde_json::json!({
                "choices": [{"message": {"content": 42}}]
            }))
            .is_err()
        );
        assert!(
            openai_extract_text(&serde_json::json!({
                "choices": [{"message": {"content": null}}]
            }))
            .is_err()
        );
    }
    #[test]
    fn parse_openai_models_skips_bad_entries() {
        let response = serde_json::json!({
            "data": [
                {"id": "openai/gpt-4o-mini", "created": 1716768000, "owned_by": "openai"},
                {"id": "openai/gpt-4o"},
                {"created": 123, "owned_by": "no-id"},
                "not-an-object",
                {"id": 7}
            ]
        });
        let models = parse_openai_models(&response);
        assert_eq!(
            models,
            vec![
                OpenAIModel {
                    id: "openai/gpt-4o-mini".into(),
                    created: Some(1716768000),
                    owned_by: Some("openai".into()),
                },
                OpenAIModel {
                    id: "openai/gpt-4o".into(),
                    created: None,
                    owned_by: None,
                },
            ]
        );
        assert!(parse_openai_models(&serde_json::json!({})).is_empty());
        assert!(parse_openai_models(&serde_json::json!({"data": {}})).is_empty());
    }
    #[test]
    fn chooses_a_key_validation_endpoint_for_openrouter_and_generic_apis() {
        assert_eq!(
            openai_access_check_url("https://openrouter.ai/api/v1").unwrap(),
            "https://openrouter.ai/api/v1/auth/key"
        );
        assert_eq!(
            openai_access_check_url("https://api.openai.com/v1/").unwrap(),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            openai_access_check_url("http://localhost:8080/v1").unwrap(),
            "http://localhost:8080/v1/models"
        );
        assert!(openai_access_check_url("not a URL").is_err());
    }
    #[test]
    fn validates_local_ollama_endpoint() {
        assert!(ModelSettings::default().keep_model_loaded);
        assert!(validate_ollama_url("http://localhost:11434").is_ok());
        assert!(validate_ollama_url("https://remote.example").is_err());
    }
    #[test]
    fn provider_matrix_matches_compiled_features() {
        assert_eq!(
            require_provider(&ModelProvider::HuggingFace).is_ok(),
            cfg!(feature = "local-ai")
        );
        assert_eq!(
            require_provider(&ModelProvider::Ollama).is_ok(),
            cfg!(feature = "remote-ai")
        );
        assert_eq!(
            require_provider(&ModelProvider::OpenAI).is_ok(),
            cfg!(feature = "remote-ai")
        );
        assert_eq!(
            require_any_ai().is_ok(),
            cfg!(any(feature = "local-ai", feature = "remote-ai"))
        );
    }

    #[test]
    fn only_regular_gguf_files_are_models() {
        assert!(is_gguf_model_file(Path::new("model.gguf")));
        assert!(is_gguf_model_file(Path::new("MODEL.GGUF")));
        assert!(!is_gguf_model_file(Path::new("model.gguf.part")));
        assert!(!is_gguf_model_file(Path::new("notes.txt")));
        assert!(!is_gguf_model_file(Path::new("model-settings.json")));
    }

    #[test]
    fn rejects_unsafe_model_ids() {
        assert!(delete_local_model("../install.json").is_err());
        assert!(delete_local_model("model-settings.json").is_err());
    }

    #[cfg(not(feature = "local-ai"))]
    #[tokio::test]
    async fn disabled_local_ai_rejects_hugging_face_search_before_network() {
        let error = search_huggingface_models("qwen", 1).await.unwrap_err();
        assert!(error.contains("local AI is disabled"));
    }
}
