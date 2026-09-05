use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, time::{Duration, Instant}};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelProvider {
    HuggingFace,
    Ollama,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelSettings {
    pub provider: Option<ModelProvider>,
    pub model_id: Option<String>,
    pub ollama_url: Option<String>,
}

pub fn models_root() -> PathBuf {
    if let Some(base) = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA")) {
        return PathBuf::from(base).join("IrohMD/models");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/irohmd/models")
}

fn settings_path() -> PathBuf {
    models_root().with_file_name("model-settings.json")
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
    fs::write(
        path,
        serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub fn list_local_models() -> Result<Vec<LocalModel>, String> {
    let root = models_root();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if !path.is_file()
            || path.file_name().and_then(|x| x.to_str()) == Some("model-settings.json")
        {
            continue;
        }
        let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
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
    {
        return Err("invalid model id".into());
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

pub fn delete_all_local_models() -> Result<usize, String> {
    let root = models_root();
    if !root.is_dir() {
        return Ok(0);
    }
    let mut deleted = 0;
    for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_file()
            && path.file_name().and_then(|x| x.to_str()) != Some("model-settings.json")
        {
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

pub fn validate_ollama_url(raw: &str) -> Result<String, String> {
    let url = raw.trim().trim_end_matches('/');
    if url != "http://127.0.0.1:11434" && url != "http://localhost:11434" {
        return Err("Ollama must use the local http://localhost:11434 endpoint".into());
    }
    Ok(url.into())
}

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

pub async fn ollama_generate(raw_url: &str, model: &str, prompt: &str) -> Result<String, String> {
    let url = validate_ollama_url(raw_url)?;
    if model.trim().is_empty() {
        return Err("an Ollama model is required".into());
    }
    let response = reqwest::Client::new().post(format!("{url}/api/generate"))
        .json(&serde_json::json!({"model": model, "prompt": prompt, "stream": false, "format": "json"}))
        .send().await.map_err(|e| format!("Ollama is unavailable: {e}"))?
        .error_for_status().map_err(|e| e.to_string())?
        .json::<serde_json::Value>().await.map_err(|e| format!("invalid Ollama response: {e}"))?;
    response
        .get("response")
        .and_then(|x| x.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Ollama returned no generated response".into())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HuggingFaceModel {
    pub id: String,
    pub downloads: u64,
    pub likes: u64,
    pub last_modified: Option<String>,
}

pub async fn search_huggingface_models(
    query: &str,
    limit: usize,
) -> Result<Vec<HuggingFaceModel>, String> {
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

pub async fn download_huggingface_gguf<F>(
    repo_id: &str,
    filename: &str,
    mut on_progress: F,
) -> Result<LocalModel, String>
where
    F: FnMut(DownloadProgress),
{
    if !repo_id.contains('/')
        || filename.contains('/')
        || !filename.to_ascii_lowercase().ends_with(".gguf")
    {
        return Err("expected a Hugging Face repo id and a .gguf filename".into());
    }
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
    let mut file = tokio::fs::File::create(&partial_path).await.map_err(|e| e.to_string())?;
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
            on_progress(DownloadProgress { filename: filename.into(), downloaded_bytes, total_bytes, bytes_per_second: downloaded_bytes as f64 / elapsed });
            last_update = now;
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    tokio::fs::rename(&partial_path, &path).await.map_err(|e| e.to_string())?;
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    on_progress(DownloadProgress { filename: filename.into(), downloaded_bytes, total_bytes: total_bytes.or(Some(downloaded_bytes)), bytes_per_second: downloaded_bytes as f64 / elapsed });
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_local_ollama_endpoint() {
        assert!(validate_ollama_url("http://localhost:11434").is_ok());
        assert!(validate_ollama_url("https://remote.example").is_err());
    }
    #[test]
    fn rejects_unsafe_model_ids() {
        assert!(delete_local_model("../install.json").is_err());
        assert!(delete_local_model("model-settings.json").is_err());
    }
}
