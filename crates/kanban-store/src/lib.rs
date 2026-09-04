//! Local board persistence. Markdown remains the source of truth.
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("invalid card id: {0}")]
    InvalidId(String),
    #[error("duplicate card id {id} in {first} and {second}")]
    Duplicate {
        id: String,
        first: PathBuf,
        second: PathBuf,
    },
    #[error("card not found: {0}")]
    NotFound(String),
    #[error("path escapes board root")]
    Escape,
}
pub type Result<T> = std::result::Result<T, StoreError>;

/// A store deliberately deals in serialized markdown so malformed documents are never rewritten.
pub trait BoardStore {
    fn open(path: impl AsRef<Path>) -> Result<Self>
    where
        Self: Sized;
    fn list_cards(&self) -> Result<Vec<String>>;
    fn read_card(&self, id: &str) -> Result<String>;
    fn write_card(&mut self, id: &str, markdown: &str) -> Result<()>;
    fn delete_card(&mut self, id: &str) -> Result<()>;
    fn read_board_metadata(&self) -> Result<String>;
    fn write_board_metadata(&mut self, markdown: &str) -> Result<()>;
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn confined(root: &Path, id: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err(StoreError::InvalidId(id.into()));
    };
    let p = root.join("cards").join(format!("{id}.md"));
    if !p.starts_with(root) {
        return Err(StoreError::Escape);
    };
    Ok(p)
}
fn atomic(path: &Path, data: &str) -> io::Result<()> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!(
        ".{name}.tmp-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    fs::write(&tmp, data)?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}
fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct FsBoardStore {
    root: PathBuf,
}
impl FsBoardStore {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(path.as_ref().join("cards"))?;
        let root = path.as_ref().canonicalize()?;
        Ok(Self { root })
    }
}
impl BoardStore for FsBoardStore {
    fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::new(path)
    }
    fn list_cards(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashMap::new();
        for e in fs::read_dir(self.root.join("cards"))? {
            let p = e?.path();
            if p.extension().and_then(|x| x.to_str()) != Some("md") {
                continue;
            }
            let text = fs::read_to_string(&p)?;
            let id = parse_id(&text)
                .unwrap_or_else(|| p.file_stem().unwrap().to_string_lossy().into_owned());
            if let Some(first) = seen.insert(id.clone(), p.clone()) {
                return Err(StoreError::Duplicate {
                    id,
                    first,
                    second: p,
                });
            }
            out.push(id);
        }
        out.sort();
        Ok(out)
    }
    fn read_card(&self, id: &str) -> Result<String> {
        let p = confined(&self.root, id)?;
        fs::read_to_string(p).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                StoreError::NotFound(id.into())
            } else {
                e.into()
            }
        })
    }
    fn write_card(&mut self, id: &str, markdown: &str) -> Result<()> {
        let p = confined(&self.root, id)?;
        fs::create_dir_all(self.root.join("cards"))?;
        atomic(&p, markdown)?;
        Ok(())
    }
    fn delete_card(&mut self, id: &str) -> Result<()> {
        let p = confined(&self.root, id)?;
        match fs::remove_file(p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(StoreError::NotFound(id.into())),
            Err(e) => Err(e.into()),
        }
    }
    fn read_board_metadata(&self) -> Result<String> {
        fs::read_to_string(self.root.join("board.md")).map_err(Into::into)
    }
    fn write_board_metadata(&mut self, markdown: &str) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        atomic(&self.root.join("board.md"), markdown)?;
        Ok(())
    }
}
fn parse_id(s: &str) -> Option<String> {
    for l in s.lines().take(30) {
        let x = l.trim();
        if let Some(v) = x.strip_prefix("id:") {
            let v = v.trim().trim_matches('"');
            if valid_id(v) {
                return Some(v.into());
            }
        }
    }
    None
}

#[derive(Debug, Default, Clone)]
pub struct MemoryBoardStore {
    cards: std::collections::BTreeMap<String, String>,
    metadata: String,
}
impl MemoryBoardStore {
    pub fn new() -> Self {
        Self::default()
    }
}
impl BoardStore for MemoryBoardStore {
    fn open(_: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::default())
    }
    fn list_cards(&self) -> Result<Vec<String>> {
        Ok(self.cards.keys().cloned().collect())
    }
    fn read_card(&self, id: &str) -> Result<String> {
        self.cards
            .get(id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(id.into()))
    }
    fn write_card(&mut self, id: &str, s: &str) -> Result<()> {
        self.cards.insert(id.into(), s.into());
        Ok(())
    }
    fn delete_card(&mut self, id: &str) -> Result<()> {
        self.cards
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| StoreError::NotFound(id.into()))
    }
    fn read_board_metadata(&self) -> Result<String> {
        Ok(self.metadata.clone())
    }
    fn write_board_metadata(&mut self, s: &str) -> Result<()> {
        self.metadata = s.into();
        Ok(())
    }
}
impl MemoryBoardStore {
    pub fn put(&mut self, id: impl Into<String>, s: impl Into<String>) {
        self.cards.insert(id.into(), s.into());
    }
    pub fn remove(&mut self, id: &str) -> Result<()> {
        self.cards
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| StoreError::NotFound(id.into()))
    }
    pub fn set_metadata(&mut self, s: impl Into<String>) {
        self.metadata = s.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filesystem_crud_and_external_reread() {
        let root = std::env::temp_dir().join(format!("kanban-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut s = FsBoardStore::new(&root).unwrap();
        s.write_card("x", "---\nid: x\n---\nhello").unwrap();
        assert_eq!(s.list_cards().unwrap(), vec!["x"]);
        assert_eq!(s.read_card("x").unwrap(), "---\nid: x\n---\nhello");
        fs::write(root.join("cards/x.md"), "---\nid: x\n---\nexternal").unwrap();
        assert!(s.read_card("x").unwrap().contains("external"));
        s.delete_card("x").unwrap();
        assert!(s.read_card("x").is_err());
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn duplicate_ids_are_reported() {
        let root = std::env::temp_dir().join(format!("kanban-dup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut s = FsBoardStore::new(&root).unwrap();
        s.write_card("a", "id: same").unwrap();
        s.write_card("b", "id: same").unwrap();
        assert!(matches!(s.list_cards(), Err(StoreError::Duplicate { .. })));
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn memory_roundtrip() {
        let mut s = MemoryBoardStore::new();
        s.put("a", "x");
        assert_eq!(s.read_card("a").unwrap(), "x");
        assert_eq!(s.list_cards().unwrap(), vec!["a"]);
        assert!(s.remove("a").is_ok())
    }
    #[test]
    fn id_confinement() {
        assert!(confined(Path::new("/tmp"), "../x").is_err())
    }
}
