#![cfg_attr(test, allow(dead_code, unused_imports))]
use iroh::{EndpointAddr, EndpointId};
use knot_core::{Card, CardFrontmatter, SyncMetadata};
use knot_iroh::{
    ConnectedIrohTransport, IrohTransport, generate_secret_key, secret_key_from_bytes,
    secret_key_to_bytes,
};
use knot_revisions::{Revision, snapshot, tombstone};
use knot_store::{BoardStore, FsBoardStore};
use knot_sync::{DeviceIdentity, MemoryRevisionRepository, RevisionRepository};
#[cfg(target_os = "ios")]
use notify::PollWatcher;
#[cfg(not(target_os = "ios"))]
use notify::RecommendedWatcher;
use notify::{Config, EventKind, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(target_os = "ios")]
use std::time::Duration;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
};
#[cfg(mobile)]
use tauri::Manager;

mod app_paths;
#[cfg(feature = "local-ai")]
mod gguf_runtime;
#[cfg(not(feature = "local-ai"))]
#[path = "gguf_runtime_stub.rs"]
mod gguf_runtime;
mod model_manager;
pub mod quick_add;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardColumn {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardInfo {
    pub path: String,
    pub board_id: String,
    pub title: String,
    pub columns: Vec<BoardColumn>,
    pub labels: BTreeMap<String, String>,
    pub cards: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardInfo {
    pub id: String,
    pub title: String,
    pub body: String,
    pub column: String,
    pub position: i64,
    pub labels: Vec<String>,
    pub label_colors: std::collections::BTreeMap<String, String>,
    pub due: Option<String>,
    pub start: Option<String>,
    pub updated_at: Option<String>,
    pub revision: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedCard {
    pub card: CardInfo,
    pub revision_id: String,
    pub archived_at: u64,
}
fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ParsedCardDraft {
    #[serde(deserialize_with = "deserialize_null_default")]
    pub title: String,
    #[serde(deserialize_with = "deserialize_null_default")]
    pub body: String,
    #[serde(deserialize_with = "deserialize_null_default")]
    pub column: String,
    #[serde(deserialize_with = "deserialize_null_default")]
    pub labels: Vec<String>,
    #[serde(deserialize_with = "deserialize_null_default")]
    pub label_colors: BTreeMap<String, String>,
    pub due: Option<String>,
    pub start: Option<String>,
    #[serde(deserialize_with = "deserialize_null_default")]
    pub confidence: f32,
    #[serde(deserialize_with = "deserialize_null_default")]
    pub warnings: Vec<String>,
}

fn parse_model_json(raw: &str) -> Result<serde_json::Value, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("model returned an empty response".into());
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Ok(value);
    }
    for (start, ch) in trimmed.char_indices() {
        if ch != '{' {
            continue;
        }
        let mut values =
            serde_json::Deserializer::from_str(&trimmed[start..]).into_iter::<serde_json::Value>();
        if let Some(Ok(value)) = values.next()
            && value.is_object()
        {
            return Ok(value);
        }
    }
    let preview: String = trimmed.chars().take(160).collect();
    Err(format!(
        "model did not return a JSON object (response began: {preview})"
    ))
}

#[cfg(test)]
fn parse_model_draft(raw: &str) -> Result<ParsedCardDraft, String> {
    serde_json::from_value(parse_model_json(raw)?)
        .map_err(|error| format!("model returned invalid quick-add JSON: {error}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchEvent {
    pub path: String,
    pub kind: String,
    pub valid: bool,
    pub duplicate: bool,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub trusted: bool,
    pub address: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub connected: bool,
    pub trusted_peers: usize,
    pub pending_events: usize,
    pub endpoint_id: Option<String>,
    pub address: Option<String>,
    pub connection: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointInfo {
    pub endpoint_id: String,
    pub address: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    pub card_id: String,
    pub local_revision_id: String,
    pub remote_revision_id: String,
    pub parent_revision_ids: Vec<String>,
    pub local: Option<CardInfo>,
    pub remote: Option<CardInfo>,
    pub local_tombstone: bool,
    pub remote_tombstone: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub status: String,
    pub transferred: usize,
    pub received: usize,
    pub conflicts: Vec<SyncConflict>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct InstallPeer {
    endpoint_id: String,
    address: Option<String>,
    authorized_boards: std::collections::HashSet<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct InstallConfig {
    device_id: String,
    device_name: String,
    #[serde(default)]
    secret_key: Option<Vec<u8>>,
    peers: HashMap<String, InstallPeer>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct CardInput {
    pub title: String,
    pub body: String,
    pub column: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub label_colors: std::collections::BTreeMap<String, String>,
    pub position: Option<i64>,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictResolutionInput {
    pub choice: String,
    pub local: CardInput,
    pub remote: CardInput,
    pub manual: Option<CardInput>,
    #[serde(default)]
    pub tombstone: bool,
    pub parent_revision_ids: Vec<String>,
}
fn valid_revision_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn config_path() -> PathBuf {
    app_paths::config_path()
}
fn load_config() -> InstallConfig {
    let p = config_path();
    fs::read(&p)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| InstallConfig {
            device_id: ulid::Ulid::new().to_string(),
            device_name: std::env::var("KNOT_DEVICE_NAME")
                .unwrap_or_else(|_| "Knot Desktop".into()),
            secret_key: None,
            peers: HashMap::new(),
        })
}
fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", ulid::Ulid::new()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    #[cfg(windows)]
    if path.exists() {
        let backup = path.with_file_name(format!(".{name}.{}.backup", ulid::Ulid::new()));
        fs::copy(path, &backup).map_err(|e| e.to_string())?;
        if let Err(error) = fs::remove_file(path) {
            let _ = fs::remove_file(&backup);
            let _ = fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        return match fs::rename(&temporary, path) {
            Ok(()) => {
                let _ = fs::remove_file(&backup);
                Ok(())
            }
            Err(error) => {
                let restore = fs::rename(&backup, path);
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
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        error.to_string()
    })
}
fn save_config(c: &InstallConfig) -> Result<(), String> {
    let p = config_path();
    atomic_write_bytes(
        &p,
        &serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(target_os = "ios")]
type RuntimeWatcher = PollWatcher;
#[cfg(not(target_os = "ios"))]
type RuntimeWatcher = RecommendedWatcher;

struct Runtime {
    boards: HashMap<String, FsBoardStore>,
    events: VecDeque<WatchEvent>,
    watchers: Vec<RuntimeWatcher>,
    watched_paths: std::collections::HashSet<String>,
    peers: std::collections::HashSet<String>,
    self_hashes: HashMap<String, String>,
    /// Last known revision per board/card, used to parent watcher tombstones.
    last_revisions: HashMap<String, String>,
    self_deletes: std::collections::HashSet<String>,
    transport: Option<IrohTransport>,
    config: InstallConfig,
    last_connection: String,
    listener_started: bool,
}
static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
fn runtime() -> &'static Mutex<Runtime> {
    RUNTIME.get_or_init(|| {
        let config = load_config();
        let peers = config.peers.keys().cloned().collect();
        Mutex::new(Runtime {
            boards: HashMap::new(),
            events: VecDeque::new(),
            watchers: Vec::new(),
            watched_paths: std::collections::HashSet::new(),
            peers,
            self_hashes: HashMap::new(),
            last_revisions: HashMap::new(),
            self_deletes: std::collections::HashSet::new(),
            transport: None,
            config,
            last_connection: "disconnected".into(),
            listener_started: false,
        })
    })
}
fn key(path: &str) -> String {
    PathBuf::from(path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path))
        .to_string_lossy()
        .into_owned()
}
fn board(path: &str) -> Result<FsBoardStore, String> {
    let k = key(path);
    let mut r = runtime().lock().unwrap();
    if !r.boards.contains_key(&k) {
        r.boards.insert(
            k.clone(),
            FsBoardStore::open(&k).map_err(|e| e.to_string())?,
        );
    }
    Ok(r.boards.get(&k).unwrap().clone())
}
fn read_board_attachment(path: &str, relative: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let root = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|c| {
            !matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        return Err("Attachment must be inside the board".into());
    }
    let file = fs::canonicalize(root.join(relative)).map_err(|e| e.to_string())?;
    if !file.starts_with(&root) {
        return Err("Attachment must be inside the board".into());
    }
    let extension = file
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !matches!(
        extension.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "avif"
            | "bmp"
            | "svg"
            | "mp3"
            | "wav"
            | "ogg"
            | "m4a"
            | "mp4"
            | "webm"
            | "mov"
            | "pdf"
    ) {
        return Err("Unsupported attachment type".into());
    }
    let source = fs::File::open(file).map_err(|e| e.to_string())?;
    if !source.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Attachment is not a file".into());
    }
    let limit = 50 * 1024 * 1024;
    let mut bytes = Vec::new();
    source
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Attachment exceeds 50 MB".into());
    }
    Ok(bytes)
}

fn save_store(path: &str, store: FsBoardStore) {
    runtime().lock().unwrap().boards.insert(key(path), store);
}
fn board_identity(path: &Path) -> Result<String, String> {
    let metadata = path.join("board.md");
    let text = fs::read_to_string(&metadata).map_err(|e| e.to_string())?;
    let mut parsed = knot_core::Board::parse(&text).map_err(|e| e.to_string())?;
    if let Some(id) = parsed.metadata.get("id").and_then(|v| v.as_str())
        && knot_core::validate_ulid(id)
    {
        return Ok(id.to_string());
    }
    if parsed.metadata.contains_key("id") {
        return Err("board id must be a valid ULID".into());
    }
    let id = ulid::Ulid::new().to_string();
    parsed
        .metadata
        .insert("id".into(), serde_yaml::Value::String(id.clone()));
    atomic_write_bytes(
        &metadata,
        parsed.to_markdown().map_err(|e| e.to_string())?.as_bytes(),
    )?;
    Ok(id)
}
fn default_columns() -> Vec<BoardColumn> {
    vec![
        BoardColumn {
            id: "backlog".into(),
            name: "Backlog".into(),
            extra: HashMap::new(),
        },
        BoardColumn {
            id: "doing".into(),
            name: "Doing".into(),
            extra: HashMap::new(),
        },
        BoardColumn {
            id: "done".into(),
            name: "Done".into(),
            extra: HashMap::new(),
        },
    ]
}
fn board_metadata(path: &Path) -> Result<(knot_core::Board, String, Vec<BoardColumn>), String> {
    let file = path.join("board.md");
    let text = fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let mut b = knot_core::Board::parse(&text).map_err(|e| e.to_string())?;
    let mut changed = false;
    let id = match b.metadata.get("id").and_then(|v| v.as_str()) {
        Some(id) if knot_core::validate_ulid(id) => id.to_string(),
        Some(_) => return Err("board id must be a valid ULID".into()),
        None => {
            let id = ulid::Ulid::new().to_string();
            b.metadata
                .insert("id".into(), serde_yaml::Value::String(id.clone()));
            changed = true;
            id
        }
    };
    let parsed_columns = b
        .metadata
        .get("columns")
        .and_then(|v| serde_yaml::from_value::<Vec<BoardColumn>>(v.clone()).ok())
        .filter(|columns| !columns.is_empty());
    let columns = parsed_columns.clone().unwrap_or_else(default_columns);
    if parsed_columns.is_none() {
        b.metadata.insert(
            "columns".into(),
            serde_yaml::to_value(&columns).map_err(|e| e.to_string())?,
        );
        changed = true;
    }
    if b.metadata
        .get("title")
        .and_then(|v| v.as_str())
        .is_none_or(|v| v.trim().is_empty())
    {
        b.metadata
            .insert("title".into(), serde_yaml::Value::String("My board".into()));
        changed = true;
    }
    if changed {
        atomic_write_bytes(
            &file,
            b.to_markdown().map_err(|e| e.to_string())?.as_bytes(),
        )?;
    }
    Ok((b, id, columns))
}
fn inspect(path: String) -> Result<BoardInfo, String> {
    let (b, board_id, columns) = board_metadata(Path::new(&path))?;
    let s = board(&path)?;
    Ok(BoardInfo {
        path,
        board_id,
        title: b
            .metadata
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("My board")
            .into(),
        columns,
        labels: b
            .metadata
            .get("labels")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default(),
        cards: s.list_cards().map_err(|e| e.to_string())?,
    })
}
fn save_board_metadata(
    path: &str,
    mut f: impl FnMut(&mut knot_core::Board) -> Result<(), String>,
) -> Result<BoardInfo, String> {
    board_metadata(Path::new(path))?;
    let file = Path::new(path).join("board.md");
    let text = fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let mut b = knot_core::Board::parse(&text).map_err(|e| e.to_string())?;
    f(&mut b)?;
    atomic_write_bytes(
        &file,
        b.to_markdown().map_err(|e| e.to_string())?.as_bytes(),
    )?;
    inspect(path.to_string())
}
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "column".into() } else { out }
}
fn card_info(id: String, markdown: &str) -> Result<CardInfo, String> {
    let c = Card::parse(markdown).map_err(|e| e.to_string())?;
    let label_colors = c.frontmatter.label_colors();
    Ok(CardInfo {
        id,
        title: c.frontmatter.title,
        body: c.body,
        column: c.frontmatter.column,
        position: c.frontmatter.position,
        label_colors,
        labels: c.frontmatter.labels,
        due: c.frontmatter.due,
        start: c
            .frontmatter
            .extra
            .get("start")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        updated_at: c
            .frontmatter
            .sync
            .as_ref()
            .and_then(|s| s.updated_at.clone()),
        revision: c.frontmatter.sync.and_then(|s| s.revision),
    })
}
fn revision() -> String {
    ulid::Ulid::new().to_string()
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
fn revisions_dir(root: &Path) -> Result<PathBuf, String> {
    let knot = root.join(".knot");
    for path in [&knot, &knot.join("revisions")] {
        if let Ok(metadata) = fs::symlink_metadata(path)
            && metadata.file_type().is_symlink()
        {
            return Err(format!(
                "refusing symlinked revision path: {}",
                path.display()
            ));
        }
    }
    Ok(knot.join("revisions"))
}
fn validate_revision_record(r: &Revision) -> Result<(), String> {
    let safe = |value: &str| {
        !value.is_empty()
            && value.len() <= 200
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    };
    if !safe(&r.revision_id)
        || !safe(&r.card_id)
        || r.parents.iter().any(|parent| !safe(parent))
        || (r.tombstone && r.snapshot.is_some())
        || (!r.tombstone
            && r.snapshot
                .as_deref()
                .map(|body| match knot_core::Card::parse(body) {
                    Ok(card) => card.frontmatter.id != r.card_id,
                    Err(_) => true,
                })
                .unwrap_or(true))
        || r.content_hash != knot_revisions::content_hash(r.snapshot.as_deref(), r.tombstone)
    {
        return Err(format!("invalid revision record: {}", r.revision_id));
    }
    Ok(())
}
fn revision_has_cycle(
    id: &str,
    parents: &HashMap<String, Vec<String>>,
    visiting: &mut std::collections::HashSet<String>,
    visited: &mut std::collections::HashSet<String>,
) -> bool {
    if visiting.contains(id) {
        return true;
    }
    if !visited.insert(id.to_string()) {
        return false;
    }
    visiting.insert(id.to_string());
    let cycle = parents
        .get(id)
        .into_iter()
        .flatten()
        .any(|parent| revision_has_cycle(parent, parents, visiting, visited));
    visiting.remove(id);
    cycle
}
fn validate_revision_graph(revisions: &[Revision]) -> Result<(), String> {
    let mut by_id = HashMap::new();
    for revision in revisions {
        if by_id.insert(&revision.revision_id, revision).is_some() {
            return Err(format!("duplicate revision ID: {}", revision.revision_id));
        }
    }
    let mut parents_by_id = HashMap::new();
    for revision in revisions {
        let mut parents = std::collections::HashSet::new();
        for parent_id in &revision.parents {
            if !parents.insert(parent_id) || parent_id == &revision.revision_id {
                return Err(format!(
                    "invalid parents for revision {}",
                    revision.revision_id
                ));
            }
            let parent = by_id.get(parent_id).ok_or_else(|| {
                format!("missing parent {parent_id} for {}", revision.revision_id)
            })?;
            if parent.card_id != revision.card_id {
                return Err(format!("parent card mismatch for {}", revision.revision_id));
            }
        }
        parents_by_id.insert(revision.revision_id.clone(), revision.parents.clone());
    }
    let mut visiting = std::collections::HashSet::new();
    let mut visited = std::collections::HashSet::new();
    if parents_by_id
        .keys()
        .any(|id| revision_has_cycle(id, &parents_by_id, &mut visiting, &mut visited))
    {
        return Err("revision graph contains a cycle".into());
    }
    Ok(())
}
fn validate_existing_revision(path: &Path, expected: &Revision) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "refusing symlinked revision file: {}",
            path.display()
        ));
    }
    let existing =
        serde_json::from_slice::<Revision>(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("{}: {error}", path.display()))?;
    validate_revision_record(&existing)?;
    let same_revision = existing.revision_id == expected.revision_id
        && existing.card_id == expected.card_id
        && existing.parents == expected.parents
        && existing.content_hash == expected.content_hash
        && existing.snapshot == expected.snapshot
        && existing.tombstone == expected.tombstone;
    if same_revision {
        Ok(())
    } else {
        Err(format!("revision ID collision: {}", expected.revision_id))
    }
}

fn persist_revision(root: &Path, r: &Revision) -> Result<(), String> {
    validate_revision_record(r)?;
    let d = revisions_dir(root)?;
    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    for parent_id in &r.parents {
        let parent_path = d.join(format!("{parent_id}.json"));
        let bytes = fs::read(&parent_path)
            .map_err(|_| format!("missing parent {parent_id} for {}", r.revision_id))?;
        let parent: Revision = serde_json::from_slice(&bytes)
            .map_err(|e| format!("{}: {e}", parent_path.display()))?;
        validate_revision_record(&parent)?;
        if parent.card_id != r.card_id {
            return Err(format!("parent card mismatch for {}", r.revision_id));
        }
    }
    let p = d.join(format!("{}.json", r.revision_id));
    match fs::symlink_metadata(&p) {
        Ok(_) => return validate_existing_revision(&p, r),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let bytes = serde_json::to_vec_pretty(r).map_err(|e| e.to_string())?;
    let temporary = d.join(format!(".{}.{}.tmp", r.revision_id, ulid::Ulid::new()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    match fs::hard_link(&temporary, &p) {
        Ok(()) => {
            let _ = fs::remove_file(&temporary);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temporary);
            validate_existing_revision(&p, r)
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error.to_string())
        }
    }
}
fn load_revisions(path: &str) -> Result<Vec<Revision>, String> {
    let directory = revisions_dir(Path::new(path))?;
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut revisions = Vec::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "refusing symlinked revision file: {}",
                path.display()
            ));
        }
        if !metadata.is_file() {
            continue;
        }
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let revision = serde_json::from_slice::<Revision>(&bytes)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        validate_revision_record(&revision)?;
        revisions.push(revision);
    }
    validate_revision_graph(&revisions)?;
    Ok(revisions)
}
fn archived_cards(path: &str) -> Result<Vec<ArchivedCard>, String> {
    let revisions = load_revisions(path)?;
    let parents: std::collections::HashSet<String> = revisions
        .iter()
        .flat_map(|r| r.parents.iter().cloned())
        .collect();
    let by_id: HashMap<String, Revision> = revisions
        .into_iter()
        .map(|r| (r.revision_id.clone(), r))
        .collect();
    let mut result = Vec::new();
    for revision in by_id
        .values()
        .filter(|r| r.tombstone && !parents.contains(&r.revision_id))
    {
        let mut pending = revision.parents.clone();
        let mut snapshot = None;
        while let Some(parent) = pending.pop() {
            if let Some(candidate) = by_id.get(&parent) {
                if candidate.tombstone {
                    pending.extend(candidate.parents.clone());
                } else if candidate.snapshot.is_some() {
                    snapshot = Some(candidate);
                    break;
                }
            }
        }
        let Some(snapshot) = snapshot else { continue };
        let card = card_info(
            revision.card_id.clone(),
            snapshot.snapshot.as_deref().unwrap(),
        )?;
        result.push(ArchivedCard {
            card,
            revision_id: revision.revision_id.clone(),
            archived_at: revision.timestamp,
        });
    }
    result.sort_by(|a, b| {
        b.archived_at
            .cmp(&a.archived_at)
            .then_with(|| a.card.title.cmp(&b.card.title))
    });
    Ok(result)
}
fn content_hash(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.replace("\r\n", "\n").replace('\r', "\n").as_bytes());
    format!("{:x}", h.finalize())
}

fn revision_key(root: &Path, id: &str) -> String {
    format!("{}\0{}", root.to_string_lossy(), id)
}

static BOARD_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

fn board_lock(path: &Path) -> Arc<Mutex<()>> {
    let key = key(&path.to_string_lossy());
    let registry = BOARD_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = registry.lock().unwrap_or_else(|e| e.into_inner());
    locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn tracked_write(store: &mut FsBoardStore, id: &str, markdown: &str) -> Result<(), String> {
    let old = store.card_path(id).ok();
    store.write_card(id, markdown).map_err(|e| e.to_string())?;
    let path = store.card_path(id).map_err(|e| e.to_string())?;
    let mut r = runtime().lock().unwrap();
    r.self_hashes
        .insert(path.to_string_lossy().into_owned(), content_hash(markdown));
    if let Some(old) = old.filter(|old| old != &path) {
        r.self_deletes.insert(old.to_string_lossy().into_owned());
        r.self_hashes.remove(&old.to_string_lossy().into_owned());
    }
    Ok(())
}

fn process_external_event(root: &Path, path: &Path, kind: &EventKind) {
    let board_mutex = board_lock(root);
    let _io = board_mutex.lock().unwrap_or_else(|e| e.into_inner());
    let is_card = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        == Some("cards")
        && path.extension().and_then(|x| x.to_str()) == Some("md");
    if !is_card {
        return;
    }
    let path_string = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let root_key = key(&root.to_string_lossy());
    let mut valid = true;
    let mut duplicate = false;
    let mut error = None;
    if matches!(kind, EventKind::Create(_) | EventKind::Modify(_)) {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                valid = false;
                error = Some(e.to_string());
                String::new()
            }
        };
        if valid {
            match Card::parse(&text) {
                Err(e) => {
                    valid = false;
                    error = Some(format!("malformed Markdown/YAML; file left untouched: {e}"));
                }
                Ok(card) => {
                    let id = card.frontmatter.id.clone();
                    if let Ok(entries) = fs::read_dir(root.join("cards")) {
                        for entry in entries.flatten() {
                            let other = entry.path();
                            if other == path
                                || other.extension().and_then(|x| x.to_str()) != Some("md")
                            {
                                continue;
                            }
                            if fs::read_to_string(&other)
                                .ok()
                                .and_then(|s| Card::parse(&s).ok())
                                .map(|c| c.frontmatter.id == id)
                                .unwrap_or(false)
                            {
                                duplicate = true;
                            }
                        }
                    }
                    let hash = content_hash(&text);
                    // notify may canonicalize paths; use the same canonical board key as stores.
                    let event_key = Path::new(&root_key)
                        .join("cards")
                        .join(path.file_name().unwrap_or_default())
                        .to_string_lossy()
                        .into_owned();
                    let self_write = {
                        let r = runtime().lock().unwrap();
                        // Keep the hash for the whole notify burst. A single
                        // local write can produce more than one modify event.
                        r.self_hashes
                            .get(&event_key)
                            .map(|h| h == &hash)
                            .unwrap_or(false)
                    };
                    // The UI already has the result of a local mutation. Do not
                    // echo that filesystem event back as an external change.
                    if self_write {
                        return;
                    }
                    if !duplicate {
                        let old = FsBoardStore::open(root)
                            .ok()
                            .and_then(|store| store.card_path(&id).ok())
                            .and_then(|path| fs::read_to_string(path).ok());
                        let revision_key = revision_key(root, &id);
                        let known_parent = runtime()
                            .lock()
                            .unwrap()
                            .last_revisions
                            .get(&revision_key)
                            .cloned();
                        let parents = known_parent
                            .or_else(|| {
                                old.as_deref()
                                    .and_then(|s| Card::parse(s).ok())
                                    .and_then(|c| c.frontmatter.sync)
                                    .and_then(|s| s.revision)
                            })
                            .into_iter()
                            .collect();
                        let rev = snapshot(
                            revision(),
                            id.clone(),
                            parents,
                            chrono::Utc::now().timestamp() as u64,
                            text,
                        );
                        let rid = rev.revision_id.clone();
                        if persist_revision(root, &rev).is_ok() {
                            runtime()
                                .lock()
                                .unwrap()
                                .last_revisions
                                .insert(revision_key, rid);
                        }
                    }
                }
            }
        }
    } else if matches!(kind, EventKind::Remove(_))
        && let Some(stem) = path.file_stem().and_then(|x| x.to_str())
    {
        let id = knot_store::extract_card_id(stem);
        let self_delete = runtime().lock().unwrap().self_deletes.remove(&path_string);
        if self_delete {
            return;
        }
        let parent = runtime()
            .lock()
            .unwrap()
            .last_revisions
            .get(&revision_key(root, id))
            .cloned();
        let rev = tombstone(
            revision(),
            id.to_string(),
            parent.into_iter().collect(),
            chrono::Utc::now().timestamp() as u64,
        );
        let rid = rev.revision_id.clone();
        if persist_revision(root, &rev).is_ok() {
            runtime()
                .lock()
                .unwrap()
                .last_revisions
                .insert(revision_key(root, id), rid);
        }
    }
    runtime().lock().unwrap().events.push_back(WatchEvent {
        path: path_string,
        kind: format!("{kind:?}"),
        valid,
        duplicate,
        error,
    });
}

fn with_mutation(
    path: &str,
    id: &str,
    input: CardInput,
    parent_override: Option<Vec<String>>,
) -> Result<CardInfo, String> {
    let board_mutex = board_lock(Path::new(path));
    let _io = board_mutex.lock().unwrap_or_else(|e| e.into_inner());
    for (name, value) in [
        ("due", input.due.as_deref()),
        ("start", input.start.as_deref()),
    ] {
        if let Some(value) = value
            && !value.trim().is_empty()
            && (chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_err() || value.len() != 10)
        {
            return Err(format!("{name} must be a valid YYYY-MM-DD date"));
        }
    }
    let mut s = board(path)?;
    let old = s.read_card(id).ok();
    let mut fm = old
        .as_deref()
        .and_then(|x| Card::parse(x).ok())
        .map(|x| x.frontmatter)
        .unwrap_or_else(|| CardFrontmatter {
            id: id.into(),
            title: String::new(),
            column: "backlog".into(),
            position: 1000,
            due: None,
            labels: vec![],
            created_at: Some(now()),
            sync: None,
            activity: vec![],
            extra: Default::default(),
        });
    let parent = parent_override.unwrap_or_else(|| {
        fm.sync
            .as_ref()
            .and_then(|x| x.revision.clone())
            .into_iter()
            .collect()
    });
    let rid = revision();
    fm.title = input.title;
    fm.column = input.column;
    fm.position = input.position.unwrap_or(fm.position);
    fm.labels = input.labels;
    if !input.label_colors.is_empty() {
        fm.extra.insert(
            "label_colors".into(),
            serde_yaml::to_value(input.label_colors).map_err(|e| e.to_string())?,
        );
    }
    if let Some(due) = input.due {
        fm.due = (!due.trim().is_empty()).then_some(due);
    }
    if let Some(start) = input.start {
        if start.trim().is_empty() {
            fm.extra.remove("start");
        } else {
            fm.extra
                .insert("start".into(), serde_yaml::Value::String(start));
        }
    }
    fm.sync = Some(SyncMetadata {
        revision: Some(rid),
        parents: parent.into_iter().collect(),
        content_hash: None,
        updated_at: Some(now()),
        extra: Default::default(),
    });
    let md = Card {
        frontmatter: fm.clone(),
        body: input.body,
    }
    .to_markdown()
    .map_err(|e| e.to_string())?;
    let previous = s.read_card(id).ok();
    tracked_write(&mut s, id, &md)?;
    let canonical_root = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path));
    runtime().lock().unwrap().self_hashes.insert(
        canonical_root
            .join("cards")
            .join(
                s.card_path(id)
                    .map_err(|e| e.to_string())?
                    .file_name()
                    .unwrap(),
            )
            .to_string_lossy()
            .into_owned(),
        content_hash(&md),
    );
    let parents = fm
        .sync
        .as_ref()
        .map(|x| x.parents.clone())
        .unwrap_or_default();
    let rid = fm.sync.as_ref().and_then(|x| x.revision.clone()).unwrap();
    let rev = snapshot(
        rid,
        id,
        parents,
        chrono::Utc::now().timestamp() as u64,
        md.clone(),
    );
    if let Err(error) = persist_revision(Path::new(path), &rev) {
        if let Some(previous) = previous {
            let _ = tracked_write(&mut s, id, &previous);
        } else {
            let _ = s.delete_card(id);
        }
        let _ = runtime().lock().unwrap().self_hashes.remove(
            &canonical_root
                .join("cards")
                .join(
                    s.card_path(id)
                        .ok()
                        .and_then(|path| path.file_name().map(|name| name.to_owned()))
                        .unwrap_or_default(),
                )
                .to_string_lossy()
                .into_owned(),
        );
        return Err(error);
    }
    runtime()
        .lock()
        .unwrap()
        .last_revisions
        .insert(revision_key(&canonical_root, id), rev.revision_id.clone());
    save_store(path, s);
    card_info(id.into(), &md)
}
async fn ensure_transport() -> Result<IrohTransport, String> {
    if let Some(t) = runtime().lock().unwrap().transport.clone() {
        return Ok(t);
    }
    let key = {
        let mut r = runtime().lock().unwrap();
        match r.config.secret_key.clone() {
            Some(bytes) => secret_key_from_bytes(&bytes).map_err(|e| e.to_string())?,
            None => {
                let k = generate_secret_key();
                r.config.secret_key = Some(secret_key_to_bytes(&k).to_vec());
                save_config(&r.config)?;
                k
            }
        }
    };
    let t = IrohTransport::bind_with_secret_key(key)
        .await
        .map_err(|e| e.to_string())?;
    runtime().lock().unwrap().transport = Some(t.clone());
    start_listener(t.clone());
    Ok(t)
}
fn start_listener(transport: IrohTransport) {
    let should_start = {
        let mut r = runtime().lock().unwrap();
        if r.listener_started {
            false
        } else {
            r.listener_started = true;
            true
        }
    };
    if !should_start {
        return;
    }
    tokio::spawn(async move {
        while let Ok(mut wire) = transport.accept_connected().await {
            let remote = wire.peer().to_string();
            let selected = {
                let r = runtime().lock().unwrap();
                r.config.peers.get(&remote).and_then(|peer| {
                    r.boards.keys().find_map(|path| {
                        board_identity(Path::new(path))
                            .ok()
                            .filter(|id| peer.authorized_boards.contains(id))
                            .map(|id| (path.clone(), id, r.config.clone()))
                    })
                })
            };
            let Some((path, board_id, cfg)) = selected else {
                continue;
            };
            let repo = match load_repo(&path) {
                Ok(x) => x,
                Err(_) => continue,
            };
            let shared = Arc::new(tokio::sync::Mutex::new(repo));
            let mut identity = DeviceIdentity::new(cfg.device_name);
            identity.device_id = cfg.device_id;
            identity.endpoint_id = transport.endpoint_id().to_string();
            identity.trust(remote.clone());
            identity.authorize_board(board_id.clone());
            if knot_sync::sync_once(&mut wire, shared.clone(), &identity, &remote, &board_id)
                .await
                .is_ok()
                && let Ok(repo) = Arc::try_unwrap(shared)
            {
                let repo = repo.into_inner();
                let _ = materialize(&path, &repo);
                runtime().lock().unwrap().last_connection = "connected".into();
            }
        }
    });
}
fn load_repo(path: &str) -> Result<MemoryRevisionRepository, String> {
    let mut repo = MemoryRevisionRepository::new(board_identity(Path::new(path))?);
    for revision in load_revisions(path)? {
        repo.add(knot_sync::Revision::from_domain(&revision));
    }
    Ok(repo)
}
fn materialize(path: &str, repo: &MemoryRevisionRepository) -> Result<usize, String> {
    let board_mutex = board_lock(Path::new(path));
    let _io = board_mutex.lock().unwrap_or_else(|e| e.into_inner());
    let mut n = 0;
    let mut store = board(path)?;
    let all = repo.all();
    let mut pending: Vec<_> = all
        .iter()
        .map(|revision| revision.to_domain(chrono::Utc::now().timestamp() as u64))
        .collect();
    while !pending.is_empty() {
        let mut progress = false;
        let mut remaining = Vec::new();
        for revision in pending {
            match persist_revision(Path::new(path), &revision) {
                Ok(()) => progress = true,
                Err(error) if error.contains("missing parent") => remaining.push(revision),
                Err(error) => return Err(error),
            }
        }
        if !progress {
            return Err("unable to persist revision graph in parent order".into());
        }
        pending = remaining;
    }
    let heads: std::collections::HashSet<_> = repo.heads().into_iter().collect();
    let mut by_card: HashMap<String, Vec<knot_sync::Revision>> = HashMap::new();
    for r in all.into_iter().filter(|r| heads.contains(&r.id)) {
        by_card.entry(r.card_id.clone()).or_default().push(r);
    }
    for revisions in by_card.into_values().filter(|items| items.len() == 1) {
        let d = revisions[0].to_domain(chrono::Utc::now().timestamp() as u64);
        if d.tombstone {
            if store.read_card(&d.card_id).is_ok() {
                let deleted_path = store
                    .card_path(&d.card_id)
                    .ok()
                    .map(|path| path.to_string_lossy().into_owned());
                if let Some(path) = &deleted_path {
                    runtime().lock().unwrap().self_deletes.insert(path.clone());
                }
                if let Err(error) = store.delete_card(&d.card_id) {
                    if let Some(path) = deleted_path {
                        runtime().lock().unwrap().self_deletes.remove(&path);
                    }
                    return Err(error.to_string());
                }
                n += 1;
            }
            runtime().lock().unwrap().last_revisions.insert(
                revision_key(Path::new(path), &d.card_id),
                d.revision_id.clone(),
            );
        } else if let Some(s) = d.snapshot {
            tracked_write(&mut store, &d.card_id, &s)?;
            runtime().lock().unwrap().last_revisions.insert(
                revision_key(Path::new(path), &d.card_id),
                d.revision_id.clone(),
            );
            n += 1;
        }
    }
    save_store(path, store);
    Ok(n)
}
mod commands {
    use super::*;
    use tauri::Emitter;
    #[tauri::command]
    pub fn model_settings() -> Result<model_manager::ModelSettings, String> {
        model_manager::require_any_ai()?;
        Ok(model_manager::load_settings())
    }
    #[tauri::command]
    pub fn save_model_settings(settings: model_manager::ModelSettings) -> Result<(), String> {
        model_manager::require_any_ai()?;
        if let Some(provider) = settings.provider.as_ref() {
            model_manager::require_provider(provider)?;
        }
        model_manager::save_settings(&settings)
    }
    #[tauri::command]
    pub fn list_local_models() -> Result<Vec<model_manager::LocalModel>, String> {
        model_manager::require_local_ai()?;
        model_manager::list_local_models()
    }
    #[tauri::command]
    pub async fn list_ollama_models(
        url: Option<String>,
    ) -> Result<Vec<model_manager::OllamaModel>, String> {
        model_manager::require_remote_ai()?;
        model_manager::list_ollama_models(url.as_deref().unwrap_or("http://127.0.0.1:11434")).await
    }
    #[tauri::command]
    pub async fn list_openai_models(
        base_url: Option<String>,
        api_key: Option<String>,
    ) -> Result<Vec<model_manager::OpenAIModel>, String> {
        model_manager::require_remote_ai()?;
        model_manager::list_openai_models(
            base_url
                .as_deref()
                .unwrap_or(model_manager::DEFAULT_OPENAI_BASE_URL),
            api_key.as_deref(),
        )
        .await
    }
    #[tauri::command]
    pub async fn check_openai_access(
        base_url: Option<String>,
        api_key: Option<String>,
    ) -> Result<(), String> {
        model_manager::require_remote_ai()?;
        model_manager::check_openai_access(
            base_url
                .as_deref()
                .unwrap_or(model_manager::DEFAULT_OPENAI_BASE_URL),
            api_key.as_deref(),
        )
        .await
    }
    #[tauri::command]
    pub async fn parse_quick_add(path: String, text: String) -> Result<ParsedCardDraft, String> {
        model_manager::require_any_ai()?;
        if text.trim().is_empty() {
            return Err("quick-add input is empty".into());
        }
        let settings = model_manager::load_settings();
        let provider = settings.provider.ok_or("configure an AI provider first")?;
        let model = settings.model_id.ok_or("choose a model first")?;
        model_manager::require_provider(&provider)?;
        let info = inspect(path)?;
        let column_pairs = info
            .columns
            .iter()
            .map(|column| (column.id.clone(), column.name.clone()))
            .collect::<Vec<_>>();
        let columns = column_pairs
            .iter()
            .map(|(id, name)| format!("{id}={name}"))
            .collect::<Vec<_>>()
            .join(", ");
        let column_ids = column_pairs
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let hints = quick_add::deterministic_hints(&text, &column_pairs);
        let schema = quick_add::compact_schema(&column_ids);
        let labels = info
            .labels
            .iter()
            .map(|(name, color)| format!("{name}={color}"))
            .collect::<Vec<_>>()
            .join(", ");
        let now = chrono::Local::now();
        let prompt = quick_add::compact_prompt_at_with_labels(
            &hints.model_input,
            &columns,
            &labels,
            &now.to_rfc3339(),
            &quick_add::timezone_name(),
            &now.format("%:z").to_string(),
        );
        let raw = match provider {
            model_manager::ModelProvider::Ollama => {
                model_manager::ollama_generate(
                    settings
                        .ollama_url
                        .as_deref()
                        .unwrap_or("http://127.0.0.1:11434"),
                    &model,
                    &prompt,
                    &schema,
                    settings.keep_model_loaded,
                )
                .await?
            }
            model_manager::ModelProvider::OpenAI => {
                model_manager::openai_chat_completion(
                    settings
                        .openai_base_url
                        .as_deref()
                        .unwrap_or(model_manager::DEFAULT_OPENAI_BASE_URL),
                    settings.openai_api_key.as_deref(),
                    &model,
                    &prompt,
                    &schema,
                )
                .await?
            }
            model_manager::ModelProvider::HuggingFace => {
                let local = model_manager::list_local_models()?
                    .into_iter()
                    .find(|m| m.id == model)
                    .ok_or("selected Hugging Face model is not installed")?;
                let path = local.path.ok_or("selected model has no local path")?;
                gguf_runtime::generate(
                    &path,
                    &model_manager::models_root(),
                    &prompt,
                    quick_add::COMPACT_MAX_TOKENS,
                    settings.keep_model_loaded,
                    &schema,
                )?
            }
        };
        let compact = parse_model_json(&raw)?;
        let mut expanded = quick_add::expand_compact(&compact)?;
        let model_labels = expanded
            .get("labels")
            .and_then(serde_json::Value::as_array)
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let available_labels = info.labels.keys().cloned().collect::<Vec<_>>();
        let merged_labels =
            quick_add::merge_labels(&model_labels, &hints.labels, &available_labels);
        let object = expanded
            .as_object_mut()
            .ok_or("Quick Add output must be an object")?;
        object.insert("body".into(), serde_json::Value::String(hints.body));
        object.insert("column".into(), serde_json::Value::String(hints.column));
        object.insert(
            "labels".into(),
            serde_json::to_value(merged_labels).map_err(|e| e.to_string())?,
        );
        let mut draft: ParsedCardDraft = serde_json::from_value(expanded)
            .map_err(|error| format!("model returned invalid quick-add JSON: {error}"))?;
        if draft.title.trim().is_empty() {
            return Err("model returned an empty title".into());
        }
        let valid_column = info
            .columns
            .iter()
            .find(|c| {
                c.id.eq_ignore_ascii_case(&draft.column)
                    || c.name.eq_ignore_ascii_case(&draft.column)
            })
            .map(|c| c.id.clone());
        match valid_column {
            Some(column) => draft.column = column,
            None => {
                draft
                    .warnings
                    .push(format!("Unknown column: {}", draft.column));
                draft.column = info
                    .columns
                    .first()
                    .map(|c| c.id.clone())
                    .unwrap_or_else(|| "backlog".into());
            }
        }
        draft.confidence = draft.confidence.clamp(0.0, 1.0);
        let validated = serde_json::to_value(&draft).map_err(|e| e.to_string())?;
        quick_add::validate_output(&validated, &column_ids)
            .map_err(|error| format!("model returned invalid Quick Add data: {error}"))?;
        Ok(draft)
    }

    #[tauri::command]
    pub async fn search_huggingface_models(
        query: String,
        limit: Option<usize>,
    ) -> Result<Vec<model_manager::HuggingFaceModel>, String> {
        model_manager::require_local_ai()?;
        model_manager::search_huggingface_models(&query, limit.unwrap_or(20)).await
    }

    #[tauri::command]
    pub async fn download_huggingface_gguf(
        app: tauri::AppHandle,
        repo_id: String,
        filename: String,
    ) -> Result<model_manager::LocalModel, String> {
        model_manager::require_local_ai()?;
        model_manager::download_huggingface_gguf(&repo_id, &filename, |progress| {
            let _ = app.emit("model-download-progress", progress);
        })
        .await
    }
    fn local_model_path(id: &str) -> Result<String, String> {
        model_manager::list_local_models()?
            .into_iter()
            .find(|model| model.id == id)
            .ok_or_else(|| "selected Hugging Face model is not installed".to_string())?
            .path
            .ok_or_else(|| "selected model has no local path".to_string())
    }
    #[tauri::command]
    pub fn load_local_model(id: String) -> Result<(), String> {
        model_manager::require_local_ai()?;
        gguf_runtime::load_model(&local_model_path(&id)?, &model_manager::models_root())
    }
    #[tauri::command]
    pub fn unload_local_model() -> Result<bool, String> {
        model_manager::require_local_ai()?;
        gguf_runtime::unload_model()
    }
    #[tauri::command]
    pub fn local_model_loaded(id: String) -> Result<bool, String> {
        gguf_runtime::model_loaded(&local_model_path(&id)?, &model_manager::models_root())
    }
    #[tauri::command]
    pub fn inspect_local_model(path: String) -> Result<gguf_runtime::GgufModelInfo, String> {
        gguf_runtime::inspect_model(&path, &model_manager::models_root())
    }
    #[tauri::command]
    pub fn delete_local_model(id: String) -> Result<bool, String> {
        model_manager::require_local_ai()?;
        model_manager::delete_local_model(&id)
    }
    #[tauri::command]
    pub fn delete_all_local_models() -> Result<usize, String> {
        model_manager::require_local_ai()?;
        model_manager::delete_all_local_models()
    }
    #[tauri::command]
    pub fn open_board(path: String) -> Result<BoardInfo, String> {
        inspect(path)
    }
    #[tauri::command]
    pub fn create_board(path: String) -> Result<BoardInfo, String> {
        let p = PathBuf::from(&path);
        fs::create_dir_all(p.join("cards")).map_err(|e| e.to_string())?;
        let b = p.join("board.md");
        if !b.exists() {
            let temporary = p.join(format!(".board.{}.tmp", ulid::Ulid::new()));
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| e.to_string())?;
            file.write_all(b"---\ntitle: My board\n---\n\n")
                .map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            if let Err(error) = fs::rename(&temporary, &b) {
                let _ = fs::remove_file(&temporary);
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error.to_string());
                }
            }
        }
        inspect(path)
    }
    #[tauri::command]
    pub fn open_or_create_board(path: String) -> Result<BoardInfo, String> {
        if Path::new(&path).join("board.md").exists() {
            open_board(path)
        } else {
            create_board(path)
        }
    }
    #[tauri::command]
    pub fn rename_board(path: String, title: String) -> Result<BoardInfo, String> {
        if title.trim().is_empty() {
            return Err("board title must be nonempty".into());
        }
        save_board_metadata(&path, |b| {
            b.metadata.insert(
                "title".into(),
                serde_yaml::Value::String(title.trim().into()),
            );
            Ok(())
        })
    }
    #[tauri::command]
    pub fn rename_column(
        path: String,
        column_id: String,
        name: String,
    ) -> Result<BoardInfo, String> {
        if name.trim().is_empty() {
            return Err("column name must be nonempty".into());
        }
        save_board_metadata(&path, |b| {
            let mut cols: Vec<BoardColumn> = b
                .metadata
                .get("columns")
                .and_then(|v| serde_yaml::from_value(v.clone()).ok())
                .filter(|columns: &Vec<BoardColumn>| !columns.is_empty())
                .unwrap_or_else(default_columns);
            let c = cols
                .iter_mut()
                .find(|c| c.id == column_id)
                .ok_or_else(|| "column not found".to_string())?;
            c.name = name.trim().into();
            b.metadata.insert(
                "columns".into(),
                serde_yaml::to_value(cols).map_err(|e| e.to_string())?,
            );
            Ok(())
        })
    }
    #[tauri::command]
    pub fn reorder_columns(path: String, column_ids: Vec<String>) -> Result<BoardInfo, String> {
        save_board_metadata(&path, |b| {
            let cols: Vec<BoardColumn> = b
                .metadata
                .get("columns")
                .and_then(|v| serde_yaml::from_value(v.clone()).ok())
                .filter(|columns: &Vec<BoardColumn>| !columns.is_empty())
                .unwrap_or_else(default_columns);
            if column_ids.len() != cols.len() {
                return Err("column order must include every column exactly once".into());
            }
            let mut by_id: HashMap<String, BoardColumn> = cols
                .into_iter()
                .map(|column| (column.id.clone(), column))
                .collect();
            let mut reordered = Vec::with_capacity(column_ids.len());
            for id in &column_ids {
                reordered.push(by_id.remove(id).ok_or_else(|| {
                    "column order contains an unknown or duplicate column".to_string()
                })?);
            }
            if !by_id.is_empty() {
                return Err("column order must include every column exactly once".into());
            }
            b.metadata.insert(
                "columns".into(),
                serde_yaml::to_value(reordered).map_err(|e| e.to_string())?,
            );
            Ok(())
        })
    }
    #[tauri::command]
    pub fn create_label(path: String, name: String, color: String) -> Result<BoardInfo, String> {
        let name = name.trim().to_string();
        let color = color.trim().to_string();
        if name.is_empty()
            || name.len() > 80
            || color.is_empty()
            || color.len() > 200
            || color.chars().any(|c| matches!(c, '<' | '>' | ';' | '\"'))
        {
            return Err("invalid label name or color".into());
        }
        save_board_metadata(&path, |b| {
            let mut labels: BTreeMap<String, String> = b
                .metadata
                .get("labels")
                .and_then(|v| serde_yaml::from_value(v.clone()).ok())
                .unwrap_or_default();
            if labels.contains_key(&name) {
                return Err("label already exists".into());
            }
            labels.insert(name.clone(), color.clone());
            b.metadata.insert(
                "labels".into(),
                serde_yaml::to_value(labels).map_err(|e| e.to_string())?,
            );
            Ok(())
        })
    }
    #[tauri::command]
    pub fn update_label(
        path: String,
        name: String,
        new_name: String,
        color: String,
    ) -> Result<BoardInfo, String> {
        let name = name.trim().to_string();
        let new_name = new_name.trim().to_string();
        let color = color.trim().to_string();
        if name.is_empty()
            || new_name.is_empty()
            || new_name.len() > 80
            || color.is_empty()
            || color.len() > 200
            || color.contains(['<', '>', ';', '\"'])
        {
            return Err("invalid label name or color".into());
        }
        save_board_metadata(&path, |b| {
            let mut labels: BTreeMap<String, String> = b
                .metadata
                .get("labels")
                .and_then(|v| serde_yaml::from_value(v.clone()).ok())
                .unwrap_or_default();
            if !labels.contains_key(&name) {
                return Err("label not found".into());
            }
            if name != new_name && labels.contains_key(&new_name) {
                return Err("label already exists".into());
            }
            labels.remove(&name);
            labels.insert(new_name.clone(), color.clone());
            b.metadata.insert(
                "labels".into(),
                serde_yaml::to_value(labels).map_err(|e| e.to_string())?,
            );
            Ok(())
        })
    }
    #[tauri::command]
    pub fn delete_label(path: String, name: String) -> Result<BoardInfo, String> {
        save_board_metadata(&path, |b| {
            let mut labels: BTreeMap<String, String> = b
                .metadata
                .get("labels")
                .and_then(|v| serde_yaml::from_value(v.clone()).ok())
                .unwrap_or_default();
            labels
                .remove(name.trim())
                .ok_or_else(|| "label not found".to_string())?;
            b.metadata.insert(
                "labels".into(),
                serde_yaml::to_value(labels).map_err(|e| e.to_string())?,
            );
            Ok(())
        })
    }
    #[tauri::command]
    pub fn create_column(path: String, name: String) -> Result<BoardInfo, String> {
        if name.trim().is_empty() {
            return Err("column name must be nonempty".into());
        }
        save_board_metadata(&path, |b| {
            let mut cols: Vec<BoardColumn> = b
                .metadata
                .get("columns")
                .and_then(|v| serde_yaml::from_value(v.clone()).ok())
                .filter(|columns: &Vec<BoardColumn>| !columns.is_empty())
                .unwrap_or_else(default_columns);
            let base = slug(name.trim());
            let mut id = base.clone();
            let mut n = 2;
            while cols.iter().any(|c| c.id == id) {
                id = format!("{base}-{n}");
                n += 1;
            }
            cols.push(BoardColumn {
                id,
                name: name.trim().into(),
                extra: HashMap::new(),
            });
            b.metadata.insert(
                "columns".into(),
                serde_yaml::to_value(cols).map_err(|e| e.to_string())?,
            );
            Ok(())
        })
    }
    /// Only media inside an opened board is exposed to the Markdown renderer.
    #[tauri::command]
    pub fn read_attachment(path: String, relative: String) -> Result<tauri::ipc::Response, String> {
        read_board_attachment(&path, &relative).map(tauri::ipc::Response::new)
    }
    #[tauri::command]
    pub fn list_cards(path: String) -> Result<Vec<CardInfo>, String> {
        let s = board(&path)?;
        let cards: Vec<CardInfo> = s
            .list_cards()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|id| {
                let md = s.read_card(&id).map_err(|e| e.to_string())?;
                card_info(id, &md)
            })
            .collect::<Result<_, _>>()?;
        let root = Path::new(&path)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(&path));
        let mut runtime = runtime().lock().unwrap();
        for card in &cards {
            if let Some(revision) = card.revision.as_ref() {
                runtime
                    .last_revisions
                    .entry(revision_key(&root, &card.id))
                    .or_insert_with(|| revision.clone());
            }
        }
        Ok(cards)
    }
    #[tauri::command]
    pub fn list_archived_cards(path: String) -> Result<Vec<ArchivedCard>, String> {
        archived_cards(&path)
    }
    #[tauri::command]
    pub fn restore_card(path: String, id: String, revision_id: String) -> Result<CardInfo, String> {
        if board(&path)?.read_card(&id).is_ok() {
            return Err("card is already active".into());
        }
        let archived = archived_cards(&path)?
            .into_iter()
            .find(|item| item.card.id == id && item.revision_id == revision_id)
            .ok_or_else(|| "archived card revision not found".to_string())?;
        with_mutation(
            &path,
            &id,
            CardInput {
                title: archived.card.title,
                body: archived.card.body,
                column: archived.card.column,
                labels: archived.card.labels,
                label_colors: archived.card.label_colors,
                position: Some(archived.card.position),
                due: archived.card.due,
                start: archived.card.start,
            },
            Some(vec![revision_id]),
        )
    }
    #[tauri::command]
    pub fn read_card(path: String, id: String) -> Result<CardInfo, String> {
        let s = board(&path)?;
        card_info(id.clone(), &s.read_card(&id).map_err(|e| e.to_string())?)
    }
    #[tauri::command]
    pub fn add_card(path: String, input: CardInput) -> Result<CardInfo, String> {
        let id = ulid::Ulid::new().to_string();
        with_mutation(&path, &id, input, None)
    }
    #[tauri::command]
    pub fn update_card(path: String, id: String, input: CardInput) -> Result<CardInfo, String> {
        with_mutation(&path, &id, input, None)
    }
    #[tauri::command]
    pub fn resolve_conflict(
        path: String,
        id: String,
        resolution: ConflictResolutionInput,
    ) -> Result<CardInfo, String> {
        if resolution.parent_revision_ids.len() != 2
            || resolution.parent_revision_ids[0] == resolution.parent_revision_ids[1]
            || resolution
                .parent_revision_ids
                .iter()
                .any(|p| !valid_revision_id(p))
        {
            return Err(
                "conflict resolution requires two distinct valid parent revision IDs".into(),
            );
        }
        let revisions_dir = revisions_dir(Path::new(&path))?;
        for parent in &resolution.parent_revision_ids {
            if !revisions_dir.join(format!("{parent}.json")).is_file() {
                return Err(format!("parent revision not found: {parent}"));
            }
        }
        let selected = match resolution.choice.as_str() {
            "local" => resolution.local,
            "remote" => resolution.remote,
            "manual" => resolution
                .manual
                .ok_or_else(|| "manual resolution payload is required".to_string())?,
            _ => return Err("choice must be local, remote, or manual".into()),
        };
        let board_mutex = board_lock(Path::new(&path));
        let _io = board_mutex.lock().unwrap_or_else(|e| e.into_inner());
        let mut store = board(&path)?;
        let old = store.read_card(&id).ok();
        if resolution.tombstone {
            let deleted_info = CardInfo {
                id: id.clone(),
                title: selected.title.clone(),
                body: selected.body.clone(),
                column: selected.column.clone(),
                position: selected.position.unwrap_or(1000),
                labels: selected.labels.clone(),
                label_colors: selected.label_colors.clone(),
                due: selected.due.clone(),
                start: selected.start.clone(),
                updated_at: None,
                revision: None,
            };
            let revision_id = revision();
            let rev = tombstone(
                revision_id,
                id.clone(),
                resolution.parent_revision_ids.clone(),
                chrono::Utc::now().timestamp() as u64,
            );
            let deleted_path = store
                .card_path(&id)
                .ok()
                .map(|path| path.to_string_lossy().into_owned());
            if let Some(deleted_path) = &deleted_path {
                runtime()
                    .lock()
                    .unwrap()
                    .self_deletes
                    .insert(deleted_path.clone());
            }
            if deleted_path.is_some()
                && let Err(error) = store.delete_card(&id)
            {
                if let Some(deleted_path) = &deleted_path {
                    runtime().lock().unwrap().self_deletes.remove(deleted_path);
                }
                return Err(error.to_string());
            }
            if let Err(error) = persist_revision(Path::new(&path), &rev) {
                if let Some(old) = &old {
                    let _ = tracked_write(&mut store, &id, old);
                }
                if let Some(deleted_path) = &deleted_path {
                    runtime().lock().unwrap().self_deletes.remove(deleted_path);
                }
                return Err(error);
            }
            runtime()
                .lock()
                .unwrap()
                .last_revisions
                .insert(revision_key(Path::new(&path), &id), rev.revision_id.clone());
            save_store(&path, store);
            return Ok(old
                .as_deref()
                .and_then(|markdown| card_info(id.clone(), markdown).ok())
                .unwrap_or(deleted_info));
        }
        let old = old.ok_or_else(|| "card not found".to_string())?;
        let mut fm = Card::parse(&old).map_err(|e| e.to_string())?.frontmatter;
        fm.title = selected.title;
        fm.column = selected.column;
        fm.position = selected.position.unwrap_or(fm.position);
        fm.labels = selected.labels;
        if !selected.label_colors.is_empty() {
            fm.extra.insert(
                "label_colors".into(),
                serde_yaml::to_value(selected.label_colors).map_err(|e| e.to_string())?,
            );
        }
        fm.due = selected.due;
        fm.extra.remove("start");
        if let Some(start) = selected.start {
            fm.extra
                .insert("start".into(), serde_yaml::Value::String(start));
        }
        let revision_id = revision();
        fm.sync = Some(SyncMetadata {
            revision: Some(revision_id.clone()),
            parents: resolution.parent_revision_ids.clone(),
            content_hash: None,
            updated_at: Some(now()),
            extra: Default::default(),
        });
        let markdown = Card {
            frontmatter: fm,
            body: selected.body,
        }
        .to_markdown()
        .map_err(|e| e.to_string())?;
        tracked_write(&mut store, &id, &markdown)?;
        let rev = snapshot(
            revision_id,
            id.clone(),
            resolution.parent_revision_ids,
            chrono::Utc::now().timestamp() as u64,
            markdown.clone(),
        );
        if let Err(error) = persist_revision(Path::new(&path), &rev) {
            let _ = tracked_write(&mut store, &id, &old);
            return Err(error);
        }
        runtime()
            .lock()
            .unwrap()
            .last_revisions
            .insert(revision_key(Path::new(&path), &id), rev.revision_id.clone());
        save_store(&path, store);
        card_info(id, &markdown)
    }
    #[tauri::command]
    pub fn move_card(
        path: String,
        id: String,
        column: String,
        position: Option<i64>,
    ) -> Result<CardInfo, String> {
        let old = read_card(path.clone(), id.clone())?;
        with_mutation(
            &path,
            &id,
            CardInput {
                title: old.title,
                body: old.body,
                column,
                labels: old.labels,
                label_colors: old.label_colors,
                position,
                due: old.due,
                start: old.start,
            },
            None,
        )
    }
    /// Return the canonical absolute filesystem path for a card.
    #[tauri::command]
    pub fn share_path(path: String, id: String) -> Result<String, String> {
        if !knot_core::validate_ulid(&id) {
            return Err("card id must be a valid ULID".into());
        }
        let root = Path::new(&path).canonicalize().map_err(|e| e.to_string())?;
        let store = FsBoardStore::open(&root).map_err(|e| e.to_string())?;
        let card = store.card_path(&id).map_err(|e| e.to_string())?;
        Ok(card.to_string_lossy().into_owned())
    }

    #[tauri::command]
    pub fn delete_card(path: String, id: String) -> Result<bool, String> {
        let board_mutex = board_lock(Path::new(&path));
        let _io = board_mutex.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = board(&path)?;
        let old = s.read_card(&id).map_err(|e| e.to_string())?;
        let parsed = Card::parse(&old).map_err(|e| e.to_string())?;
        let parents = parsed
            .frontmatter
            .sync
            .and_then(|x| x.revision)
            .into_iter()
            .collect();
        let rev = tombstone(
            revision(),
            id.clone(),
            parents,
            chrono::Utc::now().timestamp() as u64,
        );
        let delete_path = s
            .card_path(&id)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        runtime()
            .lock()
            .unwrap()
            .self_deletes
            .insert(delete_path.clone());
        if let Err(error) = s.delete_card(&id) {
            runtime().lock().unwrap().self_deletes.remove(&delete_path);
            return Err(error.to_string());
        }
        if let Err(error) = persist_revision(Path::new(&path), &rev) {
            let _ = tracked_write(&mut s, &id, &old);
            runtime().lock().unwrap().self_deletes.remove(&delete_path);
            return Err(error);
        }
        runtime().lock().unwrap().last_revisions.insert(
            revision_key(
                &Path::new(&path)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&path)),
                &id,
            ),
            rev.revision_id.clone(),
        );
        save_store(&path, s);
        Ok(true)
    }
    #[tauri::command]
    pub fn watch_board(path: String) -> Result<bool, String> {
        let root = PathBuf::from(&path);
        if !root.is_dir() {
            return Err("board folder does not exist".into());
        }
        let watch_key = root
            .canonicalize()
            .unwrap_or_else(|_| root.clone())
            .to_string_lossy()
            .into_owned();
        {
            let r = runtime().lock().unwrap();
            if r.watched_paths.contains(&watch_key) {
                return Ok(true);
            }
        }
        let event_root = root.clone();
        let callback_root = event_root.clone();
        #[cfg(target_os = "ios")]
        let mut watcher = PollWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(ev) = res {
                    for p in ev.paths {
                        process_external_event(&callback_root, &p, &ev.kind);
                    }
                }
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )
        .map_err(|e| e.to_string())?;
        #[cfg(not(target_os = "ios"))]
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(ev) = res {
                    for p in ev.paths {
                        process_external_event(&callback_root, &p, &ev.kind);
                    }
                }
            },
            Config::default(),
        )
        .map_err(|e| e.to_string())?;
        watcher
            .watch(&event_root, RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;
        let mut r = runtime().lock().unwrap();
        if r.watched_paths.insert(watch_key) {
            r.watchers.push(watcher);
        }
        Ok(true)
    }
    #[tauri::command]
    pub fn poll_watch_events() -> Vec<WatchEvent> {
        let mut r = runtime().lock().unwrap();
        r.events.drain(..).collect()
    }
    #[tauri::command]
    pub fn pair_peer(peer_id: String) -> Result<PeerInfo, String> {
        EndpointId::from_str(&peer_id).map_err(|e| format!("invalid endpoint id: {e}"))?;
        Err(
            "pairing requires endpoint address and authorized board IDs; use pair_peer_address"
                .into(),
        )
    }
    #[tauri::command]
    pub fn list_trusted_peers() -> Vec<PeerInfo> {
        runtime()
            .lock()
            .unwrap()
            .config
            .peers
            .iter()
            .map(|(peer_id, peer)| PeerInfo {
                peer_id: peer_id.clone(),
                trusted: true,
                address: peer.address.clone(),
            })
            .collect()
    }
    #[tauri::command]
    pub async fn endpoint_info() -> Result<EndpointInfo, String> {
        let t = ensure_transport().await?;
        Ok(EndpointInfo {
            endpoint_id: t.endpoint_id().to_string(),
            address: serde_json::to_string(&t.endpoint().addr()).unwrap_or_else(|_| "{}".into()),
        })
    }
    #[tauri::command]
    pub fn pair_peer_address(
        peer_id: String,
        address: String,
        authorized_boards: Option<Vec<String>>,
    ) -> Result<PeerInfo, String> {
        let id = EndpointId::from_str(&peer_id).map_err(|e| format!("invalid endpoint id: {e}"))?;
        let addr = serde_json::from_str::<EndpointAddr>(&address)
            .map_err(|e| format!("invalid endpoint address: {e}"))?;
        if id != addr.id {
            return Err("endpoint id does not match address".into());
        }
        let mut r = runtime().lock().unwrap();
        let requested = authorized_boards.ok_or("authorized board IDs are required")?;
        if requested.is_empty() || requested.iter().any(|id| !knot_core::validate_ulid(id)) {
            return Err("authorized boards must be valid board ULIDs".into());
        }
        let boards = requested.into_iter().collect();
        r.peers.insert(peer_id.clone());
        r.config.peers.insert(
            peer_id.clone(),
            InstallPeer {
                endpoint_id: peer_id.clone(),
                address: Some(serde_json::to_string(&addr).unwrap_or(address.clone())),
                authorized_boards: boards,
            },
        );
        save_config(&r.config)?;
        Ok(PeerInfo {
            peer_id,
            trusted: true,
            address: Some(serde_json::to_string(&addr).unwrap_or(address)),
        })
    }
    #[tauri::command]
    pub async fn sync_board(
        path: String,
        peer_id: String,
        address: String,
    ) -> Result<SyncResult, String> {
        let peer =
            EndpointId::from_str(&peer_id).map_err(|e| format!("invalid endpoint id: {e}"))?;
        let addr = serde_json::from_str::<EndpointAddr>(&address)
            .map_err(|e| format!("invalid endpoint address: {e}"))?;
        if peer != addr.id {
            return Err("endpoint id does not match address".into());
        }
        let board_id = board_identity(Path::new(&path))?;
        let (cfg, t) = {
            let r = runtime().lock().unwrap();
            if !r.peers.contains(&peer_id) {
                return Err("untrusted peer".into());
            }
            let p = r
                .config
                .peers
                .get(&peer_id)
                .ok_or("peer is not paired with address")?;
            if !p.authorized_boards.contains(&board_id) {
                return Err("board unauthorized".into());
            }
            (r.config.clone(), r.transport.clone())
        };
        let t = match t {
            Some(t) => t,
            None => ensure_transport().await?,
        };
        let local_endpoint_id = t.endpoint_id().to_string();
        let mut wire = ConnectedIrohTransport::connect(t, addr)
            .await
            .map_err(|e| e.to_string())?;
        let repo = load_repo(&path)?;
        let before = repo.revisions.len();
        let mut identity = DeviceIdentity::new(cfg.device_name);
        identity.device_id = cfg.device_id;
        identity.endpoint_id = local_endpoint_id;
        identity.trust(peer_id.clone());
        identity.authorize_board(board_id.clone());
        let shared = Arc::new(tokio::sync::Mutex::new(repo));
        let conflicts =
            knot_sync::sync_once(&mut wire, shared.clone(), &identity, &peer_id, &board_id)
                .await
                .map_err(|e| e.to_string())?;
        let repo = Arc::try_unwrap(shared)
            .map_err(|_| "sync busy".to_string())?
            .into_inner();
        let received = repo.revisions.len().saturating_sub(before);
        materialize(&path, &repo)?;
        let conflicts: Vec<SyncConflict> = conflicts
            .into_iter()
            .map(|conflict| {
                let local_revision_id = conflict.local.id.clone();
                let remote_revision_id = conflict.remote.id.clone();
                Ok(SyncConflict {
                    card_id: conflict.card_id.clone(),
                    local_revision_id,
                    remote_revision_id,
                    parent_revision_ids: vec![
                        conflict.local.id.clone(),
                        conflict.remote.id.clone(),
                    ],
                    local: if conflict.local.tombstone {
                        None
                    } else {
                        Some(card_info(
                            conflict.card_id.clone(),
                            &conflict.local.content,
                        )?)
                    },
                    remote: if conflict.remote.tombstone {
                        None
                    } else {
                        Some(card_info(conflict.card_id, &conflict.remote.content)?)
                    },
                    local_tombstone: conflict.local.tombstone,
                    remote_tombstone: conflict.remote.tombstone,
                })
            })
            .collect::<Result<_, String>>()?;
        let status = if conflicts.is_empty() {
            "connected"
        } else {
            "conflict"
        };
        let mut r = runtime().lock().unwrap();
        r.last_connection = status.into();
        Ok(SyncResult {
            status: status.into(),
            transferred: received,
            received,
            conflicts,
        })
    }
    #[tauri::command]
    pub fn app_capabilities() -> AppCapabilities {
        AppCapabilities {
            local_ai: cfg!(feature = "local-ai"),
            remote_ai: cfg!(feature = "remote-ai"),
            ai_available: cfg!(any(feature = "local-ai", feature = "remote-ai")),
            mobile: cfg!(mobile),
        }
    }
    #[tauri::command]
    pub fn open_default_board() -> Result<BoardInfo, String> {
        let path = app_paths::documents_dir()?.join("Default Board");
        open_or_create_board(path.to_string_lossy().into_owned())
    }
    #[tauri::command]
    pub async fn dial_remote_peer(address_json: String) -> Result<serde_json::Value, String> {
        let result =
            knot_iroh::diagnose_remote_peer_json(&address_json, std::time::Duration::from_secs(20))
                .await
                .map_err(|error| error.to_string())?;
        Ok(serde_json::json!({
            "peer": result.peer.to_string(),
            "hello_acknowledged": result.hello_acknowledged,
            "path": result.path,
            "relay_only_requested": result.relay_only_requested,
        }))
    }
    #[tauri::command]
    pub fn sync_status() -> SyncStatus {
        let r = runtime().lock().unwrap();
        SyncStatus {
            connected: r.last_connection != "disconnected",
            trusted_peers: r.peers.len(),
            pending_events: r.events.len(),
            endpoint_id: r.transport.as_ref().map(|t| t.endpoint_id().to_string()),
            address: r.transport.as_ref().map(|t| {
                serde_json::to_string(&t.endpoint().addr()).unwrap_or_else(|_| "{}".into())
            }),
            connection: r.last_connection.clone(),
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct AppCapabilities {
    pub local_ai: bool,
    pub remote_ai: bool,
    pub ai_available: bool,
    pub mobile: bool,
}

#[cfg(not(test))]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            #[cfg(mobile)]
            {
                let data = _app.path().app_data_dir().map_err(|e| e.to_string())?;
                let documents = _app.path().document_dir().map_err(|e| e.to_string())?;
                app_paths::initialize(data, documents).map_err(std::io::Error::other)?;
            }
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::open_board,
            commands::open_default_board,
            commands::app_capabilities,
            commands::dial_remote_peer,
            commands::create_board,
            commands::open_or_create_board,
            commands::rename_board,
            commands::rename_column,
            commands::reorder_columns,
            commands::create_column,
            commands::create_label,
            commands::update_label,
            commands::delete_label,
            commands::list_cards,
            commands::read_attachment,
            commands::list_archived_cards,
            commands::restore_card,
            commands::read_card,
            commands::add_card,
            commands::update_card,
            commands::resolve_conflict,
            commands::share_path,
            commands::delete_card,
            commands::move_card,
            commands::watch_board,
            commands::poll_watch_events,
            commands::pair_peer,
            commands::pair_peer_address,
            commands::endpoint_info,
            commands::sync_board,
            commands::list_trusted_peers,
            commands::sync_status,
            commands::model_settings,
            commands::save_model_settings,
            commands::list_local_models,
            commands::delete_local_model,
            commands::delete_all_local_models,
            commands::list_ollama_models,
            commands::list_openai_models,
            commands::check_openai_access,
            commands::search_huggingface_models,
            commands::download_huggingface_gguf,
            commands::parse_quick_add,
            commands::load_local_model,
            commands::unload_local_model,
            commands::local_model_loaded,
            commands::inspect_local_model
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
#[cfg(test)]
mod tests {
    use super::commands::*;
    use super::*;

    #[test]
    fn app_capabilities_reports_desktop_local_ai_state() {
        let capabilities = app_capabilities();
        assert!(!capabilities.mobile);
        assert_eq!(capabilities.local_ai, cfg!(feature = "local-ai"));
        assert_eq!(capabilities.remote_ai, cfg!(feature = "remote-ai"));
        assert_eq!(
            capabilities.ai_available,
            cfg!(any(feature = "local-ai", feature = "remote-ai"))
        );
    }

    #[tokio::test]
    #[cfg(not(feature = "remote-ai"))]
    async fn remote_commands_return_actionable_feature_errors() {
        let error = list_ollama_models(None).await.unwrap_err();
        assert!(error.contains("remote-ai feature"));
        let error = list_openai_models(None, None).await.unwrap_err();
        assert!(error.contains("remote-ai feature"));
    }

    #[test]
    #[cfg(not(feature = "local-ai"))]
    fn stub_commands_return_actionable_local_ai_errors() {
        assert!(
            load_local_model("missing".into())
                .unwrap_err()
                .contains("local AI is disabled")
        );
        assert!(
            unload_local_model()
                .unwrap_err()
                .contains("local AI is disabled")
        );
        assert!(
            inspect_local_model("missing".into())
                .unwrap_err()
                .contains("local AI is disabled")
        );
    }

    #[cfg(all(feature = "local-ai", not(feature = "remote-ai")))]
    #[test]
    fn local_only_rejects_persisting_remote_provider() {
        let settings = model_manager::ModelSettings {
            provider: Some(model_manager::ModelProvider::Ollama),
            ..Default::default()
        };
        assert!(
            save_model_settings(settings)
                .unwrap_err()
                .contains("remote-ai feature")
        );
    }

    #[cfg(all(feature = "remote-ai", not(feature = "local-ai")))]
    #[test]
    fn remote_only_rejects_persisting_local_provider() {
        let settings = model_manager::ModelSettings {
            provider: Some(model_manager::ModelProvider::HuggingFace),
            ..Default::default()
        };
        assert!(
            save_model_settings(settings)
                .unwrap_err()
                .contains("local-ai feature")
        );
    }

    #[cfg(not(any(feature = "local-ai", feature = "remote-ai")))]
    #[tokio::test]
    async fn no_ai_commands_reject_before_reading_settings_or_board() {
        let settings_error = model_settings().unwrap_err();
        assert!(settings_error.contains("AI is disabled in this build"));
        let save_error = save_model_settings(model_manager::ModelSettings::default()).unwrap_err();
        assert!(save_error.contains("AI is disabled in this build"));
        let quick_add_error = parse_quick_add("/missing-board".into(), "task".into())
            .await
            .unwrap_err();
        assert!(quick_add_error.contains("AI is disabled in this build"));
    }

    #[tokio::test]
    async fn dial_remote_peer_rejects_malformed_diagnostic_input() {
        let error = dial_remote_peer("not-json".into()).await.unwrap_err();
        assert!(error.contains("invalid EndpointAddr JSON"));
    }

    #[test]
    fn read_board_attachment_confines_media_to_board() {
        let root = std::env::temp_dir().join(format!("knot-attachments-{}", ulid::Ulid::new()));
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("assets/test.png"), b"image").unwrap();
        fs::write(root.join("secret.txt"), b"secret").unwrap();
        let path = root.to_str().unwrap();
        assert_eq!(
            read_board_attachment(path, "assets/test.png").unwrap(),
            b"image"
        );
        assert!(read_board_attachment(path, "../secret.png").is_err());
        assert!(read_board_attachment(path, "secret.txt").is_err());
        assert!(
            read_board_attachment(path, root.join("assets/test.png").to_str().unwrap()).is_err()
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(std::env::temp_dir(), root.join("outside")).unwrap();
            assert!(read_board_attachment(path, "outside/missing.png").is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_json_from_chatty_model_responses() {
        let raw = r#"Here is the result:
```json
{"title":"Ship it","body":"","column":"doing","labels":[],"due":null,"start":null,"confidence":0.9,"warnings":[]}
```"#;
        let draft = parse_model_draft(raw).unwrap();
        assert_eq!(draft.title, "Ship it");
        assert_eq!(draft.column, "doing");
        let compact = parse_model_json(
            "Ignore {unfinished prose. Result: ```json\n{\"t\":\"Ship it\",\"b\":\"\",\"c\":\"doing\",\"l\":[],\"m\":{},\"d\":null,\"s\":null}\n```",
        )
        .unwrap();
        let expanded = quick_add::expand_compact(&compact).unwrap();
        assert_eq!(expanded["title"], "Ship it");
        assert_eq!(expanded["column"], "doing");
        let nulls = parse_model_draft(
            r#"{"title":"Task","body":null,"column":null,"labels":null,"confidence":null,"warnings":null}"#,
        )
        .unwrap();
        assert_eq!(nulls.confidence, 0.0);
        assert!(nulls.labels.is_empty());
        assert!(
            parse_model_draft("   ")
                .unwrap_err()
                .contains("empty response")
        );
    }

    #[test]
    fn board_metadata_commands_persist() {
        let p = std::env::temp_dir().join(format!("knot-board-meta-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        let path = p.to_string_lossy().into_owned();
        let created = create_board(path.clone()).unwrap();
        assert_eq!(created.title, "My board");
        assert_eq!(
            created
                .columns
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            vec!["backlog", "doing", "done"]
        );
        let renamed = rename_board(path.clone(), "Roadmap".into()).unwrap();
        assert_eq!(renamed.title, "Roadmap");
        let renamed_col =
            rename_column(path.clone(), "doing".into(), "In progress".into()).unwrap();
        assert_eq!(renamed_col.columns[1].name, "In progress");
        let added = create_column(path.clone(), "Review queue".into()).unwrap();
        assert_eq!(added.columns.len(), 4);
        let reopened = open_board(path.clone()).unwrap();
        assert_eq!(reopened.title, "Roadmap");
        assert_eq!(reopened.columns, added.columns);
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn metadata_migration_preserves_column_extensions_and_failed_writes() {
        let p = std::env::temp_dir().join(format!("knot-board-edge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(p.join("cards")).unwrap();
        let path = p.to_string_lossy().into_owned();
        fs::write(
            p.join("board.md"),
            "---
id: 01ARZ3NDEKTSV4RRFFQ69G5FAV
title: Edge
columns:
  - id: todo
    name: Todo
    color: blue
---

body",
        )
        .unwrap();
        rename_column(path.clone(), "todo".into(), "To do".into()).unwrap();
        assert!(
            fs::read_to_string(p.join("board.md"))
                .unwrap()
                .contains("color: blue")
        );

        fs::write(
            p.join("board.md"),
            "---
id: 01ARZ3NDEKTSV4RRFFQ69G5FAV
title: Edge
columns: []
---

body",
        )
        .unwrap();
        let migrated = open_board(path.clone()).unwrap();
        assert_eq!(migrated.columns.len(), 3);
        assert_eq!(
            create_column(path.clone(), "QA".into())
                .unwrap()
                .columns
                .len(),
            4
        );

        let invalid = "---
id: invalid
title: Before
---

body";
        fs::write(p.join("board.md"), invalid).unwrap();
        assert!(rename_board(path, "After".into()).is_err());
        assert_eq!(fs::read_to_string(p.join("board.md")).unwrap(), invalid);
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn open_or_create_initializes_empty_directories_and_preserves_existing_boards() {
        let p = std::env::temp_dir().join(format!("knot-smart-open-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        let path = p.to_string_lossy().into_owned();
        let created = open_or_create_board(path.clone()).unwrap();
        assert_eq!(created.title, "My board");
        assert!(p.join("board.md").is_file());
        assert!(p.join("cards").is_dir());
        rename_board(path.clone(), "Existing".into()).unwrap();
        let reopened = open_or_create_board(path).unwrap();
        assert_eq!(reopened.title, "Existing");
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn reorders_columns_without_losing_metadata() {
        let p = std::env::temp_dir().join(format!("knot-column-order-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        let path = p.to_string_lossy().into_owned();
        create_board(path.clone()).unwrap();
        let reordered = reorder_columns(
            path.clone(),
            vec!["done".into(), "backlog".into(), "doing".into()],
        )
        .unwrap();
        assert_eq!(
            reordered
                .columns
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            vec!["done", "backlog", "doing"]
        );
        assert!(reorder_columns(path, vec!["done".into(), "done".into(), "doing".into()]).is_err());
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn creates_and_cruds() {
        let p = std::env::temp_dir().join(format!("knot-desktop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        create_board(p.to_string_lossy().into()).unwrap();
        let c = add_card(
            p.to_string_lossy().into(),
            CardInput {
                title: "T".into(),
                body: "B".into(),
                column: "backlog".into(),
                labels: vec![],
                label_colors: Default::default(),
                position: None,
                due: Some("2026-09-12".into()),
                start: Some("2026-09-10".into()),
            },
        )
        .unwrap();
        assert_eq!(
            read_card(p.to_string_lossy().into(), c.id.clone())
                .unwrap()
                .title,
            "T"
        );
        let raw = fs::read_to_string(
            p.join("cards")
                .join(knot_store::card_filename("backlog", "T", &c.id)),
        )
        .unwrap();
        let parsed = Card::parse(&raw).unwrap();
        assert_eq!(parsed.frontmatter.due.as_deref(), Some("2026-09-12"));
        assert_eq!(
            parsed
                .frontmatter
                .extra
                .get("start")
                .and_then(|v| v.as_str()),
            Some("2026-09-10")
        );
        update_card(
            p.to_string_lossy().into(),
            c.id.clone(),
            CardInput {
                title: "U".into(),
                body: "B2".into(),
                column: "done".into(),
                labels: vec![],
                label_colors: Default::default(),
                position: None,
                due: None,
                start: None,
            },
        )
        .unwrap();
        let raw = fs::read_to_string(
            p.join("cards")
                .join(knot_store::card_filename("done", "U", &c.id)),
        )
        .unwrap();
        let parsed = Card::parse(&raw).unwrap();
        assert_eq!(parsed.frontmatter.due.as_deref(), Some("2026-09-12"));
        assert_eq!(
            parsed
                .frontmatter
                .extra
                .get("start")
                .and_then(|v| v.as_str()),
            Some("2026-09-10")
        );
        update_card(
            p.to_string_lossy().into(),
            c.id.clone(),
            CardInput {
                title: "U".into(),
                body: "B2".into(),
                column: "done".into(),
                labels: vec![],
                label_colors: Default::default(),
                position: None,
                due: Some(String::new()),
                start: Some(String::new()),
            },
        )
        .unwrap();
        let raw = fs::read_to_string(
            p.join("cards")
                .join(knot_store::card_filename("done", "U", &c.id)),
        )
        .unwrap();
        let parsed = Card::parse(&raw).unwrap();
        assert_eq!(parsed.frontmatter.due, None);
        assert!(!parsed.frontmatter.extra.contains_key("start"));
        assert!(delete_card(p.to_string_lossy().into(), c.id).unwrap());
        let revisions = fs::read_dir(p.join(".knot/revisions")).unwrap().count();
        assert_eq!(revisions, 4);
        assert!(pair_peer("".into()).is_err());
        assert!(pair_peer("peer-test".into()).is_err());
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn concurrent_revision_collision_publishes_only_one_record() {
        let root = std::env::temp_dir().join(format!("knot-revision-race-{}", ulid::Ulid::new()));
        fs::create_dir_all(&root).unwrap();
        let card_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string();
        let left_body = "---\nid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\ntitle: Left\ncolumn: backlog\nposition: 1000\n---\n\nleft";
        let right_body = "---\nid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\ntitle: Right\ncolumn: backlog\nposition: 1000\n---\n\nright";
        let left = snapshot("same-id", card_id.clone(), vec![], 1, left_body);
        let right = snapshot("same-id", card_id, vec![], 2, right_body);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles = [left, right].map(|revision| {
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                persist_revision(&root, &revision)
            })
        });
        let results = handles.map(|handle| handle.join().unwrap());
        assert_eq!(
            results.iter().filter(|result| result.is_ok()).count(),
            1,
            "results: {results:?}"
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| result
                    .as_ref()
                    .is_err_and(|error| error.contains("collision")))
                .count(),
            1
        );
        let revisions = load_revisions(root.to_str().unwrap()).unwrap();
        assert_eq!(revisions.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn conflict_resolution_persists_both_parents() {
        let p = std::env::temp_dir().join(format!("knot-conflict-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        create_board(p.to_string_lossy().into()).unwrap();
        let c = add_card(
            p.to_string_lossy().into(),
            CardInput {
                title: "base".into(),
                body: "base".into(),
                column: "backlog".into(),
                labels: vec![],
                label_colors: Default::default(),
                position: Some(1),
                due: None,
                start: None,
            },
        )
        .unwrap();
        let first = c.revision.unwrap();
        let second = revision();
        let markdown = FsBoardStore::open(&p).unwrap().read_card(&c.id).unwrap();
        persist_revision(
            &p,
            &snapshot(second.clone(), c.id.clone(), vec![], 1, markdown),
        )
        .unwrap();
        let resolved = resolve_conflict(
            p.to_string_lossy().into(),
            c.id,
            ConflictResolutionInput {
                choice: "manual".into(),
                local: CardInput {
                    title: "local".into(),
                    body: "l".into(),
                    column: "backlog".into(),
                    labels: vec![],
                    label_colors: Default::default(),
                    position: Some(1),
                    due: None,
                    start: None,
                },
                remote: CardInput {
                    title: "remote".into(),
                    body: "r".into(),
                    column: "done".into(),
                    labels: vec![],
                    label_colors: Default::default(),
                    position: Some(2),
                    due: None,
                    start: None,
                },
                manual: Some(CardInput {
                    title: "merged".into(),
                    body: "merged body".into(),
                    column: "doing".into(),
                    labels: vec!["merged".into()],
                    label_colors: Default::default(),
                    position: Some(3),
                    due: None,
                    start: None,
                }),
                tombstone: false,
                parent_revision_ids: vec![first.clone(), second.clone()],
            },
        )
        .unwrap();
        let rev_id = resolved.revision.unwrap();
        let bytes = fs::read(p.join(".knot/revisions").join(format!("{rev_id}.json"))).unwrap();
        let persisted: Revision = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(persisted.parents, vec![first, second]);
        assert_eq!(resolved.title, "merged");
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn share_path_is_posix_root_relative_and_safe() {
        let p = std::env::temp_dir().join(format!("knot-share-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        create_board(p.to_string_lossy().into_owned()).unwrap();
        let card = add_card(
            p.to_string_lossy().into_owned(),
            CardInput {
                title: "x".into(),
                body: "".into(),
                column: "backlog".into(),
                labels: vec![],
                label_colors: Default::default(),
                position: Some(1),
                due: None,
                start: None,
            },
        )
        .unwrap();
        let expected = p
            .canonicalize()
            .unwrap()
            .join("cards")
            .join(knot_store::card_filename("backlog", "x", &card.id))
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            share_path(p.to_string_lossy().into_owned(), card.id).unwrap(),
            expected
        );
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn install_config_roundtrips_identity_and_authorization() {
        let secret = generate_secret_key();
        let endpoint = secret.public().to_string();
        let board = ulid::Ulid::new().to_string();
        let config = InstallConfig {
            device_id: ulid::Ulid::new().to_string(),
            device_name: "device".into(),
            secret_key: Some(secret_key_to_bytes(&secret).to_vec()),
            peers: HashMap::from([(
                endpoint.clone(),
                InstallPeer {
                    endpoint_id: endpoint.clone(),
                    address: None,
                    authorized_boards: [board.clone()].into_iter().collect(),
                },
            )]),
        };
        let encoded = serde_json::to_vec(&config).unwrap();
        let decoded: InstallConfig = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            secret_key_from_bytes(decoded.secret_key.as_ref().unwrap())
                .unwrap()
                .public()
                .to_string(),
            secret.public().to_string()
        );
        assert!(decoded.peers[&endpoint].authorized_boards.contains(&board));
    }
    #[test]
    fn board_identity_migrates_valid_metadata_but_preserves_malformed() {
        let p = std::env::temp_dir().join(format!("knot-board-id-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(p.join("cards")).unwrap();
        fs::write(p.join("board.md"), "---\ntitle: Valid\n---\n\nbody").unwrap();
        let first = board_identity(&p).unwrap();
        assert_eq!(first, board_identity(&p).unwrap());
        fs::write(
            p.join("board.md"),
            "---\ntitle: With columns\ncolumns:\n  - id: todo\n    name: Todo\n---\n\nbody",
        )
        .unwrap();
        let migrated = inspect(p.to_string_lossy().into_owned()).unwrap().board_id;
        assert_eq!(
            migrated,
            inspect(p.to_string_lossy().into_owned()).unwrap().board_id
        );
        let bad = "---\ntitle: [\n---\nuntouched";
        fs::write(p.join("board.md"), bad).unwrap();
        assert!(board_identity(&p).is_err());
        assert_eq!(fs::read_to_string(p.join("board.md")).unwrap(), bad);
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn materialize_applies_only_head_and_tombstone() {
        let p = std::env::temp_dir().join(format!("knot-materialize-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        create_board(p.to_string_lossy().into()).unwrap();
        let mut repo = MemoryRevisionRepository::new(board_identity(&p).unwrap());
        let card_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let old = "---\nid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\ntitle: Old\ncolumn: backlog\nposition: 1000\n---\n\nold";
        let new = "---\nid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\ntitle: New\ncolumn: backlog\nposition: 1000\n---\n\nnew";
        repo.add(knot_sync::Revision {
            id: "z-old".into(),
            card_id: card_id.into(),
            parents: vec![],
            content: old.into(),
            tombstone: false,
        });
        repo.add(knot_sync::Revision {
            id: "a-new".into(),
            card_id: card_id.into(),
            parents: vec!["z-old".into()],
            content: new.into(),
            tombstone: false,
        });
        materialize(p.to_str().unwrap(), &repo).unwrap();
        assert!(
            FsBoardStore::open(&p)
                .unwrap()
                .read_card(card_id)
                .unwrap()
                .contains("title: New")
        );
        assert_eq!(fs::read_dir(p.join(".knot/revisions")).unwrap().count(), 2);
        let imported_path = p.join(".knot/revisions/a-new.json");
        let mut imported: Revision =
            serde_json::from_slice(&fs::read(&imported_path).unwrap()).unwrap();
        imported.timestamp = imported.timestamp.saturating_sub(1);
        fs::write(
            &imported_path,
            serde_json::to_vec_pretty(&imported).unwrap(),
        )
        .unwrap();
        repo.add(knot_sync::Revision {
            id: "b-delete".into(),
            card_id: card_id.into(),
            parents: vec!["a-new".into()],
            content: String::new(),
            tombstone: true,
        });
        materialize(p.to_str().unwrap(), &repo).unwrap();
        assert!(FsBoardStore::open(&p).unwrap().read_card(card_id).is_err());
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn failed_watch_does_not_poison_retry() {
        let p = std::env::temp_dir().join(format!("knot-watch-retry-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        assert!(watch_board(p.to_string_lossy().into()).is_err());
        fs::create_dir_all(&p).unwrap();
        assert!(watch_board(p.to_string_lossy().into()).unwrap());
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn watcher_reports_malformed_external_edit() {
        let p = std::env::temp_dir().join(format!("knot-watch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        create_board(p.to_string_lossy().into()).unwrap();
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        fs::write(
            p.join("cards/a.md"),
            format!(
                "---\nid: {}\ntitle: A\ncolumn: backlog\nposition: 1\n---\n\nbody",
                id
            ),
        )
        .unwrap();
        watch_board(p.to_string_lossy().into()).unwrap();
        let bad = "---\nid: [\n---\nuntouched";
        fs::write(p.join("cards/a.md"), bad).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let ev = poll_watch_events();
        assert!(ev.iter().any(|e| !e.valid && e.error.is_some()));
        assert_eq!(fs::read_to_string(p.join("cards/a.md")).unwrap(), bad);
        let _ = fs::remove_dir_all(p);
    }
}
