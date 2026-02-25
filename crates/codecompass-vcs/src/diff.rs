use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed { old_path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffEntry {
    pub path: String,
    pub kind: FileChangeKind,
}

impl DiffEntry {
    pub fn added(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind: FileChangeKind::Added,
        }
    }

    pub fn modified(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind: FileChangeKind::Modified,
        }
    }

    pub fn deleted(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind: FileChangeKind::Deleted,
        }
    }

    pub fn renamed(old_path: impl Into<String>, new_path: impl Into<String>) -> Self {
        let old_path = old_path.into();
        let new_path = new_path.into();
        Self {
            path: new_path.clone(),
            kind: FileChangeKind::Renamed { old_path },
        }
    }
}
