//! Structured failures from the portable state codec and runtime bridge.

use crate::error::RuntimeError;
use std::{error::Error, fmt};

pub type SnapshotResult<T> = Result<T, SnapshotError>;

/// The operation that rejected a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotStage {
    Describe,
    Export,
    Restore,
    Validate,
    Encode,
    Decode,
    Migrate,
}

/// One owned component of a diagnostic path; no runtime identity is retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathComponent {
    Field(String),
    Index(usize),
    Variant(String),
    Definition(usize),
}

/// A path rooted at the selected DTO, formatted only when requested.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotPath(pub Vec<PathComponent>);

impl SnapshotPath {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn push(&mut self, component: PathComponent) {
        self.0.push(component);
    }

    pub fn pop(&mut self) {
        self.0.pop();
    }
}

impl fmt::Display for SnapshotPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "$")?;
        for component in &self.0 {
            match component {
                PathComponent::Field(name) => write!(f, ".{name}")?,
                PathComponent::Index(index) => write!(f, "[{index}]")?,
                PathComponent::Variant(name) => write!(f, "::{name}")?,
                PathComponent::Definition(index) => write!(f, ".schema[{index}]")?,
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotErrorKind {
    Cancelled,
    LimitExceeded { limit: &'static str },
    Allocation,
    InvalidHeader,
    InvalidWire,
    InvalidSchema,
    SchemaMismatch,
    TypeMismatch,
    Duplicate,
    NonCanonical,
    UnknownMember,
    MissingMember,
    UnsupportedType,
    Cycle { first_path: SnapshotPath },
    Alias { first_path: SnapshotPath },
    Runtime,
    Migration,
}

/// A structured diagnostic, optionally preserving the original runtime failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotError {
    pub stage: SnapshotStage,
    pub kind: SnapshotErrorKind,
    pub path: SnapshotPath,
    pub message: String,
    pub runtime_error: Option<Box<RuntimeError>>,
}

impl SnapshotError {
    pub fn new(stage: SnapshotStage, kind: SnapshotErrorKind, message: impl Into<String>) -> Self {
        Self {
            stage,
            kind,
            path: SnapshotPath::root(),
            message: message.into(),
            runtime_error: None,
        }
    }

    pub fn at(mut self, path: &SnapshotPath) -> Self {
        self.path = path.clone();
        self
    }

    pub fn runtime(stage: SnapshotStage, error: RuntimeError) -> Self {
        Self {
            stage,
            kind: SnapshotErrorKind::Runtime,
            path: SnapshotPath::root(),
            message: error.to_string(),
            runtime_error: Some(Box::new(error)),
        }
    }

    pub fn limit(stage: SnapshotStage, limit: &'static str) -> Self {
        Self::new(stage, SnapshotErrorKind::LimitExceeded { limit }, limit)
    }
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} {:?} at {}: {}",
            self.stage, self.kind, self.path, self.message
        )
    }
}

impl Error for SnapshotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.runtime_error
            .as_deref()
            .map(|error| error as &dyn Error)
    }
}
