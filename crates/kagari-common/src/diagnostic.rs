use std::fmt::{self, Display, Formatter};

use crate::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span: None,
        }
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }
}

impl Display for Diagnostic {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.span {
            Some(span) => write!(
                f,
                "{:?}: {} at {}..{}",
                self.severity, self.message, span.start, span.end
            ),
            None => write!(f, "{:?}: {}", self.severity, self.message),
        }
    }
}
