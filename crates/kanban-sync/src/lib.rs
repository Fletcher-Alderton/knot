//! Transport-independent, validated synchronization protocol (phases 9–13).
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::Mutex;
use ulid::Ulid;

pub const PROTOCOL_VERSION: u16 = 1;
const MAX_MESSAGE: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Revision {
    pub id: String,
    pub card_id: String,
    pub parents: Vec<String>,
    pub content: String,
    pub tombstone: bool,
}
impl Revision {
    pub fn new(
        card_id: impl Into<String>,
        content: impl Into<String>,
        parents: Vec<String>,
    ) -> Self {
        Self {
            id: Ulid::new().to_string(),
            card_id: card_id.into(),
            parents,
            content: content.into(),
            tombstone: false,
        }
    }
    pub fn tombstone(card_id: impl Into<String>, parents: Vec<String>) -> Self {
        Self {
            id: Ulid::new().to_string(),
            card_id: card_id.into(),
            parents,
            content: String::new(),
            tombstone: true,
        }
    }
    pub fn from_domain(value: &kanban_revisions::Revision) -> Self {
        Self {
            id: value.revision_id.clone(),
            card_id: value.card_id.clone(),
            parents: value.parents.clone(),
            content: value.snapshot.clone().unwrap_or_default(),
            tombstone: value.tombstone,
        }
    }
    pub fn to_domain(&self, timestamp: u64) -> kanban_revisions::Revision {
        if self.tombstone {
            kanban_revisions::tombstone(
                self.id.clone(),
                self.card_id.clone(),
                self.parents.clone(),
                timestamp,
            )
        } else {
            kanban_revisions::snapshot(
                self.id.clone(),
                self.card_id.clone(),
                self.parents.clone(),
                timestamp,
                self.content.clone(),
            )
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    pub device_id: String,
    pub endpoint_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoardSummary {
    pub board_id: String,
    pub heads: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevisionAvailable {
    pub revision_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevisionRequest {
    pub revision_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevisionResponse {
    pub revision: Revision,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ack {
    pub revision_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorMessage {
    pub code: String,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "payload")]
pub enum Message {
    Hello(Hello),
    BoardSummary(BoardSummary),
    RevisionAvailable(RevisionAvailable),
    RevisionRequest(RevisionRequest),
    RevisionResponse(RevisionResponse),
    Ack(Ack),
    Error(ErrorMessage),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireMessage {
    pub version: u16,
    pub message: Message,
}
impl WireMessage {
    pub fn new(message: Message) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            message,
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let b = serde_json::to_vec(self).map_err(|e| ProtocolError::Codec(e.to_string()))?;
        if b.len() > MAX_MESSAGE {
            return Err(ProtocolError::TooLarge);
        }
        Ok(b)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_MESSAGE {
            return Err(ProtocolError::TooLarge);
        }
        let m: Self =
            serde_json::from_slice(bytes).map_err(|e| ProtocolError::Codec(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion(self.version));
        }
        match &self.message {
            Message::Hello(h) if h.device_id.is_empty() || h.endpoint_id.is_empty() => {
                Err(ProtocolError::Invalid("empty identity".into()))
            }
            Message::BoardSummary(s) if s.board_id.is_empty() => {
                Err(ProtocolError::Invalid("empty board".into()))
            }
            Message::RevisionResponse(r)
                if r.revision.id.is_empty() || r.revision.card_id.is_empty() =>
            {
                Err(ProtocolError::Invalid("invalid revision".into()))
            }
            _ => Ok(()),
        }
    }
}
#[derive(thiserror::Error, Debug)]
pub enum ProtocolError {
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u16),
    #[error("message too large")]
    TooLarge,
    #[error("invalid message: {0}")]
    Invalid(String),
    #[error("codec error: {0}")]
    Codec(String),
}

#[async_trait]
pub trait PeerTransport: Send + Sync {
    async fn send(&mut self, message: WireMessage) -> Result<(), TransportError>;
    async fn recv(&mut self) -> Result<WireMessage, TransportError>;
}
#[derive(thiserror::Error, Debug)]
pub enum TransportError {
    #[error("channel closed")]
    Closed,
    #[error("protocol: {0}")]
    Protocol(#[from] ProtocolError),
}
struct WireChan {
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
}
pub struct InMemoryTransport {
    chan: WireChan,
}
impl InMemoryTransport {
    pub fn pair(buffer: usize) -> (Self, Self) {
        let (a_tx, a_rx) = tokio::sync::mpsc::channel(buffer);
        let (b_tx, b_rx) = tokio::sync::mpsc::channel(buffer);
        (
            Self {
                chan: WireChan { tx: a_tx, rx: b_rx },
            },
            Self {
                chan: WireChan { tx: b_tx, rx: a_rx },
            },
        )
    }
}
#[async_trait]
impl PeerTransport for InMemoryTransport {
    async fn send(&mut self, m: WireMessage) -> Result<(), TransportError> {
        self.chan
            .tx
            .send(m.encode()?)
            .await
            .map_err(|_| TransportError::Closed)
    }
    async fn recv(&mut self) -> Result<WireMessage, TransportError> {
        let b = self.chan.rx.recv().await.ok_or(TransportError::Closed)?;
        Ok(WireMessage::decode(&b)?)
    }
}

pub trait RevisionRepository: Send + Sync {
    fn board_id(&self) -> &str;
    fn heads(&self) -> Vec<String>;
    fn get(&self, id: &str) -> Option<Revision>;
    fn all(&self) -> Vec<Revision>;
    fn insert(&mut self, r: Revision) -> bool;
}
#[derive(Default)]
pub struct MemoryRevisionRepository {
    pub board: String,
    pub revisions: HashMap<String, Revision>,
    pub applied: HashSet<String>,
}
impl MemoryRevisionRepository {
    pub fn new(board: impl Into<String>) -> Self {
        Self {
            board: board.into(),
            ..Default::default()
        }
    }
    pub fn add(&mut self, r: Revision) {
        self.insert(r);
    }
}
impl RevisionRepository for MemoryRevisionRepository {
    fn board_id(&self) -> &str {
        &self.board
    }
    fn heads(&self) -> Vec<String> {
        let all: HashSet<_> = self.revisions.keys().cloned().collect();
        let parents: HashSet<_> = self
            .revisions
            .values()
            .flat_map(|r| r.parents.iter().cloned())
            .collect();
        let mut x: Vec<_> = all.difference(&parents).cloned().collect();
        x.sort();
        x
    }
    fn get(&self, id: &str) -> Option<Revision> {
        self.revisions.get(id).cloned()
    }
    fn all(&self) -> Vec<Revision> {
        let mut values: Vec<_> = self.revisions.values().cloned().collect();
        values.sort_by(|a, b| a.id.cmp(&b.id));
        values
    }
    fn insert(&mut self, r: Revision) -> bool {
        if self.revisions.contains_key(&r.id) {
            false
        } else {
            self.applied.insert(r.id.clone());
            self.revisions.insert(r.id.clone(), r);
            true
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub card_id: String,
    pub local: Revision,
    pub remote: Revision,
}
#[derive(thiserror::Error, Debug)]
pub enum SyncError {
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("unauthorized peer")]
    Unauthorized,
    #[error("board not authorized")]
    BoardUnauthorized,
    #[error("conflict requires resolution")]
    Conflict,
}
#[derive(Clone, Debug)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub device_name: String,
    pub endpoint_id: String,
    pub trusted_peers: HashSet<String>,
    pub authorized_boards: HashSet<String>,
}
impl DeviceIdentity {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            device_id: Ulid::new().to_string(),
            device_name: name.into(),
            endpoint_id: Ulid::new().to_string(),
            trusted_peers: HashSet::new(),
            authorized_boards: HashSet::new(),
        }
    }
    /// Trust a peer by its manually copied transport EndpointId, not its device ULID.
    pub fn trust(&mut self, endpoint_id: impl Into<String>) {
        self.trusted_peers.insert(endpoint_id.into());
    }
    pub fn authorize_board(&mut self, id: impl Into<String>) {
        self.authorized_boards.insert(id.into());
    }
}

fn merged_id(card_id: &str, parents: &[String], content: &str, tombstone: bool) -> String {
    let mut parents = parents.to_vec();
    parents.sort();
    let mut h = Sha256::new();
    h.update(card_id);
    for p in parents {
        h.update([0]);
        h.update(p);
    }
    h.update([tombstone as u8]);
    h.update(content);
    format!("merge:{}", hex::encode(h.finalize()))
}
fn conflict(local: &Revision, remote: &Revision) -> Box<Conflict> {
    Box::new(Conflict {
        card_id: local.card_id.clone(),
        local: local.clone(),
        remote: remote.clone(),
    })
}
/// Deterministically merges equivalent heads. Ancestry-aware merging is performed by sync_memory.
pub fn merge_revisions(local: &Revision, remote: &Revision) -> Result<Revision, Box<Conflict>> {
    if local.card_id != remote.card_id
        || local.content != remote.content
        || local.tombstone != remote.tombstone
    {
        return Err(conflict(local, remote));
    }
    let mut parents = vec![local.id.clone(), remote.id.clone()];
    parents.sort();
    Ok(Revision {
        id: merged_id(&local.card_id, &parents, &local.content, local.tombstone),
        card_id: local.card_id.clone(),
        parents,
        content: local.content.clone(),
        tombstone: local.tombstone,
    })
}
/// Resolve an explicit conflict without overwriting either parent.
pub fn resolve_conflict(
    conflict: &Conflict,
    content: impl Into<String>,
    tombstone: bool,
) -> Revision {
    let content = content.into();
    let mut parents = vec![conflict.local.id.clone(), conflict.remote.id.clone()];
    parents.sort();
    Revision {
        id: merged_id(&conflict.card_id, &parents, &content, tombstone),
        card_id: conflict.card_id.clone(),
        parents,
        content,
        tombstone,
    }
}
fn ancestors(repo: &MemoryRevisionRepository, id: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut stack = vec![id.to_string()];
    while let Some(x) = stack.pop() {
        if let Some(r) = repo.revisions.get(&x) {
            for p in &r.parents {
                if out.insert(p.clone()) {
                    stack.push(p.clone())
                }
            }
        }
    }
    out
}
fn merge_divergent(
    repo: &MemoryRevisionRepository,
    local: &Revision,
    remote: &Revision,
) -> Result<Revision, Box<Conflict>> {
    if local.card_id != remote.card_id || local.tombstone || remote.tombstone {
        return Err(conflict(local, remote));
    }
    let mut la = ancestors(repo, &local.id);
    la.insert(local.id.clone());
    let mut ra = ancestors(repo, &remote.id);
    ra.insert(remote.id.clone());
    let mut common: Vec<_> = la.intersection(&ra).cloned().collect();
    common.sort();
    let Some(base) = common
        .into_iter()
        .rev()
        .find_map(|id| repo.revisions.get(&id))
    else {
        return Err(conflict(local, remote));
    };
    if base.tombstone {
        return Err(conflict(local, remote));
    }
    let Ok(base_card) = kanban_core::Card::parse(&base.content) else {
        return Err(conflict(local, remote));
    };
    let Ok(local_card) = kanban_core::Card::parse(&local.content) else {
        return Err(conflict(local, remote));
    };
    let Ok(remote_card) = kanban_core::Card::parse(&remote.content) else {
        return Err(conflict(local, remote));
    };
    let result = kanban_merge::merge(&base_card, &local_card, &remote_card);
    if result.is_conflicted() {
        return Err(conflict(local, remote));
    }
    let Ok(content) = result.card.serialize() else {
        return Err(conflict(local, remote));
    };
    let mut parents = vec![local.id.clone(), remote.id.clone()];
    parents.sort();
    Ok(Revision {
        id: merged_id(&local.card_id, &parents, &content, false),
        card_id: local.card_id.clone(),
        parents,
        content,
        tombstone: false,
    })
}

#[derive(Default, Debug, Clone)]
pub struct SyncOutcome {
    pub transferred: usize,
    pub merged: usize,
    pub conflicts: Vec<Conflict>,
}
/// Synchronize two in-memory repositories. The union is applied first, then divergent heads
/// are merged deterministically; unresolved edits remain explicit.
pub fn sync_memory(
    a: &mut MemoryRevisionRepository,
    b: &mut MemoryRevisionRepository,
) -> SyncOutcome {
    let mut out = SyncOutcome::default();
    let all: Vec<Revision> = a
        .revisions
        .values()
        .chain(b.revisions.values())
        .cloned()
        .collect();
    for r in all {
        if a.insert(r.clone()) {
            out.transferred += 1;
        }
        if b.insert(r) {
            out.transferred += 1;
        }
    }
    let mut by_card: HashMap<String, Vec<Revision>> = HashMap::new();
    for r in a.revisions.values() {
        by_card
            .entry(r.card_id.clone())
            .or_default()
            .push(r.clone());
    }
    for (_card, rs) in by_card {
        let ids: HashSet<String> = rs.iter().flat_map(|r| r.parents.iter().cloned()).collect();
        let mut heads: Vec<Revision> = rs.into_iter().filter(|r| !ids.contains(&r.id)).collect();
        heads.sort_by(|x, y| x.id.cmp(&y.id));
        if heads.len() == 2 {
            match merge_divergent(a, &heads[0], &heads[1]) {
                Ok(m) => {
                    a.insert(m.clone());
                    b.insert(m);
                    out.merged += 1;
                }
                Err(c) => out.conflicts.push(*c),
            }
        }
    }
    out
}

async fn send_revisions<T: PeerTransport, R: RevisionRepository>(
    transport: &mut T,
    repo: &Arc<Mutex<R>>,
) -> Result<(), SyncError> {
    let revisions = repo.lock().await.all();
    for revision in revisions {
        transport
            .send(WireMessage::new(Message::RevisionResponse(
                RevisionResponse { revision },
            )))
            .await?;
    }
    transport
        .send(WireMessage::new(Message::Ack(Ack {
            revision_id: "sync-complete".into(),
        })))
        .await?;
    Ok(())
}
fn validate_incoming_revision<R: RevisionRepository>(
    repo: &R,
    r: &Revision,
) -> Result<(), SyncError> {
    if r.id.is_empty()
        || r.card_id.is_empty()
        || r.card_id.len() > 200
        || !r
            .card_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || r.id.len() > 200
        || r.parents.len() > 2
        || r.parents.iter().any(|p| p.is_empty() || p == &r.id)
        || r.parents.windows(2).any(|p| p[0] == p[1])
        || r.content.len() > MAX_MESSAGE
        || (r.tombstone && !r.content.is_empty())
        || (!r.tombstone && r.content.is_empty())
    {
        return Err(SyncError::Transport(TransportError::Protocol(
            ProtocolError::Invalid("invalid revision response".into()),
        )));
    }
    if let Some(existing) = repo.get(&r.id) {
        if existing != *r {
            return Err(SyncError::Transport(TransportError::Protocol(
                ProtocolError::Invalid("revision ID collision".into()),
            )));
        }
        return Ok(());
    }
    // Parents must already be present; sender order is deterministic and staged below.
    if let Some(parent) = r.parents.iter().find(|p| repo.get(p).is_none()) {
        return Err(SyncError::Transport(TransportError::Protocol(
            ProtocolError::Invalid(format!("missing revision parent {parent}")),
        )));
    }
    if r.parents
        .iter()
        .filter_map(|p| repo.get(p))
        .any(|p| p.card_id != r.card_id)
    {
        return Err(SyncError::Transport(TransportError::Protocol(
            ProtocolError::Invalid("revision parent card mismatch".into()),
        )));
    }
    Ok(())
}

async fn receive_revisions<T: PeerTransport, R: RevisionRepository>(
    transport: &mut T,
    repo: &Arc<Mutex<R>>,
) -> Result<(), SyncError> {
    let mut pending = Vec::new();
    loop {
        match transport.recv().await?.message {
            Message::RevisionResponse(response) => pending.push(response.revision),
            Message::Ack(a) if a.revision_id == "sync-complete" => {
                // Import topologically so valid responses may arrive in any order.
                loop {
                    let mut progress = false;
                    let mut remaining = Vec::new();
                    let mut guard = repo.lock().await;
                    for r in pending.drain(..) {
                        if r.parents.iter().all(|p| guard.get(p).is_some()) {
                            validate_incoming_revision(&*guard, &r)?;
                            guard.insert(r);
                            progress = true;
                        } else {
                            remaining.push(r);
                        }
                    }
                    drop(guard);
                    pending = remaining;
                    if pending.is_empty() {
                        break;
                    }
                    if !progress {
                        return Err(SyncError::Transport(TransportError::Protocol(
                            ProtocolError::Invalid("unresolvable revision parent".into()),
                        )));
                    }
                }
                return Ok(());
            }
            _ => {
                return Err(SyncError::Transport(TransportError::Protocol(
                    ProtocolError::Invalid("expected revision or completion ack".into()),
                )));
            }
        }
    }
}

/// Exchanges all missing revisions. Revisions are addressed by IDs, never paths.
pub async fn sync_once<T: PeerTransport, R: RevisionRepository>(
    transport: &mut T,
    repo: Arc<Mutex<R>>,
    identity: &DeviceIdentity,
    remote_endpoint_id: &str,
    board_id: &str,
) -> Result<Vec<Conflict>, SyncError> {
    if !identity.trusted_peers.contains(remote_endpoint_id) {
        return Err(SyncError::Unauthorized);
    }
    if !identity.authorized_boards.contains(board_id) {
        return Err(SyncError::BoardUnauthorized);
    }
    let (device, heads) = {
        let r = repo.lock().await;
        (r.board_id().to_string(), r.heads())
    };
    if device != board_id {
        return Err(SyncError::BoardUnauthorized);
    }
    transport
        .send(WireMessage::new(Message::Hello(Hello {
            device_id: identity.device_id.clone(),
            endpoint_id: identity.endpoint_id.clone(),
        })))
        .await?;
    match transport.recv().await?.message {
        Message::Hello(h)
            if h.endpoint_id == remote_endpoint_id
                && identity.trusted_peers.contains(&h.endpoint_id) => {}
        Message::Hello(_) => return Err(SyncError::Unauthorized),
        _ => {
            return Err(SyncError::Transport(TransportError::Protocol(
                ProtocolError::Invalid("expected hello".into()),
            )));
        }
    }
    transport
        .send(WireMessage::new(Message::BoardSummary(BoardSummary {
            board_id: board_id.into(),
            heads,
        })))
        .await?;
    match transport.recv().await?.message {
        Message::BoardSummary(s) if s.board_id == board_id => {}
        Message::BoardSummary(_) => return Err(SyncError::BoardUnauthorized),
        _ => {
            return Err(SyncError::Transport(TransportError::Protocol(
                ProtocolError::Invalid("expected board summary".into()),
            )));
        }
    }
    // Stable role ordering avoids deadlock on bounded transports.
    if identity.endpoint_id.as_str() < remote_endpoint_id {
        send_revisions(transport, &repo).await?;
        receive_revisions(transport, &repo).await?;
    } else {
        receive_revisions(transport, &repo).await?;
        send_revisions(transport, &repo).await?;
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn card(title: &str, due: Option<&str>, body: &str) -> String {
        kanban_core::Card {
            frontmatter: kanban_core::CardFrontmatter {
                id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
                title: title.into(),
                column: "todo".into(),
                position: 1000,
                due: due.map(str::to_string),
                labels: vec![],
                created_at: None,
                sync: None,
                activity: vec![],
                extra: BTreeMap::new(),
            },
            body: body.into(),
        }
        .serialize()
        .unwrap()
    }
    fn rev(id: &str, parents: Vec<&str>, text: String) -> Revision {
        Revision {
            id: id.into(),
            card_id: "card".into(),
            parents: parents.into_iter().map(str::to_string).collect(),
            content: text,
            tombstone: false,
        }
    }
    fn trusted_pair() -> (DeviceIdentity, DeviceIdentity) {
        let mut a = DeviceIdentity::new("a");
        let mut b = DeviceIdentity::new("b");
        a.trust(b.endpoint_id.clone());
        b.trust(a.endpoint_id.clone());
        a.authorize_board("board");
        b.authorize_board("board");
        (a, b)
    }

    #[test]
    fn wire_roundtrip_and_validation() {
        let w = WireMessage::new(Message::Ack(Ack {
            revision_id: "r".into(),
        }));
        assert_eq!(WireMessage::decode(&w.encode().unwrap()).unwrap(), w);
        let bad = WireMessage {
            version: 99,
            message: Message::Ack(Ack {
                revision_id: "x".into(),
            }),
        };
        assert!(matches!(
            WireMessage::decode(&serde_json::to_vec(&bad).unwrap()),
            Err(ProtocolError::UnsupportedVersion(99))
        ));
        assert!(WireMessage::decode(b"not json").is_err());
        assert!(matches!(
            WireMessage::decode(&vec![0; MAX_MESSAGE + 1]),
            Err(ProtocolError::TooLarge)
        ));
    }

    #[tokio::test]
    async fn two_real_engines_complete_handshake_and_reconnect() {
        let (a_id, b_id) = trusted_pair();
        let mut initial = MemoryRevisionRepository::new("board");
        initial.add(rev("a", vec![], card("created", None, "body")));
        let ar = Arc::new(Mutex::new(initial));
        let br = Arc::new(Mutex::new(MemoryRevisionRepository::new("board")));
        for _ in 0..2 {
            let (mut at, mut bt) = InMemoryTransport::pair(4);
            let (x, y) = tokio::join!(
                sync_once(&mut at, ar.clone(), &a_id, &b_id.endpoint_id, "board"),
                sync_once(&mut bt, br.clone(), &b_id, &a_id.endpoint_id, "board")
            );
            assert!(x.is_ok() && y.is_ok());
            assert!(br.lock().await.get("a").is_some());
        }
        assert_eq!(ar.lock().await.revisions.len(), 1);
        assert_eq!(br.lock().await.revisions.len(), 1);
    }
    #[tokio::test]
    async fn transport_rejects_malformed_and_oversize() {
        let (a, mut b) = InMemoryTransport::pair(2);
        a.chan.tx.send(b"bad".to_vec()).await.unwrap();
        assert!(matches!(
            b.recv().await,
            Err(TransportError::Protocol(ProtocolError::Codec(_)))
        ));
        let (a, mut b) = InMemoryTransport::pair(2);
        a.chan.tx.send(vec![0; MAX_MESSAGE + 1]).await.unwrap();
        assert!(matches!(
            b.recv().await,
            Err(TransportError::Protocol(ProtocolError::TooLarge))
        ));
    }
    #[tokio::test]
    async fn trust_and_board_authorization_are_enforced() {
        let (mut at, _) = InMemoryTransport::pair(1);
        let repo = Arc::new(Mutex::new(MemoryRevisionRepository::new("board")));
        let d = DeviceIdentity::new("a");
        assert!(matches!(
            sync_once(&mut at, repo.clone(), &d, "peer", "board").await,
            Err(SyncError::Unauthorized)
        ));
        let mut d = DeviceIdentity::new("a");
        d.trust("peer");
        assert!(matches!(
            sync_once(&mut at, repo, &d, "peer", "board").await,
            Err(SyncError::BoardUnauthorized)
        ));
    }

    #[test]
    fn fast_forward_create_edit_delete_and_repeat() {
        let mut a = MemoryRevisionRepository::new("board");
        let mut b = MemoryRevisionRepository::new("board");
        let one = rev("a", vec![], card("one", None, "body"));
        a.add(one);
        assert_eq!(sync_memory(&mut a, &mut b).transferred, 1);
        let two = rev("b", vec!["a"], card("two", None, "body"));
        b.add(two);
        assert_eq!(sync_memory(&mut a, &mut b).transferred, 1);
        let del = Revision {
            id: "c".into(),
            card_id: "card".into(),
            parents: vec!["b".into()],
            content: String::new(),
            tombstone: true,
        };
        a.add(del);
        assert_eq!(sync_memory(&mut a, &mut b).transferred, 1);
        let sizes = (a.revisions.len(), b.revisions.len());
        let out = sync_memory(&mut a, &mut b);
        assert_eq!(out.transferred, 0);
        assert_eq!(sizes, (a.revisions.len(), b.revisions.len()));
        assert!(b.get("c").unwrap().tombstone);
    }
    #[test]
    fn offline_nonconflicting_edits_merge_and_converge() {
        let base = rev(
            "a",
            vec![],
            card(
                "base",
                None,
                "one
two
three",
            ),
        );
        let left = rev(
            "b",
            vec!["a"],
            card(
                "local",
                None,
                "ONE
two
three",
            ),
        );
        let right = rev(
            "c",
            vec!["a"],
            card(
                "base",
                Some("tomorrow"),
                "one
two
THREE",
            ),
        );
        let mut a = MemoryRevisionRepository::new("board");
        let mut b = MemoryRevisionRepository::new("board");
        a.add(base.clone());
        b.add(base);
        a.add(left);
        b.add(right);
        let out = sync_memory(&mut a, &mut b);
        assert_eq!(out.merged, 1);
        assert!(out.conflicts.is_empty());
        assert_eq!(a.heads(), b.heads());
        assert_eq!(a.heads().len(), 1);
        let again = sync_memory(&mut a, &mut b);
        assert_eq!(again.transferred, 0);
        assert_eq!(again.merged, 0);
    }
    #[test]
    fn unresolved_conflict_then_resolution_converges() {
        let base = rev("a", vec![], card("base", None, "body"));
        let left = rev("b", vec!["a"], card("left", None, "body"));
        let right = rev("c", vec!["a"], card("right", None, "body"));
        let mut a = MemoryRevisionRepository::new("board");
        let mut b = MemoryRevisionRepository::new("board");
        a.add(base.clone());
        b.add(base);
        a.add(left);
        b.add(right);
        let out = sync_memory(&mut a, &mut b);
        assert_eq!(out.conflicts.len(), 1);
        assert_eq!(a.heads().len(), 2);
        let resolution = resolve_conflict(&out.conflicts[0], card("manual", None, "body"), false);
        assert_eq!(resolution.parents.len(), 2);
        a.add(resolution.clone());
        let out = sync_memory(&mut a, &mut b);
        assert_eq!(out.transferred, 1);
        assert_eq!(a.heads(), b.heads());
        assert_eq!(b.heads(), vec![resolution.id]);
    }
    #[test]
    fn revision_crate_adapter_preserves_tombstone() {
        let d = kanban_revisions::tombstone("r", "card", vec![], 7);
        let w = Revision::from_domain(&d);
        assert!(w.tombstone);
        assert_eq!(w.to_domain(7), d);
    }
    #[test]
    fn identity_is_path_independent() {
        let mut d = DeviceIdentity::new("x");
        d.trust("peer");
        d.authorize_board("board");
        assert!(!d.authorized_boards.contains("/tmp/board"));
    }
}
