//! Deterministic, ancestry-aware three-way merge for cards.
use knot_core::{Activity, Card};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictKind {
    Scalar,
    Body,
    Delete,
    Other,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeConflict {
    pub field: String,
    pub kind: ConflictKind,
    pub base: String,
    pub local: String,
    pub remote: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeResult {
    pub card: Card,
    pub conflicts: Vec<MergeConflict>,
}
impl MergeResult {
    pub fn is_conflicted(&self) -> bool {
        !self.conflicts.is_empty()
    }
}

/// Merge three card snapshots. The operation is deterministic and does not mutate inputs.
pub fn merge(base: &Card, local: &Card, remote: &Card) -> MergeResult {
    let mut out = base.clone();
    let mut conflicts = Vec::new();
    let b = &base.frontmatter;
    let l = &local.frontmatter;
    let r = &remote.frontmatter;
    out.frontmatter.title = scalar(
        "title",
        &b.title,
        &l.title,
        &r.title,
        &mut conflicts,
        choose_string,
    );
    out.frontmatter.due = scalar("due", &b.due, &l.due, &r.due, &mut conflicts, choose_option);
    out.frontmatter.column = merge_column(&b.column, &l.column, &r.column, l, r);
    out.frontmatter.position = merge_position(b.position, l.position, r.position, local, remote);
    out.frontmatter.labels = merge_labels(&b.labels, &l.labels, &r.labels);
    out.frontmatter.activity = merge_activity(&b.activity, &l.activity, &r.activity);
    // Metadata is revision-layer data; preserve the deterministic newest side.
    out.frontmatter.sync = choose_sync(base, local, remote);
    out.body = merge_body(&base.body, &local.body, &remote.body, &mut conflicts);
    MergeResult {
        card: out,
        conflicts,
    }
}

fn scalar<T: Clone + PartialEq + std::fmt::Debug>(
    name: &str,
    b: &T,
    l: &T,
    r: &T,
    c: &mut Vec<MergeConflict>,
    pick: fn(&T, &T) -> T,
) -> T {
    if l == b {
        r.clone()
    } else if r == b || l == r {
        l.clone()
    } else {
        c.push(MergeConflict {
            field: name.into(),
            kind: ConflictKind::Scalar,
            base: format!("{b:?}"),
            local: format!("{l:?}"),
            remote: format!("{r:?}"),
        });
        pick(l, r)
    }
}
fn choose_string(a: &String, b: &String) -> String {
    if a <= b { a.clone() } else { b.clone() }
}
fn choose_option(a: &Option<String>, b: &Option<String>) -> Option<String> {
    match (a, b) {
        (Some(x), Some(y)) => Some(choose_string(x, y)),
        _ => {
            if a.as_ref().map(ToString::to_string) <= b.as_ref().map(ToString::to_string) {
                a.clone()
            } else {
                b.clone()
            }
        }
    }
}
fn merge_column(
    b: &str,
    l: &str,
    r: &str,
    lc: &knot_core::CardFrontmatter,
    rc: &knot_core::CardFrontmatter,
) -> String {
    if l == b {
        return r.into();
    }
    if r == b || l == r {
        return l.into();
    }
    let la = newest_move(lc);
    let ra = newest_move(rc);
    if la > ra || (la == ra && l <= r) {
        l.into()
    } else {
        r.into()
    }
}
fn newest_move(f: &knot_core::CardFrontmatter) -> String {
    f.activity
        .iter()
        .filter(|a| {
            let t = a.event_type.to_ascii_lowercase();
            t == "move" || t == "moved" || t == "column"
        })
        .map(|a| a.id.clone())
        .max()
        .unwrap_or_default()
}
fn merge_position(b: i64, l: i64, r: i64, lc: &knot_core::Card, rc: &knot_core::Card) -> i64 {
    if l == b {
        r
    } else if r == b || l == r {
        l
    } else {
        let kl = stable_key(lc);
        let kr = stable_key(rc);
        if kl <= kr { l } else { r }
    }
}
fn stable_key(c: &Card) -> String {
    format!(
        "{}|{}|{}|{}",
        c.frontmatter
            .sync
            .as_ref()
            .and_then(|s| s.revision.clone())
            .unwrap_or_default(),
        c.frontmatter.id,
        c.frontmatter.title,
        c.body
    )
}
fn choose_sync(b: &Card, l: &Card, r: &Card) -> Option<knot_core::SyncMetadata> {
    if stable_key(l) >= stable_key(r) {
        l.frontmatter.sync.clone()
    } else if stable_key(r) >= stable_key(b) {
        r.frontmatter.sync.clone()
    } else {
        b.frontmatter.sync.clone()
    }
}
fn merge_labels(base: &[String], local: &[String], remote: &[String]) -> Vec<String> {
    let b: BTreeSet<_> = base.iter().collect();
    let l: BTreeSet<_> = local.iter().collect();
    let r: BTreeSet<_> = remote.iter().collect();
    let mut out: BTreeSet<String> = BTreeSet::new();
    // Existing labels require retention on both branches; additions are unioned.
    for x in l.union(&r) {
        if !b.contains(x) || (l.contains(x) && r.contains(x)) {
            out.insert((*x).clone());
        }
    }
    out.into_iter().collect()
}
fn merge_activity(a: &[Activity], b: &[Activity], c: &[Activity]) -> Vec<Activity> {
    let mut m: BTreeMap<String, Activity> = BTreeMap::new();
    for x in a.iter().chain(b).chain(c) {
        m.entry(x.id.clone()).or_insert_with(|| x.clone());
    }
    m.into_values().collect()
}

fn merge_body(base: &str, local: &str, remote: &str, c: &mut Vec<MergeConflict>) -> String {
    if local == base {
        return remote.into();
    }
    if remote == base || local == remote {
        return local.into();
    }
    let (ls, le, lx) = edit_span(base, local);
    let (rs, re, rx) = edit_span(base, remote);
    if le <= rs || re <= ls {
        let lines: Vec<&str> = base.lines().collect();
        let mut edits = vec![(ls, le, lx), (rs, re, rx)];
        edits.sort_by_key(|e| e.0);
        let mut out = String::new();
        let mut at = 0;
        for (s, e, v) in edits {
            for x in &lines[at..s] {
                out.push_str(x);
                out.push('\n')
            }
            for x in v {
                out.push_str(x);
                out.push('\n')
            }
            at = e;
        }
        for x in &lines[at..] {
            out.push_str(x);
            out.push('\n')
        }
        return trim_nl(out);
    }
    c.push(MergeConflict {
        field: "body".into(),
        kind: ConflictKind::Body,
        base: base.into(),
        local: local.into(),
        remote: remote.into(),
    });
    if local <= remote {
        local.into()
    } else {
        remote.into()
    }
}
fn edit_span<'a>(base: &str, changed: &'a str) -> (usize, usize, Vec<&'a str>) {
    let b: Vec<_> = base.lines().collect();
    let x: Vec<_> = changed.lines().collect();
    let mut s = 0;
    while s < b.len() && s < x.len() && b[s] == x[s] {
        s += 1
    }
    let mut be = b.len();
    let mut xe = x.len();
    while be > s && xe > s && b[be - 1] == x[xe - 1] {
        be -= 1;
        xe -= 1
    }
    (s, be, x[s..xe].to_vec())
}
fn trim_nl(mut s: String) -> String {
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    fn card(title: &str, body: &str) -> Card {
        Card {
            frontmatter: knot_core::CardFrontmatter {
                id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
                title: title.into(),
                column: "todo".into(),
                position: 1,
                due: None,
                labels: vec!["base".into()],
                created_at: None,
                sync: None,
                activity: vec![],
                extra: BTreeMap::new(),
            },
            body: body.into(),
        }
    }
    #[test]
    fn scalar_and_body() {
        let b = card("x", "a\nb\nc");
        let mut l = b.clone();
        l.frontmatter.title = "l".into();
        l.body = "A\nb\nc".into();
        let mut r = b.clone();
        r.frontmatter.due = Some("tomorrow".into());
        r.body = "a\nb\nC".into();
        let m = merge(&b, &l, &r);
        assert_eq!(m.card.frontmatter.title, "l");
        assert_eq!(m.card.frontmatter.due.as_deref(), Some("tomorrow"));
        assert!(m.conflicts.is_empty());
    }
    #[test]
    fn conflict_explicit() {
        let b = card("x", "a");
        let mut l = b.clone();
        l.frontmatter.title = "z".into();
        let mut r = b.clone();
        r.frontmatter.title = "y".into();
        assert!(merge(&b, &l, &r).is_conflicted());
    }
    #[test]
    fn labels_remove_and_add_converge() {
        let mut b = card("x", "a");
        b.frontmatter.labels = vec!["backend".into(), "urgent".into()];
        let mut l = b.clone();
        l.frontmatter.labels = vec!["backend".into()];
        let mut r = b.clone();
        r.frontmatter.labels = vec!["backend".into(), "urgent".into(), "bug".into()];
        let x = merge(&b, &l, &r);
        let y = merge(&b, &r, &l);
        assert_eq!(x.card.frontmatter.labels, vec!["backend", "bug"]);
        assert_eq!(x.card, y.card);
    }
    #[test]
    fn repeated_merge_is_idempotent() {
        let b = card("x", "a");
        let mut l = b.clone();
        l.frontmatter.title = "l".into();
        let x = merge(&b, &l, &b);
        let y = merge(&b, &x.card, &x.card);
        assert_eq!(x.card, y.card);
    }
    #[test]
    fn body_overlap_is_conflict() {
        let b = card("x", "a\nb\nc");
        let mut l = b.clone();
        l.body = "a\nL\nc".into();
        let mut r = b.clone();
        r.body = "a\nR\nc".into();
        let m = merge(&b, &l, &r);
        assert!(m.conflicts.iter().any(|x| x.field == "body"));
    }
    #[test]
    fn newest_move_wins_symmetrically() {
        let b = card("x", "a");
        let mut l = b.clone();
        l.frontmatter.column = "doing".into();
        l.frontmatter.activity.push(Activity {
            id: "01ARZ3NDEKTSV4RRFFQ69G5FAY".into(),
            event_type: "move".into(),
            at: "1".into(),
            extra: BTreeMap::new(),
        });
        let mut r = b.clone();
        r.frontmatter.column = "done".into();
        r.frontmatter.activity.push(Activity {
            id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            event_type: "move".into(),
            at: "1".into(),
            extra: BTreeMap::new(),
        });
        let x = merge(&b, &l, &r);
        let y = merge(&b, &r, &l);
        assert_eq!(x.card, y.card);
        assert_eq!(x.card.frontmatter.column, "doing");
    }
}
