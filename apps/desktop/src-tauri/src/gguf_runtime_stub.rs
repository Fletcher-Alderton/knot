use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GgufModelInfo {
    pub path: String,
    pub context_length: u32,
    pub vocabulary_size: u32,
}
const ERR: &str = "local AI is disabled; rebuild with the local-ai feature";
pub fn generate(
    _: &str,
    _: &Path,
    _: &str,
    _: usize,
    _: bool,
    _: &serde_json::Value,
) -> Result<String, String> {
    Err(ERR.into())
}
pub fn load_model(_: &str, _: &Path) -> Result<(), String> {
    Err(ERR.into())
}
pub fn unload_model() -> Result<bool, String> {
    Err(ERR.into())
}
pub fn model_loaded(_: &str, _: &Path) -> Result<bool, String> {
    Err(ERR.into())
}
pub fn inspect_model(_: &str, _: &Path) -> Result<GgufModelInfo, String> {
    Err(ERR.into())
}
