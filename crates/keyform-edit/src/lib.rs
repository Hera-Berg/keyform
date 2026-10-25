//! Keyform editing core, independent of the UI: undo/redo history and
//! model edit operations.

pub mod history;

pub use history::{History, StepKind};

/// Object and parameter ID rules: letters, digits and underscores only, at
/// most 63 characters, not starting with a digit. spec: 50-ui/dialogs.md §2
/// (`@`, reserved for template objects, is not accepted here.)
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 63
        && !id.starts_with(|c: char| c.is_ascii_digit())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    #[test]
    fn id_rules() {
        use super::valid_id;
        assert!(valid_id("ArtMesh12"));
        assert!(valid_id("_x"));
        assert!(!valid_id(""));
        assert!(!valid_id("1abc"));
        assert!(!valid_id("a b"));
        assert!(!valid_id("a-b"));
        assert!(!valid_id("Ä"));
        assert!(valid_id(&"a".repeat(63)));
        assert!(!valid_id(&"a".repeat(64)));
    }
}
