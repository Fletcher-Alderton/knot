#![cfg_attr(test, allow(dead_code, unused_imports))]
use iroh::{EndpointAddr, EndpointId};
use kanban_core::{Card, CardFrontmatter, SyncMetadata};
use kanban_iroh::{
    ConnectedIrohTransport, IrohTransport, generate_secret_key, secret_key_from_bytes,
    secret_key_to_bytes,
};
use kanban_revisions::{Revision, snapshot, tombstone};
use kanban_store::{BoardStore, FsBoardStore};
use kanban_sync::{DeviceIdentity, MemoryRevisionRepository, RevisionRepository};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
};

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
    pub updated_at: Option<String>,
    pub revision: Option<String>,
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
pub struct SyncResult {
    pub status: String,
    pub transferred: usize,
    pub received: usize,
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
    pub position: Option<i64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictResolutionInput {
    pub choice: String,
    pub local: CardInput,
    pub remote: CardInput,
    pub manual: Option<CardInput>,
    pub parent_revision_ids: Vec<String>,
}
fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("IROHMD_CONFIG_PATH")
        .or_else(|_| std::env::var("KANBAN_CONFIG_PATH"))
        .or_else(|_| std::env::var("LUNA_CONFIG_PATH"))
    {
        return PathBuf::from(p);
    }
    if let Some(base) = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA")) {
        return PathBuf::from(base).join("IrohMD/install.json");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(".config/irohmd/install.json")
}
fn load_config() -> InstallConfig {
    let p = config_path();
    fs::read(&p)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| InstallConfig {
            device_id: ulid::Ulid::new().to_string(),
            device_name: std::env::var("KANBAN_DEVICE_NAME")
                .unwrap_or_else(|_| "IrohMD Desktop".into()),
            secret_key: None,
            peers: HashMap::new(),
        })
}
fn save_config(c: &InstallConfig) -> Result<(), String> {
    let p = config_path();
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&p, serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

struct Runtime {
    boards: HashMap<String, FsBoardStore>,
    events: VecDeque<WatchEvent>,
    watchers: Vec<RecommendedWatcher>,
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
fn save_store(path: &str, store: FsBoardStore) {
    runtime().lock().unwrap().boards.insert(key(path), store);
}
fn board_identity(path: &Path) -> Result<String, String> {
    let metadata = path.join("board.md");
    let text = fs::read_to_string(&metadata).map_err(|e| e.to_string())?;
    let mut parsed = kanban_core::Board::parse(&text).map_err(|e| e.to_string())?;
    if let Some(id) = parsed.metadata.get("id").and_then(|v| v.as_str())
        && kanban_core::validate_ulid(id)
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
    fs::write(metadata, parsed.to_markdown().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
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
fn board_metadata(path: &Path) -> Result<(kanban_core::Board, String, Vec<BoardColumn>), String> {
    let file = path.join("board.md");
    let text = fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let mut b = kanban_core::Board::parse(&text).map_err(|e| e.to_string())?;
    let mut changed = false;
    let id = match b.metadata.get("id").and_then(|v| v.as_str()) {
        Some(id) if kanban_core::validate_ulid(id) => id.to_string(),
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
    if !b
        .metadata
        .get("title")
        .and_then(|v| v.as_str())
        .is_some_and(|v| !v.trim().is_empty())
    {
        b.metadata
            .insert("title".into(), serde_yaml::Value::String("My board".into()));
        changed = true;
    }
    if changed {
        fs::write(&file, b.to_markdown().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
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
        cards: s.list_cards().map_err(|e| e.to_string())?,
    })
}
fn save_board_metadata(
    path: &str,
    mut f: impl FnMut(&mut kanban_core::Board) -> Result<(), String>,
) -> Result<BoardInfo, String> {
    board_metadata(Path::new(path))?;
    let file = Path::new(path).join("board.md");
    let text = fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let mut b = kanban_core::Board::parse(&text).map_err(|e| e.to_string())?;
    f(&mut b)?;
    fs::write(file, b.to_markdown().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
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
    Ok(CardInfo {
        id,
        title: c.frontmatter.title,
        body: c.body,
        column: c.frontmatter.column,
        position: c.frontmatter.position,
        labels: c.frontmatter.labels,
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
fn persist_revision(root: &Path, r: &Revision) -> Result<(), String> {
    let d = root.join(".kanban/revisions");
    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let p = d.join(format!("{}.json", r.revision_id));
    if !p.exists() {
        fs::write(p, serde_json::to_vec_pretty(r).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn content_hash(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.replace("\r\n", "\n").replace('\r', "\n").as_bytes());
    format!("{:x}", h.finalize())
}

fn revision_key(root: &Path, id: &str) -> String {
    format!("{}\0{}", root.to_string_lossy(), id)
}

fn process_external_event(root: &Path, path: &Path, kind: &EventKind) {
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
                        let mut r = runtime().lock().unwrap();
                        if r.self_hashes
                            .get(&event_key)
                            .map(|h| h == &hash)
                            .unwrap_or(false)
                        {
                            r.self_hashes.remove(&event_key);
                            true
                        } else {
                            false
                        }
                    };
                    if !self_write && !duplicate {
                        let old =
                            fs::read_to_string(root.join("cards").join(format!("{id}.md"))).ok();
                        let parents = old
                            .as_deref()
                            .and_then(|s| Card::parse(s).ok())
                            .and_then(|c| c.frontmatter.sync)
                            .and_then(|s| s.revision)
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
                        let _ = persist_revision(root, &rev);
                        runtime()
                            .lock()
                            .unwrap()
                            .last_revisions
                            .insert(revision_key(root, &id), rid);
                    }
                }
            }
        }
    } else if matches!(kind, EventKind::Remove(_))
        && let Some(id) = path.file_stem().and_then(|x| x.to_str())
    {
        let self_delete = runtime().lock().unwrap().self_deletes.remove(&path_string);
        if self_delete {
            runtime().lock().unwrap().events.push_back(WatchEvent {
                path: path_string,
                kind: format!("{kind:?}"),
                valid: true,
                duplicate: false,
                error: None,
            });
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
        let _ = persist_revision(root, &rev);
        runtime()
            .lock()
            .unwrap()
            .last_revisions
            .insert(revision_key(root, id), rid);
    }
    runtime().lock().unwrap().events.push_back(WatchEvent {
        path: path_string,
        kind: format!("{kind:?}"),
        valid,
        duplicate,
        error,
    });
}

fn with_mutation(path: &str, id: &str, input: CardInput) -> Result<CardInfo, String> {
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
    let parent = fm.sync.as_ref().and_then(|x| x.revision.clone());
    let rid = revision();
    fm.title = input.title;
    fm.column = input.column;
    fm.position = input.position.unwrap_or(fm.position);
    fm.labels = input.labels;
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
    s.write_card(id, &md).map_err(|e| e.to_string())?;
    let canonical_root = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path));
    runtime().lock().unwrap().self_hashes.insert(
        canonical_root
            .join("cards")
            .join(format!("{}.md", id))
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
    persist_revision(Path::new(path), &rev)?;
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
            if kanban_sync::sync_once(&mut wire, shared.clone(), &identity, &remote, &board_id)
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
    let d = Path::new(path).join(".kanban/revisions");
    if d.is_dir() {
        for e in fs::read_dir(d).map_err(|e| e.to_string())?.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) == Some("json")
                && let Ok(r) = serde_json::from_slice::<Revision>(
                    &fs::read(e.path()).map_err(|x| x.to_string())?,
                )
            {
                repo.add(kanban_sync::Revision::from_domain(&r));
            }
        }
    }
    Ok(repo)
}
fn materialize(path: &str, repo: &MemoryRevisionRepository) -> Result<usize, String> {
    let mut n = 0;
    let mut store = board(path)?;
    let all = repo.all();
    for revision in &all {
        persist_revision(
            Path::new(path),
            &revision.to_domain(chrono::Utc::now().timestamp() as u64),
        )?;
    }
    let heads: std::collections::HashSet<_> = repo.heads().into_iter().collect();
    let mut by_card: HashMap<String, Vec<kanban_sync::Revision>> = HashMap::new();
    for r in all.into_iter().filter(|r| heads.contains(&r.id)) {
        by_card.entry(r.card_id.clone()).or_default().push(r);
    }
    for revisions in by_card.into_values().filter(|items| items.len() == 1) {
        let d = revisions[0].to_domain(chrono::Utc::now().timestamp() as u64);
        if d.tombstone {
            if store.read_card(&d.card_id).is_ok() {
                let _ = store.delete_card(&d.card_id);
                n += 1;
            }
        } else if let Some(s) = d.snapshot
            && store.write_card(&d.card_id, &s).is_ok()
        {
            n += 1;
        }
    }
    save_store(path, store);
    Ok(n)
}
mod commands {
    use super::*;
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
            fs::write(
                &b,
                "---
title: My board
---

",
            )
            .map_err(|e| e.to_string())?;
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
    #[tauri::command]
    pub fn list_cards(path: String) -> Result<Vec<CardInfo>, String> {
        let s = board(&path)?;
        s.list_cards()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|id| {
                let md = s.read_card(&id).map_err(|e| e.to_string())?;
                card_info(id, &md)
            })
            .collect()
    }
    #[tauri::command]
    pub fn read_card(path: String, id: String) -> Result<CardInfo, String> {
        let s = board(&path)?;
        card_info(id.clone(), &s.read_card(&id).map_err(|e| e.to_string())?)
    }
    #[tauri::command]
    pub fn add_card(path: String, input: CardInput) -> Result<CardInfo, String> {
        let id = ulid::Ulid::new().to_string();
        with_mutation(&path, &id, input)
    }
    #[tauri::command]
    pub fn update_card(path: String, id: String, input: CardInput) -> Result<CardInfo, String> {
        with_mutation(&path, &id, input)
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
                .any(|p| !kanban_core::validate_ulid(p))
        {
            return Err(
                "conflict resolution requires two distinct valid parent revision IDs".into(),
            );
        }
        let revisions_dir = Path::new(&path).join(".kanban/revisions");
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
        let mut store = board(&path)?;
        let old = store.read_card(&id).map_err(|e| e.to_string())?;
        let mut fm = Card::parse(&old).map_err(|e| e.to_string())?.frontmatter;
        fm.title = selected.title;
        fm.column = selected.column;
        fm.position = selected.position.unwrap_or(fm.position);
        fm.labels = selected.labels;
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
        store
            .write_card(&id, &markdown)
            .map_err(|e| e.to_string())?;
        let rev = snapshot(
            revision_id,
            id.clone(),
            resolution.parent_revision_ids,
            chrono::Utc::now().timestamp() as u64,
            markdown.clone(),
        );
        persist_revision(Path::new(&path), &rev)?;
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
                position,
            },
        )
    }
    #[tauri::command]
    pub fn delete_card(path: String, id: String) -> Result<bool, String> {
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
        persist_revision(Path::new(&path), &rev)?;
        let delete_path = Path::new(&path)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(&path))
            .join("cards")
            .join(format!("{id}.md"))
            .to_string_lossy()
            .into_owned();
        runtime().lock().unwrap().self_deletes.insert(delete_path);
        runtime().lock().unwrap().last_revisions.insert(
            revision_key(
                &Path::new(&path)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&path)),
                &id,
            ),
            rev.revision_id.clone(),
        );
        s.delete_card(&id).map_err(|e| e.to_string())?;
        save_store(&path, s);
        Ok(true)
    }
    #[tauri::command]
    pub fn watch_board(path: String) -> Result<bool, String> {
        let root = PathBuf::from(&path);
        if !root.is_dir() {
            return Err("board folder does not exist".into());
        }
        let event_root = root.clone();
        let callback_root = event_root.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                for p in ev.paths {
                    process_external_event(&callback_root, &p, &ev.kind);
                }
            }
        })
        .map_err(|e| e.to_string())?;
        watcher
            .watch(&event_root, RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;
        runtime().lock().unwrap().watchers.push(watcher);
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
        if requested.is_empty() || requested.iter().any(|id| !kanban_core::validate_ulid(id)) {
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
        kanban_sync::sync_once(&mut wire, shared.clone(), &identity, &peer_id, &board_id)
            .await
            .map_err(|e| e.to_string())?;
        let repo = Arc::try_unwrap(shared)
            .map_err(|_| "sync busy".to_string())?
            .into_inner();
        let received = repo.revisions.len().saturating_sub(before);
        materialize(&path, &repo)?;
        let mut r = runtime().lock().unwrap();
        r.last_connection = "connected".into();
        Ok(SyncResult {
            status: "connected".into(),
            transferred: received,
            received,
        })
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
#[cfg(not(test))]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::open_board,
            commands::create_board,
            commands::open_or_create_board,
            commands::rename_board,
            commands::rename_column,
            commands::create_column,
            commands::list_cards,
            commands::read_card,
            commands::add_card,
            commands::update_card,
            commands::resolve_conflict,
            commands::delete_card,
            commands::move_card,
            commands::watch_board,
            commands::poll_watch_events,
            commands::pair_peer,
            commands::pair_peer_address,
            commands::endpoint_info,
            commands::sync_board,
            commands::list_trusted_peers,
            commands::sync_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
#[cfg(test)]
mod tests {
    use super::commands::*;
    use super::*;
    #[test]
    fn board_metadata_commands_persist() {
        let p = std::env::temp_dir().join(format!("luna-board-meta-{}", std::process::id()));
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
        let p = std::env::temp_dir().join(format!("luna-board-edge-{}", std::process::id()));
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
        let p = std::env::temp_dir().join(format!("irohmd-smart-open-{}", std::process::id()));
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
    fn creates_and_cruds() {
        let p = std::env::temp_dir().join(format!("luna-desktop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        create_board(p.to_string_lossy().into()).unwrap();
        let c = add_card(
            p.to_string_lossy().into(),
            CardInput {
                title: "T".into(),
                body: "B".into(),
                column: "backlog".into(),
                labels: vec![],
                position: None,
            },
        )
        .unwrap();
        assert_eq!(
            read_card(p.to_string_lossy().into(), c.id.clone())
                .unwrap()
                .title,
            "T"
        );
        update_card(
            p.to_string_lossy().into(),
            c.id.clone(),
            CardInput {
                title: "U".into(),
                body: "B2".into(),
                column: "done".into(),
                labels: vec![],
                position: None,
            },
        )
        .unwrap();
        assert!(delete_card(p.to_string_lossy().into(), c.id).unwrap());
        let revisions = fs::read_dir(p.join(".kanban/revisions")).unwrap().count();
        assert_eq!(revisions, 3);
        assert!(pair_peer("".into()).is_err());
        assert!(pair_peer("peer-test".into()).is_err());
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn conflict_resolution_persists_both_parents() {
        let p = std::env::temp_dir().join(format!("luna-conflict-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        create_board(p.to_string_lossy().into()).unwrap();
        let c = add_card(
            p.to_string_lossy().into(),
            CardInput {
                title: "base".into(),
                body: "base".into(),
                column: "backlog".into(),
                labels: vec![],
                position: Some(1),
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
                    position: Some(1),
                },
                remote: CardInput {
                    title: "remote".into(),
                    body: "r".into(),
                    column: "done".into(),
                    labels: vec![],
                    position: Some(2),
                },
                manual: Some(CardInput {
                    title: "merged".into(),
                    body: "merged body".into(),
                    column: "doing".into(),
                    labels: vec!["merged".into()],
                    position: Some(3),
                }),
                parent_revision_ids: vec![first.clone(), second.clone()],
            },
        )
        .unwrap();
        let rev_id = resolved.revision.unwrap();
        let bytes = fs::read(p.join(".kanban/revisions").join(format!("{rev_id}.json"))).unwrap();
        let persisted: Revision = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(persisted.parents, vec![first, second]);
        assert_eq!(resolved.title, "merged");
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
        let p = std::env::temp_dir().join(format!("luna-board-id-{}", std::process::id()));
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
        let p = std::env::temp_dir().join(format!("luna-materialize-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        create_board(p.to_string_lossy().into()).unwrap();
        let mut repo = MemoryRevisionRepository::new(board_identity(&p).unwrap());
        repo.add(kanban_sync::Revision {
            id: "z-old".into(),
            card_id: "card".into(),
            parents: vec![],
            content: "old".into(),
            tombstone: false,
        });
        repo.add(kanban_sync::Revision {
            id: "a-new".into(),
            card_id: "card".into(),
            parents: vec!["z-old".into()],
            content: "new".into(),
            tombstone: false,
        });
        materialize(p.to_str().unwrap(), &repo).unwrap();
        assert_eq!(
            FsBoardStore::open(&p).unwrap().read_card("card").unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_dir(p.join(".kanban/revisions")).unwrap().count(),
            2
        );
        repo.add(kanban_sync::Revision {
            id: "b-delete".into(),
            card_id: "card".into(),
            parents: vec!["a-new".into()],
            content: String::new(),
            tombstone: true,
        });
        materialize(p.to_str().unwrap(), &repo).unwrap();
        assert!(FsBoardStore::open(&p).unwrap().read_card("card").is_err());
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn watcher_reports_malformed_external_edit() {
        let p = std::env::temp_dir().join(format!("luna-watch-{}", std::process::id()));
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
