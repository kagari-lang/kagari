//! Reuse checked signatures after body edits, rebasing all source-local type IDs.
use std::collections::HashMap;

use kagari_common::{Span, cancellation::CancellationToken};

use crate::{
    AnalysisResult, AnalyzedModule,
    hir::{FunctionKind, TypeRefId},
    lower::LoweredModule,
};

use super::ModuleSignatures;

struct Surface {
    text: String,
    bodies: Vec<BodyRange>,
}

struct BodyRange {
    original: Span,
    projected: Span,
    removed_after: usize,
}

impl Surface {
    fn new(module: &LoweredModule, cancel: &CancellationToken) -> Option<Self> {
        let source = module.source.text();
        let mut bodies = module
            .module
            .functions
            .iter()
            .filter(|f| matches!(f.kind, FunctionKind::User | FunctionKind::ImplMethod))
            .map(|f| module.source_map.block_span(f.body))
            .collect::<Vec<_>>();
        bodies.sort_by_key(|span| span.start);
        let mut text = String::new();
        let mut cursor = 0;
        let mut ranges = Vec::with_capacity(bodies.len());
        for body in &bodies {
            cancel.check().ok()?;
            // Malformed/overlapping recovery ranges cannot establish equivalence.
            if body.start < cursor || body.end < body.start + 2 {
                return None;
            }
            text.push_str(source.get(cursor..body.start)?);
            let projected = Span::new(text.len(), text.len() + 2);
            text.push_str("{}");
            cursor = body.end;
            ranges.push(BodyRange {
                original: *body,
                projected,
                removed_after: body.end - projected.end,
            });
        }
        text.push_str(source.get(cursor..)?);
        Some(Self {
            text,
            bodies: ranges,
        })
    }

    fn project(&self, offset: usize) -> Option<usize> {
        let count = self
            .bodies
            .partition_point(|body| body.original.end <= offset);
        if self
            .bodies
            .get(count)
            .is_some_and(|body| body.original.start < offset)
        {
            return None;
        }
        let removed = count
            .checked_sub(1)
            .map_or(0, |i| self.bodies[i].removed_after);
        offset.checked_sub(removed)
    }

    fn expand(&self, offset: usize) -> Option<usize> {
        let count = self
            .bodies
            .partition_point(|body| body.projected.end <= offset);
        if self
            .bodies
            .get(count)
            .is_some_and(|body| body.projected.start < offset)
        {
            return None;
        }
        let removed = count
            .checked_sub(1)
            .map_or(0, |i| self.bodies[i].removed_after);
        offset.checked_add(removed)
    }

    fn span(&self, span: Span) -> Option<(usize, usize)> {
        Some((self.project(span.start)?, self.project(span.end)?))
    }
}

pub(crate) fn reuse_signatures(
    previous: &AnalyzedModule,
    current: &LoweredModule,
    cancel: &CancellationToken,
) -> Option<AnalysisResult<ModuleSignatures>> {
    let old = Surface::new(&previous.lowered, cancel)?;
    let new = Surface::new(current, cancel)?;
    if old.text != new.text
        || previous.lowered.source.module_identity() != current.source.module_identity()
    {
        return None;
    }
    // Declaration order is unchanged. Body-local type annotations, however, may
    // change both the lengths and indices of the shared type arena.
    let mut old_types: HashMap<_, Vec<_>> = HashMap::new();
    for (index, span) in previous.lowered.source_map.type_spans().iter().enumerate() {
        cancel.check().ok()?;
        if let Some(key) = old.span(*span) {
            old_types
                .entry(key)
                .or_default()
                .push(TypeRefId::new(index));
        }
    }
    let mut new_types: HashMap<_, Vec<_>> = HashMap::new();
    for (index, span) in current.source_map.type_spans().iter().enumerate() {
        cancel.check().ok()?;
        if let Some(key) = new.span(*span) {
            new_types
                .entry(key)
                .or_default()
                .push(TypeRefId::new(index));
        }
    }
    if old_types.len() != new_types.len() {
        return None;
    }
    let mut ids = HashMap::new();
    for (span, old_ids) in old_types {
        cancel.check().ok()?;
        let new_ids = new_types.remove(&span)?;
        if old_ids.len() != new_ids.len() {
            return None;
        }
        ids.extend(old_ids.into_iter().zip(new_ids));
    }
    let mut result = previous.signatures.as_ref().clone();
    let old_functions = &previous.lowered.module.functions;
    let new_functions = &current.module.functions;
    if old_functions.len() != new_functions.len()
        || result.facts.functions.len() != new_functions.len()
    {
        return None;
    }
    for ((old, new), typed) in old_functions
        .iter()
        .zip(new_functions)
        .zip(&mut result.facts.functions)
    {
        cancel.check().ok()?;
        if old.id != new.id
            || old.name != new.name
            || old.kind != new.kind
            || old.params.len() != new.params.len()
        {
            return None;
        }
        for (param, new) in typed.params.iter_mut().zip(&new.params) {
            param.id = new.id;
        }
    }
    result.facts.type_table = result.facts.type_table.remap_signature_types(&ids)?;
    for diagnostic in &mut result.diagnostics {
        cancel.check().ok()?;
        if let Some(span) = diagnostic.span {
            let (start, end) = old.span(span)?;
            diagnostic.span = Some(Span::new(new.expand(start)?, new.expand(end)?));
        }
    }
    Some(result)
}
