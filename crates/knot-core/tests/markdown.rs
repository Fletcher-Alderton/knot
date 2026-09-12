use knot_core::{Board, Card, ParseError, canonical_content_hash, validate_ulid};
use serde_yaml::Value;

const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const REV: &str = "01BX5ZZKBKACTAV9WEVGEMMVRZ";

#[test]
fn parses_board_and_serializes_keys_in_order() {
    let board = Board::parse("---\nz: 2\na: 1\n---\n\n# Board\n").unwrap();
    assert_eq!(board.metadata["a"], Value::Number(1.into()));
    let out = board.to_markdown().unwrap();
    assert!(out.find("a:").unwrap() < out.find("z:").unwrap());
    assert_eq!(Board::parse(&out).unwrap(), board);
}

#[test]
fn parses_nested_sync_activity_and_preserves_unknowns() {
    let input = format!(
        "---\nid: {ID}\ntitle: Ship\ncolumn: doing\nposition: 2000\ndue: 2026-09-10\ncreated_at: 2026-09-04T03:20:00Z\nsync:\n  revision: {REV}\n  parents: [{ID}]\n  content_hash: sha256:abc\nactivity:\n  - id: {REV}\n    type: created\n    at: 2026-09-04T03:20:00Z\ncustom:\n  nested: true\n---\n\nBody"
    );
    let card = Card::parse(&input).unwrap();
    assert_eq!(card.frontmatter.sync.as_ref().unwrap().parents, vec![ID]);
    assert_eq!(card.frontmatter.activity[0].event_type, "created");
    assert!(card.frontmatter.extra.contains_key("custom"));
    assert_eq!(Card::parse(&card.to_markdown().unwrap()).unwrap(), card);
}

#[test]
fn invalid_ids_and_non_mapping_are_rejected() {
    assert!(matches!(
        Card::parse("---\nid: nope\ntitle: x\ncolumn: todo\nposition: 0\n---\nx"),
        Err(ParseError::InvalidUlid { .. })
    ));
    assert!(matches!(
        Board::parse("---\n- list\n---\nx"),
        Err(ParseError::NotAMapping)
    ));
    assert!(!validate_ulid("01ARZ3NDEKTSV4RRFFQ69G5FAI"));
}

#[test]
fn content_hash_has_sha256_prefix_and_is_line_ending_invariant() {
    assert_eq!(
        canonical_content_hash("hello\r\nworld"),
        canonical_content_hash("hello\nworld")
    );
    assert!(canonical_content_hash("hello").starts_with("sha256:"));
}
