use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaModel, Special, params::LlamaModelParams},
    sampling::LlamaSampler,
};
use std::{fs, path::Path};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GgufModelInfo {
    pub path: String,
    pub context_length: u32,
    pub vocabulary_size: u32,
}

pub fn inspect_model(path: &str, models_root: &Path) -> Result<GgufModelInfo, String> {
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
    let backend = LlamaBackend::init().map_err(|e| e.to_string())?;
    let params = LlamaModelParams::default().with_n_gpu_layers(0);
    let model =
        LlamaModel::load_from_file(&backend, &candidate, &params).map_err(|e| e.to_string())?;
    Ok(GgufModelInfo {
        path: candidate.to_string_lossy().into_owned(),
        context_length: model.n_ctx_train(),
        vocabulary_size: model
            .n_vocab()
            .try_into()
            .map_err(|_| "invalid vocabulary size".to_string())?,
    })
}

pub fn generate(
    path: &str,
    models_root: &Path,
    prompt: &str,
    max_tokens: usize,
) -> Result<String, String> {
    let root = models_root.canonicalize().map_err(|e| e.to_string())?;
    let candidate = Path::new(path).canonicalize().map_err(|e| e.to_string())?;
    if !candidate.starts_with(&root)
        || candidate.extension().and_then(|x| x.to_str()) != Some("gguf")
    {
        return Err("model must be a GGUF file inside the IrohMD model directory".into());
    }
    let backend = LlamaBackend::init().map_err(|e| e.to_string())?;
    let model_params = LlamaModelParams::default().with_n_gpu_layers(0);
    let model = LlamaModel::load_from_file(&backend, &candidate, &model_params)
        .map_err(|e| e.to_string())?;
    let tokens = model
        .str_to_token(prompt, AddBos::Always)
        .map_err(|e| e.to_string())?;
    let mut ctx = model
        .new_context(&backend, LlamaContextParams::default())
        .map_err(|e| e.to_string())?;
    let mut batch = LlamaBatch::new(tokens.len() + max_tokens + 8, 1);
    for (position, token) in tokens.iter().enumerate() {
        batch
            .add(*token, position as i32, &[0], position + 1 == tokens.len())
            .map_err(|e| e.to_string())?;
    }
    ctx.decode(&mut batch).map_err(|e| e.to_string())?;
    let mut sampler = LlamaSampler::greedy();
    let mut output = String::new();
    for index in 0..max_tokens {
        let token = sampler.sample(&ctx, -1);
        if model.is_eog_token(token) {
            break;
        }
        output.push_str(
            &model
                .token_to_str(token, Special::Plaintext)
                .map_err(|e| e.to_string())?,
        );
        batch.clear();
        batch
            .add(token, (tokens.len() + index) as i32, &[0], true)
            .map_err(|e| e.to_string())?;
        ctx.decode(&mut batch).map_err(|e| e.to_string())?;
    }
    Ok(output)
}
