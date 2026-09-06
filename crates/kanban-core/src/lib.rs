//! Domain model and deterministic Markdown/YAML representation for Kanban boards.
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("missing YAML frontmatter")]
    MissingFrontmatter,
    #[error("malformed YAML frontmatter: {0}")]
    MalformedYaml(String),
    #[error("invalid ULID for {field}: {value}")]
    InvalidUlid { field: String, value: String },
    #[error("frontmatter must be a YAML mapping")]
    NotAMapping,
}

pub type Result<T> = std::result::Result<T, ParseError>;

/// Validate a canonical 26-character Crockford ULID.
pub fn validate_ulid(value: &str) -> bool {
    value.len() == 26
        && value
            .bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'))
        && ulid::Ulid::from_string(value).is_ok()
}

fn check_ulid(field: &str, value: &str) -> Result<()> {
    if validate_ulid(value) {
        Ok(())
    } else {
        Err(ParseError::InvalidUlid {
            field: field.into(),
            value: value.into(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SyncMetadata {
    pub revision: Option<String>,
    #[serde(default)]
    pub parents: Vec<String>,
    pub content_hash: Option<String>,
    pub updated_at: Option<String>,
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub at: String,
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CardFrontmatter {
    pub id: String,
    pub title: String,
    pub column: String,
    pub position: i64,
    pub due: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    pub created_at: Option<String>,
    pub sync: Option<SyncMetadata>,
    #[serde(default)]
    pub activity: Vec<Activity>,
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub frontmatter: CardFrontmatter,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    /// Board frontmatter fields are intentionally open-ended for forward compatibility.
    pub metadata: BTreeMap<String, Value>,
    pub body: String,
}

fn split_owned(input: &str) -> Result<(String, String)> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let start = normalized
        .strip_prefix("---\n")
        .ok_or(ParseError::MissingFrontmatter)?;
    let end = start.find("\n---").ok_or(ParseError::MissingFrontmatter)?;
    let after = &start[end + 4..];
    let body = after
        .strip_prefix("\n\n")
        .or_else(|| after.strip_prefix('\n'))
        .unwrap_or(after);
    Ok((start[..end].to_owned(), body.to_owned()))
}

fn yaml_map(yaml: &str) -> Result<BTreeMap<String, Value>> {
    let value: Value =
        serde_yaml::from_str(yaml).map_err(|e| ParseError::MalformedYaml(e.to_string()))?;
    match value {
        Value::Mapping(m) => m
            .into_iter()
            .map(|(k, v)| match k {
                Value::String(s) => Ok((s, v)),
                _ => Err(ParseError::NotAMapping),
            })
            .collect(),
        _ => Err(ParseError::NotAMapping),
    }
}

impl CardFrontmatter {
    pub fn parse(yaml: &str) -> Result<Self> {
        let fm: Self =
            serde_yaml::from_str(yaml).map_err(|e| ParseError::MalformedYaml(e.to_string()))?;
        validate_frontmatter(&fm)?;
        Ok(fm)
    }
    /// Read arbitrary CSS color values keyed by label name.
    pub fn label_colors(&self) -> BTreeMap<String, String> {
        self.extra
            .get("label_colors")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    /// Store an arbitrary CSS color for a label in YAML extension metadata.
    pub fn set_label_color(&mut self, label: impl Into<String>, color: impl Into<String>) {
        let mut colors = self.label_colors();
        colors.insert(label.into(), color.into());
        if let Ok(value) = serde_yaml::to_value(colors) {
            self.extra.insert("label_colors".into(), value);
        }
    }

    pub fn to_yaml(&self) -> Result<String> {
        validate_frontmatter(self)?;
        serde_yaml::to_string(self).map_err(|e| ParseError::MalformedYaml(e.to_string()))
    }
}

fn validate_frontmatter(fm: &CardFrontmatter) -> Result<()> {
    check_ulid("id", &fm.id)?;
    if let Some(sync) = &fm.sync {
        if let Some(v) = &sync.revision {
            check_ulid("sync.revision", v)?;
        }
        for p in &sync.parents {
            check_ulid("sync.parents", p)?;
        }
    }
    for a in &fm.activity {
        check_ulid("activity.id", &a.id)?;
    }
    Ok(())
}

impl Card {
    pub fn parse(input: &str) -> Result<Self> {
        let (yaml, body) = split_owned(input)?;
        Ok(Self {
            frontmatter: CardFrontmatter::parse(&yaml)?,
            body,
        })
    }
    pub fn from_markdown(input: &str) -> Result<Self> {
        Self::parse(input)
    }
    pub fn to_markdown(&self) -> Result<String> {
        Ok(format!(
            "---\n{}---\n\n{}",
            self.frontmatter.to_yaml()?,
            self.body
        ))
    }
    pub fn serialize(&self) -> Result<String> {
        self.to_markdown()
    }
    /// Hash the complete semantic snapshot (canonical frontmatter plus body).
    /// Sync metadata is included because it is part of the persisted card state.
    pub fn canonical_content_hash(&self) -> String {
        let snapshot = self.to_markdown().unwrap_or_else(|_| self.body.clone());
        canonical_content_hash(&snapshot)
    }
}

impl Board {
    pub fn parse(input: &str) -> Result<Self> {
        let (yaml, body) = split_owned(input)?;
        Ok(Self {
            metadata: yaml_map(&yaml)?,
            body,
        })
    }
    pub fn from_markdown(input: &str) -> Result<Self> {
        Self::parse(input)
    }
    pub fn to_markdown(&self) -> Result<String> {
        let yaml = serde_yaml::to_string(&self.metadata)
            .map_err(|e| ParseError::MalformedYaml(e.to_string()))?;
        Ok(format!("---\n{}---\n\n{}", yaml, self.body))
    }
    pub fn serialize(&self) -> Result<String> {
        self.to_markdown()
    }
}

/// Extract Markdown links from a card body as `(label, destination)` pairs.
/// Links are deliberately stored in the Markdown body rather than duplicated in YAML.
pub fn markdown_links(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find("[") {
        let after_open = &rest[open + 1..];
        let Some(close_rel) = after_open.find("](") else { break };
        let label = &after_open[..close_rel];
        let after_dest = &after_open[close_rel + 2..];
        let Some(end) = after_dest.find(")") else { break };
        let destination = &after_dest[..end];
        if !label.is_empty() && !destination.is_empty() {
            out.push((label.to_owned(), destination.to_owned()));
        }
        rest = &after_dest[end + 1..];
    }
    out
}

/// SHA-256 of normalized Markdown content, prefixed for unambiguous storage.
pub fn canonical_content_hash(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut h = Sha256::new();
    h.update(normalized.as_bytes());
    format!("sha256:{}", hex_encode(&h.finalize()))
}
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    #[test]
    fn valid_ulid() {
        assert!(validate_ulid(ID));
        assert!(!validate_ulid("bad"));
    }
    #[test]
    fn card_roundtrip_and_unknowns() {
        let s = format!(
            "---\nid: {ID}\ntitle: Test\ncolumn: todo\nposition: 1\nfuture: yes\nlabels: [x]\n---\n\nHello\n"
        );
        let c = Card::parse(&s).unwrap();
        assert_eq!(c.frontmatter.extra["future"], Value::String("yes".into()));
        let c2 = Card::parse(&c.to_markdown().unwrap()).unwrap();
        assert_eq!(c, c2);
    }
    #[test]
    fn malformed_rejected() {
        assert!(matches!(
            Card::parse("---\nid: [\n---\nx"),
            Err(ParseError::MalformedYaml(_))
        ));
    }
    #[test]
    fn label_colors_roundtrip_as_arbitrary_css() {
        let mut fm = CardFrontmatter { id: ID.into(), title: "x".into(), column: "todo".into(), position: 1, ..Default::default() };
        fm.set_label_color("urgent", "color-mix(in srgb, red 40%, #123456)");
        let parsed = CardFrontmatter::parse(&fm.to_yaml().unwrap()).unwrap();
        assert_eq!(parsed.label_colors()["urgent"], "color-mix(in srgb, red 40%, #123456)");
    }
    #[test]
    fn markdown_links_are_extracted_from_body() {
        assert_eq!(markdown_links("See [issue](https://example.test/a) and [docs](/docs)."), vec![
            ("issue".into(), "https://example.test/a".into()),
            ("docs".into(), "/docs".into()),
        ]);
    }
    #[test]
    fn hash_is_stable() {
        assert_eq!(
            canonical_content_hash("a\r\nb"),
            canonical_content_hash("a\nb")
        );
    }
    #[test]
    fn card_hash_includes_semantic_frontmatter() {
        let mut a = CardFrontmatter {
            id: ID.into(),
            title: "a".into(),
            column: "todo".into(),
            position: 1,
            ..Default::default()
        };
        let first = Card {
            frontmatter: a.clone(),
            body: "same".into(),
        }
        .canonical_content_hash();
        a.title = "b".into();
        let second = Card {
            frontmatter: a,
            body: "same".into(),
        }
        .canonical_content_hash();
        assert_ne!(first, second);
    }
}
