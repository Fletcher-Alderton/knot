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
    #[error("malformed card YAML: {0}")]
    Malformed(String),
    #[error("target occupied by unrelated card")]
    Occupied,
}
pub type Result<T> = std::result::Result<T, StoreError>;

/// Canonical card filename: `{column}-{title}-{id}.md`.
pub fn card_filename(column: &str, title: &str, id: &str) -> String {
    kanban_core::card_filename(column, title, id)
}

/// Extract ID from canonical or legacy filename stem.
pub fn extract_card_id(stem: &str) -> &str {
    kanban_core::extract_id_from_filename(stem)
}

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

    /// Current on-disk path for card, including legacy compatibility.
    pub fn card_path(&self, id: &str) -> Result<PathBuf> {
        self.find_card_path(id)?
            .ok_or_else(|| StoreError::NotFound(id.into()))
    }

    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(path.as_ref().join("cards"))?;
        let root = path.as_ref().canonicalize()?;
        Ok(Self { root })
    }

    fn scan_cards(&self) -> Result<Vec<(String, PathBuf)>> {
        let dir = self.root.join("cards");
        let meta = fs::symlink_metadata(&dir)?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(StoreError::Escape);
        }
        let mut out: Vec<(String, PathBuf)> = Vec::new();
        for e in fs::read_dir(&dir)? {
            let p = e?.path();
            let m = fs::symlink_metadata(&p)?;
            if m.file_type().is_symlink() {
                return Err(StoreError::Escape);
            }
            if p.extension().and_then(|x| x.to_str()) != Some("md") {
                continue;
            }
            let text = fs::read_to_string(&p)?;
            let id = if text.trim_start().starts_with("---") {
                parse_id(&text).ok_or_else(|| StoreError::Malformed(p.display().to_string()))?
            } else {
                extract_card_id(&p.file_stem().unwrap().to_string_lossy()).to_string()
            };
            if !valid_id(&id) {
                return Err(StoreError::InvalidId(id));
            }
            if let Some((_, first)) = out.iter().find(|(x, _)| x == &id) {
                return Err(StoreError::Duplicate {
                    id,
                    first: first.clone(),
                    second: p,
                });
            }
            out.push((id, p));
        }
        Ok(out)
    }

    /// Finds the existing file path for a card with ID `id`.
    /// Matches `{id}.md`, or files ending in `-{id}.md`, or by reading the frontmatter `id:` field.
    pub fn find_card_path(&self, id: &str) -> Result<Option<PathBuf>> {
        if !valid_id(id) {
            return Err(StoreError::InvalidId(id.into()));
        }
        let cards_dir = self.root.join("cards");
        if !cards_dir.is_dir() {
            return Ok(None);
        }

        let cards = self.scan_cards()?;
        Ok(cards
            .into_iter()
            .find(|(card_id, _)| card_id == id)
            .map(|(_, p)| p))
    }
}

impl BoardStore for FsBoardStore {
    fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::new(path)
    }

    fn list_cards(&self) -> Result<Vec<String>> {
        let mut out: Vec<String> = self.scan_cards()?.into_iter().map(|(id, _)| id).collect();
        out.sort();
        Ok(out)
    }

    fn read_card(&self, id: &str) -> Result<String> {
        if let Some(p) = self.find_card_path(id)? {
            fs::read_to_string(p).map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    StoreError::NotFound(id.into())
                } else {
                    e.into()
                }
            })
        } else {
            Err(StoreError::NotFound(id.into()))
        }
    }

    fn write_card(&mut self, id: &str, markdown: &str) -> Result<()> {
        if !valid_id(id) {
            return Err(StoreError::InvalidId(id.into()));
        }
        fs::create_dir_all(self.root.join("cards"))?;

        // Extract column and title if parsed
        let (col, title) = parse_column_and_title(markdown);
        let new_filename = if let (Some(c), Some(t)) = (col, title) {
            card_filename(&c, &t, id)
        } else {
            format!("{id}.md")
        };

        let new_path = self.root.join("cards").join(&new_filename);
        if !new_path.starts_with(&self.root) {
            return Err(StoreError::Escape);
        }

        // If card previously existed under another filename, rename/cleanup
        let existing = self.find_card_path(id)?;
        if let Ok(m) = fs::symlink_metadata(&new_path) {
            if m.file_type().is_symlink() {
                return Err(StoreError::Escape);
            }
            if existing.as_ref() != Some(&new_path) {
                let text = fs::read_to_string(&new_path)?;
                if parse_id(&text).as_deref() != Some(id) && text.trim_start().starts_with("---") {
                    return Err(StoreError::Occupied);
                }
            }
        }
        atomic(&new_path, markdown)?;
        if let Some(old_path) = existing
            && old_path != new_path
            && let Err(e) = fs::remove_file(&old_path)
        {
            let _ = fs::remove_file(&new_path);
            return Err(e.into());
        }
        Ok(())
    }

    fn delete_card(&mut self, id: &str) -> Result<()> {
        if let Some(p) = self.find_card_path(id)? {
            match fs::remove_file(p) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    Err(StoreError::NotFound(id.into()))
                }
                Err(e) => Err(e.into()),
            }
        } else {
            Err(StoreError::NotFound(id.into()))
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

fn parse_column_and_title(s: &str) -> (Option<String>, Option<String>) {
    let mut col = None;
    let mut title = None;
    for l in s.lines().take(40) {
        let x = l.trim();
        if col.is_none() && x.starts_with("column:") {
            col = Some(
                x.trim_start_matches("column:")
                    .trim()
                    .trim_matches('\'')
                    .trim_matches('"')
                    .to_string(),
            );
        }
        if title.is_none() && x.starts_with("title:") {
            title = Some(
                x.trim_start_matches("title:")
                    .trim()
                    .trim_matches('\'')
                    .trim_matches('"')
                    .to_string(),
            );
        }
        if col.is_some() && title.is_some() {
            break;
        }
    }
    (col, title)
}

fn parse_id(s: &str) -> Option<String> {
    for l in s.lines().take(30) {
        let x = l.trim();
        if let Some(v) = x.strip_prefix("id:") {
            let v = v.trim().trim_matches('\'').trim_matches('"');
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
        s.write_card("x", "---\nid: x\ncolumn: backlog\ntitle: Hello\n---\nhello")
            .unwrap();
        assert_eq!(s.list_cards().unwrap(), vec!["x"]);
        assert_eq!(
            s.read_card("x").unwrap(),
            "---\nid: x\ncolumn: backlog\ntitle: Hello\n---\nhello"
        );
        assert!(root.join("cards/backlog-Hello-x.md").is_file());

        // External overwrite of file
        fs::write(
            root.join("cards/backlog-Hello-x.md"),
            "---\nid: x\ncolumn: backlog\ntitle: Hello\n---\nexternal",
        )
        .unwrap();
        assert!(s.read_card("x").unwrap().contains("external"));

        // Renaming on write_card with changed column
        s.write_card("x", "---\nid: x\ncolumn: done\ntitle: Hello\n---\nfinished")
            .unwrap();
        assert!(!root.join("cards/backlog-Hello-x.md").exists());
        assert!(root.join("cards/done-Hello-x.md").is_file());
        assert_eq!(
            s.read_card("x").unwrap(),
            "---\nid: x\ncolumn: done\ntitle: Hello\n---\nfinished"
        );

        s.delete_card("x").unwrap();
        assert!(s.read_card("x").is_err());
        assert!(!root.join("cards/done-Hello-x.md").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_id_file_compatibility() {
        let root = std::env::temp_dir().join(format!("kanban-legacy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut s = FsBoardStore::new(&root).unwrap();
        fs::write(
            root.join("cards/01ARZ3NDEKTSV4RRFFQ69G0001.md"),
            "---\nid: 01ARZ3NDEKTSV4RRFFQ69G0001\ncolumn: todo\ntitle: Legacy\n---\nlegacy",
        )
        .unwrap();
        assert_eq!(
            s.read_card("01ARZ3NDEKTSV4RRFFQ69G0001").unwrap(),
            "---\nid: 01ARZ3NDEKTSV4RRFFQ69G0001\ncolumn: todo\ntitle: Legacy\n---\nlegacy"
        );
        assert_eq!(s.list_cards().unwrap(), vec!["01ARZ3NDEKTSV4RRFFQ69G0001"]);

        // Writing updates it to the new naming format and cleans up legacy
        s.write_card(
            "01ARZ3NDEKTSV4RRFFQ69G0001",
            "---\nid: 01ARZ3NDEKTSV4RRFFQ69G0001\ncolumn: todo\ntitle: Legacy\n---\nupdated",
        )
        .unwrap();
        assert!(!root.join("cards/01ARZ3NDEKTSV4RRFFQ69G0001.md").exists());
        assert!(
            root.join("cards/todo-Legacy-01ARZ3NDEKTSV4RRFFQ69G0001.md")
                .is_file()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_ids_are_reported() {
        let root = std::env::temp_dir().join(format!("kanban-dup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut s = FsBoardStore::new(&root).unwrap();
        s.write_card("a", "---\nid: same\ncolumn: col\ntitle: A\n---\n")
            .unwrap();
        s.write_card("b", "---\nid: same\ncolumn: col\ntitle: B\n---\n")
            .unwrap();
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
        let root = std::env::temp_dir().join(format!("kanban-confine-{}", std::process::id()));
        let mut s = FsBoardStore::new(&root).unwrap();
        assert!(s.read_card("../x").is_err());
        assert!(s.write_card("../x", "bad").is_err());
    }
}
