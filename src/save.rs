use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Save {
    pub last: String, // book hash of last opened book
    pub files: HashMap<String, (usize, usize)>, // book_hash → (chapter, byte_offset)
}

pub(crate) struct State {
    pub save: Save,
    pub save_path: String,
    pub path: String,
    pub meta: bool,
    pub bk: crate::bk::Props,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_default() {
        let save = Save::default();
        assert!(save.last.is_empty());
        assert!(save.files.is_empty());
    }

    #[test]
    fn test_save_serialize_roundtrip() {
        let mut save = Save::default();
        save.last = "abc123def".to_string();
        save.files.insert("abc123def".to_string(), (3, 42));
        save.files.insert("xyz789".to_string(), (0, 0));

        let serialized = ron::to_string(&save).unwrap();
        let deserialized: Save = ron::from_str(&serialized).unwrap();

        assert_eq!(deserialized.last, "abc123def");
        assert_eq!(deserialized.files.len(), 2);
        assert_eq!(deserialized.files.get("abc123def"), Some(&(3, 42)));
        assert_eq!(deserialized.files.get("xyz789"), Some(&(0, 0)));
    }

    #[test]
    fn test_save_deserialize_old_format() {
        // Old format used file paths as keys instead of content hashes
        let old_ron = r#"(
    last: "/path/to/book.epub",
    files: {
        "/path/to/book.epub": (2, 150),
        "/other/book.epub": (0, 0),
    },
)"#;

        let save: Save = ron::from_str(old_ron).unwrap();
        assert_eq!(save.last, "/path/to/book.epub");
        assert_eq!(save.files.len(), 2);
        assert_eq!(save.files.get("/path/to/book.epub"), Some(&(2, 150)));
    }

    #[test]
    fn test_save_empty_files() {
        let save = Save {
            last: "somehash".to_string(),
            files: HashMap::new(),
        };
        let serialized = ron::to_string(&save).unwrap();
        let deserialized: Save = ron::from_str(&serialized).unwrap();
        assert_eq!(deserialized.last, "somehash");
        assert!(deserialized.files.is_empty());
    }
}