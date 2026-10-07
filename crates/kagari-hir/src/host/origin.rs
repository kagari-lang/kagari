//! Optional tool metadata supplied alongside an offline host interface.
//! Locations never participate in native binding authority or ABI fingerprints.

use std::collections::HashMap;
use {
    kagari_common::{identity::DefinitionPath, span::Span},
    kagari_source::source_database::normalize_source_name,
    kagari_types::host_interface::{HostInterface, HostInterfaceError},
};

#[cfg(test)]
mod tests;

/// A supplied URI and half-open UTF-8 byte range. No filesystem or network read
/// occurs during installation. A tool opening the document must still check the
/// range against its current contents; this metadata does not pin that document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSourceLocation {
    uri: String,
    range: Span,
}

impl HostSourceLocation {
    /// Validates an origin URI and ordered half-open byte range without reading the document.
    ///
    /// # Errors
    ///
    /// Returns `InvalidDeclaration` for an invalid URI/source name, whitespace/control
    /// characters, or a reversed range. The range is not checked against file contents.
    pub fn new(uri: impl Into<String>, range: Span) -> Result<Self, HostInterfaceError> {
        let uri = uri.into();
        let valid_scheme = uri.split_once("://").is_some_and(|(scheme, _)| {
            scheme
                .bytes()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic())
        });
        if range.start > range.end
            || !valid_scheme
            || uri.chars().any(|ch| ch.is_control() || ch.is_whitespace())
            || normalize_source_name(&uri).is_err()
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        Ok(Self { uri, range })
    }

    /// Borrows the supplied navigation URI.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Returns the supplied half-open UTF-8 byte range.
    pub fn range(&self) -> Span {
        self.range
    }
}

/// Optional declaration and Rust implementation navigation sites, independent of binding authority.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostDeclarationOrigin {
    /// Preferred navigation target, usually a generated Kagari declaration.
    pub declaration: Option<HostSourceLocation>,
    /// Optional implementation location provided by Rust registration tooling.
    pub rust: Option<HostSourceLocation>,
}

impl HostDeclarationOrigin {
    /// Prefers the declaration view, then the Rust implementation site, or returns `None`.
    pub fn preferred(&self) -> Option<&HostSourceLocation> {
        self.declaration.as_ref().or(self.rust.as_ref())
    }
}

/// Source analysis input. The portable interface remains the executable contract;
/// origins are a separate, optional view keyed by identities in that interface.
#[derive(Debug, Clone, Default)]
pub struct HostInput {
    /// Authoritative portable host callable/type/access declarations.
    pub interface: HostInterface,
    /// Optional navigation sites keyed by identities present in the interface.
    pub origins: HashMap<DefinitionPath, HostDeclarationOrigin>,
}

impl From<HostInterface> for HostInput {
    fn from(interface: HostInterface) -> Self {
        Self {
            interface,
            origins: HashMap::new(),
        }
    }
}
