//! Immutable, content-addressed revision DAGs.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Revision {
    pub revision_id: String,
    pub card_id: String,
    pub parents: Vec<String>,
    pub content_hash: String,
    pub timestamp: u64,
    pub snapshot: Option<String>,
    pub tombstone: bool,
}
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RevisionError {
    #[error("revision already exists with different contents: {0}")]
    Conflict(String),
    #[error("missing parent: {0}")]
    MissingParent(String),
    #[error("revision not found: {0}")]
    NotFound(String),
}

pub fn content_hash(snapshot: Option<&str>, tombstone: bool) -> String {
    let mut h = Sha256::new();
    h.update(if tombstone {
        b"tombstone".as_slice()
    } else {
        b"snapshot"
    });
    if let Some(s) = snapshot {
        h.update(s.as_bytes())
    };
    format!("sha256:{}", hex::encode(h.finalize()))
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RevisionDag {
    revisions: HashMap<String, Revision>,
}
impl RevisionDag {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&mut self, r: Revision) -> Result<(), RevisionError> {
        if let Some(old) = self.revisions.get(&r.revision_id) {
            return if old == &r {
                Ok(())
            } else {
                Err(RevisionError::Conflict(r.revision_id))
            };
        }
        if r.revision_id.is_empty() || r.parents.iter().any(|p| p == &r.revision_id) {
            return Err(RevisionError::Conflict(r.revision_id));
        }
        for p in &r.parents {
            if !self.revisions.contains_key(p) {
                return Err(RevisionError::MissingParent(p.clone()));
            }
        }
        let expected = content_hash(r.snapshot.as_deref(), r.tombstone);
        if r.content_hash != expected {
            return Err(RevisionError::Conflict(r.revision_id));
        }
        self.revisions.insert(r.revision_id.clone(), r);
        Ok(())
    }
    pub fn get(&self, id: &str) -> Option<&Revision> {
        self.revisions.get(id)
    }
    pub fn parents(&self, id: &str) -> Result<Vec<String>, RevisionError> {
        Ok(self
            .get(id)
            .ok_or_else(|| RevisionError::NotFound(id.into()))?
            .parents
            .clone())
    }
    pub fn ancestors(&self, id: &str) -> Result<HashSet<String>, RevisionError> {
        self.get(id)
            .ok_or_else(|| RevisionError::NotFound(id.into()))?;
        let mut out = HashSet::new();
        let mut q = VecDeque::from([id.to_string()]);
        while let Some(x) = q.pop_front() {
            for p in self.revisions[&x].parents.iter() {
                if out.insert(p.clone()) {
                    q.push_back(p.clone())
                }
            }
        }
        Ok(out)
    }
    pub fn descendants(&self, id: &str) -> Result<HashSet<String>, RevisionError> {
        self.get(id)
            .ok_or_else(|| RevisionError::NotFound(id.into()))?;
        let mut out = HashSet::new();
        let mut q = VecDeque::from([id.to_string()]);
        while let Some(x) = q.pop_front() {
            for (rid, r) in &self.revisions {
                if r.parents.iter().any(|p| p == &x) && out.insert(rid.clone()) {
                    q.push_back(rid.clone())
                }
            }
        }
        Ok(out)
    }
    pub fn is_ancestor(&self, a: &str, d: &str) -> Result<bool, RevisionError> {
        Ok(a == d || self.ancestors(d)?.contains(a))
    }
    pub fn common_ancestor(&self, a: &str, b: &str) -> Result<Option<String>, RevisionError> {
        self.get(a)
            .ok_or_else(|| RevisionError::NotFound(a.into()))?;
        self.get(b)
            .ok_or_else(|| RevisionError::NotFound(b.into()))?;
        fn distances(d: &RevisionDag, start: &str) -> HashMap<String, usize> {
            let mut out = HashMap::from([(start.to_string(), 0)]);
            let mut q = VecDeque::from([start.to_string()]);
            while let Some(x) = q.pop_front() {
                let n = out[&x] + 1;
                for p in &d.revisions[&x].parents {
                    if !out.contains_key(p) {
                        out.insert(p.clone(), n);
                        q.push_back(p.clone());
                    }
                }
            }
            out
        }
        let da = distances(self, a);
        let db = distances(self, b);
        let mut common: Vec<_> = da
            .keys()
            .filter(|id| db.contains_key(*id))
            .cloned()
            .collect();
        common.sort_by(|x, y| (da[x].max(db[x]), x).cmp(&(da[y].max(db[y]), y)));
        Ok(common.into_iter().next())
    }
    pub fn revisions(&self) -> impl Iterator<Item = &Revision> {
        self.revisions.values()
    }
}

pub fn snapshot(
    revision_id: impl Into<String>,
    card_id: impl Into<String>,
    parents: Vec<String>,
    timestamp: u64,
    body: impl Into<String>,
) -> Revision {
    let body = body.into();
    Revision {
        revision_id: revision_id.into(),
        card_id: card_id.into(),
        content_hash: content_hash(Some(&body), false),
        parents,
        timestamp,
        snapshot: Some(body),
        tombstone: false,
    }
}
pub fn tombstone(
    revision_id: impl Into<String>,
    card_id: impl Into<String>,
    parents: Vec<String>,
    timestamp: u64,
) -> Revision {
    Revision {
        revision_id: revision_id.into(),
        card_id: card_id.into(),
        content_hash: content_hash(None, true),
        parents,
        timestamp,
        snapshot: None,
        tombstone: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dag() -> RevisionDag {
        let mut d = RevisionDag::new();
        d.insert(snapshot("a", "c", vec![], 1, "one")).unwrap();
        d.insert(snapshot("b", "c", vec!["a".into()], 2, "two"))
            .unwrap();
        d.insert(snapshot("c", "c", vec!["a".into()], 3, "three"))
            .unwrap();
        d
    }
    #[test]
    fn ancestry() {
        let d = dag();
        assert!(d.is_ancestor("a", "b").unwrap());
        assert!(d.descendants("a").unwrap().contains("b"));
        assert_eq!(d.common_ancestor("b", "c").unwrap(), Some("a".into()));
    }
    #[test]
    fn merge_and_tombstone() {
        let mut d = dag();
        let m = snapshot("m", "c", vec!["b".into(), "c".into()], 4, "merge");
        d.insert(m).unwrap();
        assert_eq!(d.parents("m").unwrap().len(), 2);
        let t = tombstone("t", "c", vec!["m".into()], 5);
        d.insert(t.clone()).unwrap();
        assert!(t.tombstone);
        assert!(t.snapshot.is_none());
    }
    #[test]
    fn immutable_conflict() {
        let mut d = RevisionDag::new();
        let a = snapshot("a", "c", vec![], 1, "x");
        d.insert(a.clone()).unwrap();
        assert!(d.insert(snapshot("a", "c", vec![], 1, "y")).is_err());
        assert!(d.insert(a).is_ok());
    }
}
