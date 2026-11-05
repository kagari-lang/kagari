//! Capturable bindings retain declaration identity rather than a spelling lookup.
//!
//! In `fn make(x: i32) { var y = x; || { y = y + x; } }`, the closure captures
//! the local `y` and parameter `x` in resolution order. Each record below joins
//! the binding's original name/range, checked type and declared rebinding policy.
//! Shadowed names have different IDs, so source text is never used as identity.

use crate::{
    compile_input::body::CheckedBodyInput,
    hir::{
        ids::{ExprId, LocalId, ParamId},
        writeability::Writeability,
    },
    resolver::resolved::ResolvedName,
    types::TypeId,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    span::Span,
};

/// A lexical value binding; declarations such as statics/functions are not captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingId {
    /// A local introduced by a pattern or a closure parameter.
    Local(LocalId),
    /// A parameter of the enclosing named function.
    Parameter(ParamId),
}

impl BindingId {
    /// Extracts a lexical value identity, rejecting type-level and module declarations.
    pub fn from_resolution(resolved: &ResolvedName) -> Option<Self> {
        match resolved {
            ResolvedName::Local(id) => Some(Self::Local(*id)),
            ResolvedName::Param(id) => Some(Self::Parameter(*id)),
            _ => None,
        }
    }

    fn resolution(self) -> ResolvedName {
        match self {
            Self::Local(id) => ResolvedName::Local(id),
            Self::Parameter(id) => ResolvedName::Param(id),
        }
    }
}

/// Checked binding metadata borrowed from one admitted source snapshot.
///
/// `Var` describes source rebinding; it does not select a physical cell or freeze
/// aliases. Elaboration decides which captured mutable locals need shared storage.
/// The type can still contain generic binders, but a consumed capture must not
/// contain inference variables or recovery types.
#[derive(Debug, Clone)]
pub struct BindingFacts<'a> {
    /// Arena/owner-qualified identity, independent of the source name.
    pub id: BindingId,
    /// Original binding spelling, including a shadowed declaration's own name.
    pub name: &'a str,
    /// Original binding range; closure construction has a different expression range.
    pub span: Span,
    /// Checked binding type before instance substitution.
    pub ty: TypeId,
    /// Declared source rebinding policy.
    pub writeability: Writeability,
}

impl<'a> CheckedBodyInput<'a> {
    /// Projects an enclosing owner's local/parameter binding, or rejects a foreign identity.
    ///
    /// Closures share their named owner's allocation so a capture may refer to an
    /// outer lexical scope. This lookup does not perform name resolution: the ID
    /// is already selected, and the matching scope only supplies display/policy data.
    pub fn binding(self, id: BindingId) -> Option<BindingFacts<'a>> {
        let (ty, span) = match id {
            BindingId::Local(local) => (self.local_type(local)?, self.local_span(local)?),
            BindingId::Parameter(parameter) => (
                self.parameter_type(parameter)?.clone(),
                self.origins.param_span(parameter),
            ),
        };
        let binding = self.names.binding(&id.resolution())?;
        Some(BindingFacts {
            id,
            name: &binding.name,
            span,
            ty,
            writeability: binding.writeability,
        })
    }

    /// Projects complete capture records in the selected closure's evaluation order.
    ///
    /// Returns `None` for foreign/non-callable IDs or an incomplete captured binding.
    /// An empty vector means a valid closure captures nothing. The enclosing input
    /// and its nested root obtain the same binding records from the same snapshot.
    /// Cancellation discards the complete projection, with checks at bounded batches
    /// and before return; a partial capture list never escapes.
    pub fn captures(
        self,
        closure: ExprId,
        cancel: &CancellationToken,
    ) -> Result<Option<Vec<BindingFacts<'a>>>, Cancelled> {
        cancel.check()?;
        if self.closure(closure).is_none() {
            return Ok(None);
        }
        let captures = self.names.closure_captures(closure);
        let mut result = Vec::with_capacity(captures.len());
        for (index, resolved) in captures.iter().enumerate() {
            if index % 64 == 0 {
                cancel.check()?;
            }
            let Some(binding) =
                BindingId::from_resolution(resolved).and_then(|id| self.binding(id))
            else {
                return Ok(None);
            };
            result.push(binding);
        }
        cancel.check()?;
        Ok(Some(result))
    }
}
