//! Detached source changes. Building a batch never selects, compiles or publishes code.

use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_runtime::source::SourceDeclaration;

/// One existing public function path or opaque installation-scoped authored locator.
/// Paths resolve once when submitted; declarations can identify private functions,
/// methods and named locals without granting permission to call them.
#[derive(Debug, Clone)]
pub enum FunctionUpdateTarget {
    Path(String),
    Declaration(SourceDeclaration),
}

impl From<String> for FunctionUpdateTarget {
    fn from(path: String) -> Self {
        Self::Path(path)
    }
}

impl From<&str> for FunctionUpdateTarget {
    fn from(path: &str) -> Self {
        Self::Path(path.to_owned())
    }
}

impl From<SourceDeclaration> for FunctionUpdateTarget {
    fn from(declaration: SourceDeclaration) -> Self {
        Self::Declaration(declaration)
    }
}

impl From<&SourceDeclaration> for FunctionUpdateTarget {
    fn from(declaration: &SourceDeclaration) -> Self {
        Self::Declaration(declaration.clone())
    }
}

/// A caller-chosen atomic batch of complete function/file replacements and file additions.
/// Names identify captured documents, not filesystem reads. Submission rejects empty,
/// duplicate or overlapping changes, more than 1,024 entries or more than 64 MiB of
/// replacement UTF-8. Validation belongs to `Runtime::update`, not these data builders.
#[derive(Debug, Clone, Default)]
pub struct Update {
    pub(crate) edits: Vec<Edit>,
    pub(crate) cancel: CancellationToken,
}

#[derive(Debug, Clone)]
pub(crate) enum Edit {
    Function {
        target: FunctionUpdateTarget,
        definition: String,
    },
    File {
        name: String,
        text: String,
    },
    Add {
        module: ModuleIdentity,
        name: String,
        text: String,
    },
}

impl Edit {
    pub(crate) fn text(&self) -> &str {
        match self {
            Self::Function { definition, .. } => definition,
            Self::File { text, .. } | Self::Add { text, .. } => text,
        }
    }
}

impl Update {
    /// Construct an empty detached batch with a fresh cancellation token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace one complete function definition in its original owner/import context.
    /// Paths use the selected Program's checked public exports; opaque locators also
    /// address private functions, methods and named locals. Body fragments, renaming
    /// the target and changing its frozen callable contract reject at submission.
    pub fn replace_function(
        &mut self,
        target: impl Into<FunctionUpdateTarget>,
        definition: impl Into<String>,
    ) -> &mut Self {
        self.edits.push(Edit::Function {
            target: target.into(),
            definition: definition.into(),
        });
        self
    }

    /// Replace an existing captured document, preserving its physical module mapping.
    /// Names are normalized against the captured source root without filesystem IO.
    pub fn replace_file(&mut self, name: impl Into<String>, text: impl Into<String>) -> &mut Self {
        self.edits.push(Edit::File {
            name: name.into(),
            text: text.into(),
        });
        self
    }

    /// Add a physical file with an explicit module in an already captured package.
    /// No dependency acquisition, file discovery or deletion is performed.
    pub fn add_file(
        &mut self,
        module: ModuleIdentity,
        name: impl Into<String>,
        text: impl Into<String>,
    ) -> &mut Self {
        self.edits.push(Edit::Add {
            module,
            name: name.into(),
            text: text.into(),
        });
        self
    }

    /// Select this submission's cancellation token instead of the fresh default.
    /// Cancellation before commit rejects the whole batch without changing its baseline.
    pub fn with_cancellation(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }
}
