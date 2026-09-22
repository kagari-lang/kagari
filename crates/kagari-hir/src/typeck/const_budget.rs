use kagari_common::{Diagnostic, DiagnosticKind, Span};

/// Per-file budget shared by const capability validation and scalar evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstLimits {
    pub max_steps: usize,
    pub max_depth: usize,
}
impl Default for ConstLimits {
    fn default() -> Self {
        Self {
            max_steps: 100_000,
            max_depth: 64,
        }
    }
}

pub(super) struct ConstBudget {
    limits: ConstLimits,
    steps: usize,
    depth: usize,
    pub exhausted: bool,
}
impl ConstBudget {
    pub fn new(limits: ConstLimits) -> Self {
        Self {
            limits,
            steps: 0,
            depth: 0,
            exhausted: false,
        }
    }
    pub fn enter(&mut self, span: Span, diagnostics: &mut crate::DiagnosticBuffer) -> bool {
        if self.exhausted {
            return false;
        }
        let exceeded = if self.steps >= self.limits.max_steps {
            Some(("const steps", self.limits.max_steps))
        } else if self.depth >= self.limits.max_depth {
            Some(("const depth", self.limits.max_depth))
        } else {
            None
        };
        if let Some((resource, limit)) = exceeded {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::CompileLimitExceeded { resource, limit })
                    .with_span(span),
            );
            self.exhausted = true;
            return false;
        }
        self.steps += 1;
        self.depth += 1;
        true
    }
    pub fn leave(&mut self) {
        self.depth -= 1;
    }
}
