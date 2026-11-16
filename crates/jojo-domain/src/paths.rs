use std::path::{Component, Path, PathBuf};

/// Intended private storage roots. This is lexical configuration, not proof of filesystem safety.
/// Directory creation, 0700 permissions, ownership and symlink inspection belong to later tasks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalRoots {
    data: PathBuf,
    worktrees: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootError {
    NotAbsolute,
    Traversal,
    FilesystemRoot,
    OverlappingRoots,
}

impl LocalRoots {
    pub fn new(data: PathBuf, worktrees: PathBuf) -> Result<Self, RootError> {
        validate_root(&data)?;
        validate_root(&worktrees)?;
        if data.starts_with(&worktrees) || worktrees.starts_with(&data) {
            return Err(RootError::OverlappingRoots);
        }
        Ok(Self { data, worktrees })
    }

    /// HOME is supplied, never discovered inside the domain.
    pub fn for_macos_home(home: &Path) -> Result<Self, RootError> {
        validate_root(home)?;
        let support = home.join("Library/Application Support");
        Self::new(support.join("Jojo"), support.join("JojoWorktrees"))
    }

    pub fn data(&self) -> &Path {
        &self.data
    }

    pub fn worktrees(&self) -> &Path {
        &self.worktrees
    }
}

fn validate_root(path: &Path) -> Result<(), RootError> {
    if !path.is_absolute() {
        return Err(RootError::NotAbsolute);
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(RootError::Traversal);
    }
    if path.parent().is_none() {
        return Err(RootError::FilesystemRoot);
    }
    Ok(())
}
