use llama_cpp_2::{
    context::params::LlamaContextParams,
    json_schema_to_grammar,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaChatMessage, LlamaModel, params::LlamaModelParams},
    sampling::LlamaSampler,
};
use std::{
    fs,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

struct LoadedModel {
    path: PathBuf,
    model: LlamaModel,
}

struct GgufRuntime {
    // Models must be dropped before the backend.
    loaded: Option<LoadedModel>,
    backend: LlamaBackend,
}

// llama.cpp permits only one initialized backend per process. Keeping the backend
// here also lets us retain a selected model without reloading it for every card.
static LLAMA_RUNTIME: Mutex<Option<GgufRuntime>> = Mutex::new(None);

fn lock_runtime() -> Result<MutexGuard<'static, Option<GgufRuntime>>, String> {
    LLAMA_RUNTIME
        .lock()
        .map_err(|_| "local model runtime lock is poisoned".to_string())
}

fn runtime(guard: &mut Option<GgufRuntime>) -> Result<&mut GgufRuntime, String> {
    if guard.is_none() {
        let mut backend = LlamaBackend::init().map_err(|e| e.to_string())?;
        if std::env::var_os("IROHMD_LLAMA_LOG").as_deref() != Some(std::ffi::OsStr::new("1")) {
            backend.void_logs();
        }
        *guard = Some(GgufRuntime {
            loaded: None,
            backend,
        });
    }
    guard
        .as_mut()
        .ok_or_else(|| "local model runtime did not initialize".to_string())
}

fn checked_model_path(path: &str, models_root: &Path) -> Result<PathBuf, String> {
    let root = models_root.canonicalize().map_err(|e| e.to_string())?;
    let candidate = Path::new(path).canonicalize().map_err(|e| e.to_string())?;
    if !candidate.starts_with(&root)
        || candidate.extension().and_then(|x| x.to_str()) != Some("gguf")
    {
        return Err("model must be a GGUF file inside the IrohMD model directory".into());
    }
    if !fs::metadata(&candidate)
        .map_err(|e| e.to_string())?
        .is_file()
    {
        return Err("model path is not a file".into());
    }
    Ok(candidate)
}

fn load_from_file(backend: &LlamaBackend, path: &Path) -> Result<LlamaModel, String> {
    // Apple Silicon uses unified memory, so offload every supported layer to Metal.
    let gpu_layers = if backend.supports_gpu_offload() {
        u32::MAX
    } else {
        0
    };
    let params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
    LlamaModel::load_from_file(backend, path, &params).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GgufModelInfo {
    pub path: String,
    pub context_length: u32,
    pub vocabulary_size: u32,
}

pub fn inspect_model(path: &str, models_root: &Path) -> Result<GgufModelInfo, String> {
    let candidate = checked_model_path(path, models_root)?;
    let mut guard = lock_runtime()?;
    let runtime = runtime(&mut guard)?;
    let model = load_from_file(&runtime.backend, &candidate)?;
    Ok(GgufModelInfo {
        path: candidate.to_string_lossy().into_owned(),
        context_length: model.n_ctx_train(),
        vocabulary_size: model
            .n_vocab()
            .try_into()
            .map_err(|_| "invalid vocabulary size".to_string())?,
    })
}

pub fn load_model(path: &str, models_root: &Path) -> Result<(), String> {
    let candidate = checked_model_path(path, models_root)?;
    let mut guard = lock_runtime()?;
    let runtime = runtime(&mut guard)?;
    if runtime
        .loaded
        .as_ref()
        .is_some_and(|item| item.path == candidate)
    {
        return Ok(());
    }
    runtime.loaded = None;
    runtime.loaded = Some(LoadedModel {
        model: load_from_file(&runtime.backend, &candidate)?,
        path: candidate,
    });
    Ok(())
}

pub fn unload_model() -> Result<bool, String> {
    let mut guard = lock_runtime()?;
    Ok(guard
        .as_mut()
        .and_then(|runtime| runtime.loaded.take())
        .is_some())
}

pub fn model_loaded(path: &str, models_root: &Path) -> Result<bool, String> {
    let candidate = checked_model_path(path, models_root)?;
    let guard = lock_runtime()?;
    Ok(guard
        .as_ref()
        .and_then(|runtime| runtime.loaded.as_ref())
        .is_some_and(|item| item.path == candidate))
}

fn format_prompt(model: &LlamaModel, prompt: &str) -> (String, AddBos) {
    if let (Ok(template), Ok(message)) = (
        model.chat_template(None),
        LlamaChatMessage::new("user".into(), prompt.into()),
    ) && let Ok(rendered) = model.apply_chat_template(&template, &[message], true)
    {
        return (rendered, AddBos::Never);
    }
    (prompt.to_owned(), AddBos::Always)
}

fn generate_with_model(
    backend: &LlamaBackend,
    model: &LlamaModel,
    prompt: &str,
    max_tokens: usize,
    schema: &serde_json::Value,
) -> Result<String, String> {
    let (prompt, add_bos) = format_prompt(model, prompt);
    let tokens = model
        .str_to_token(&prompt, add_bos)
        .map_err(|e| e.to_string())?;
    if tokens.is_empty() {
        return Err("model prompt produced no tokens".into());
    }
    let required_context = tokens.len().saturating_add(max_tokens).saturating_add(8);
    let trained_context = model.n_ctx_train() as usize;
    if required_context > trained_context {
        return Err(format!(
            "prompt and output need {required_context} tokens, but the model supports {trained_context}"
        ));
    }
    let context_tokens = required_context
        .max(512)
        .next_power_of_two()
        .min(trained_context);
    let context_tokens = u32::try_from(context_tokens)
        .map_err(|_| "model context length is too large".to_string())?;
    let mut ctx = model
        .new_context(
            backend,
            LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(context_tokens))
                .with_n_batch(context_tokens)
                .with_n_ubatch(context_tokens.min(512))
                .with_n_threads(5)
                .with_n_threads_batch(5),
        )
        .map_err(|e| e.to_string())?;
    let mut batch = LlamaBatch::new(tokens.len() + max_tokens + 8, 1);
    for (position, token) in tokens.iter().enumerate() {
        batch
            .add(*token, position as i32, &[0], position + 1 == tokens.len())
            .map_err(|e| e.to_string())?;
    }
    ctx.decode(&mut batch).map_err(|e| e.to_string())?;
    let schema_json = serde_json::to_string(schema).map_err(|e| e.to_string())?;
    let grammar = json_schema_to_grammar(&schema_json)
        .map_err(|e| format!("invalid Quick Add schema: {e}"))?;
    let grammar_sampler = LlamaSampler::grammar(model, &grammar, "root")
        .map_err(|e| format!("failed to initialize Quick Add grammar: {e}"))?;
    let mut sampler = LlamaSampler::chain_simple([grammar_sampler, LlamaSampler::greedy()]);
    let mut output = Vec::new();
    for index in 0..max_tokens {
        let token = sampler.sample(&ctx, -1);
        if model.is_eog_token(token) {
            break;
        }
        output.extend(
            model
                .token_to_piece_bytes(token, 32, false, None)
                .map_err(|e| e.to_string())?,
        );
        batch.clear();
        batch
            .add(token, (tokens.len() + index) as i32, &[0], true)
            .map_err(|e| e.to_string())?;
        ctx.decode(&mut batch).map_err(|e| e.to_string())?;
    }
    let output =
        String::from_utf8(output).map_err(|_| "local model returned invalid UTF-8".to_string())?;
    if output.trim().is_empty() {
        return Err("local model returned an empty response".into());
    }
    Ok(output)
}

pub fn generate(
    path: &str,
    models_root: &Path,
    prompt: &str,
    max_tokens: usize,
    keep_loaded: bool,
    schema: &serde_json::Value,
) -> Result<String, String> {
    let candidate = checked_model_path(path, models_root)?;
    let mut guard = lock_runtime()?;
    let runtime = runtime(&mut guard)?;
    if keep_loaded {
        if !runtime
            .loaded
            .as_ref()
            .is_some_and(|item| item.path == candidate)
        {
            runtime.loaded = None;
            runtime.loaded = Some(LoadedModel {
                model: load_from_file(&runtime.backend, &candidate)?,
                path: candidate,
            });
        }
        let model = &runtime.loaded.as_ref().expect("loaded above").model;
        generate_with_model(&runtime.backend, model, prompt, max_tokens, schema)
    } else {
        runtime.loaded = None;
        let model = load_from_file(&runtime.backend, &candidate)?;
        generate_with_model(&runtime.backend, &model, prompt, max_tokens, schema)
    }
}
