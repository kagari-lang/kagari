//! Optional tool metadata supplied alongside an offline host interface.
//! Locations never participate in native binding authority or ABI fingerprints.

use std::collections::HashMap;
use {
    kagari_common::{
        host_interface::{HostInterface, HostInterfaceError},
        identity::DefinitionPath,
        span::Span,
    },
    kagari_source::source_database::normalize_source_name,
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

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn range(&self) -> Span {
        self.range
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostDeclarationOrigin {
    /// Preferred navigation target, usually a generated Kagari declaration.
    pub declaration: Option<HostSourceLocation>,
    /// Optional implementation location provided by Rust registration tooling.
    pub rust: Option<HostSourceLocation>,
}

impl HostDeclarationOrigin {
    pub fn preferred(&self) -> Option<&HostSourceLocation> {
        self.declaration.as_ref().or(self.rust.as_ref())
    }
}

/// Source analysis input. The portable interface remains the executable contract;
/// origins are a separate, optional view keyed by identities in that interface.
#[derive(Debug, Clone, Default)]
pub struct HostInput {
    pub interface: HostInterface,
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
