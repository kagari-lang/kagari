//! Captured values and exact executable backing are actual closure-owned edges.
#[cfg(test)]
mod tests;

use crate::{
    closure::{ClosureRecord, ClosureTarget},
    execution_metadata::managed::environments::EnvironmentObject,
    module::managed::revision::RevisionState,
    resource::candidate::CandidateOrigin,
    value::{
        carriers::{ManagedRepresentation, ScopedValue, StoredValue},
        kind::ObjectKind,
        properties::ValueProperties,
    },
};
use kagari_bytecode::module::CallableTarget;
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Edge, Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct ClosureObject {
    origin: CandidateOrigin,
    record: Arc<ClosureRecord>,
    body: Option<Edge<RevisionState>>,
    environment: Option<Edge<EnvironmentObject>>,
    captures: Vec<StoredValue>,
}

impl ClosureObject {
    /// The semantic producer checks captured types and call policy before this
    /// physical constructor. Every input is protected before any source growth.
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        record: Arc<ClosureRecord>,
        body: Gc<'gc, RevisionState>,
        environment: Option<Gc<'gc, EnvironmentObject>>,
        captures: &[ScopedValue<'gc>],
        origin: CandidateOrigin,
    ) -> Result<Gc<'gc, Self>, GcError> {
        if cx.read(body, |body| body.code().program_identity())
            != record.implementation.program_identity()
            || captures.len() != record.capture_types.len()
        {
            return Err(GcError::Representation);
        }
        match (&record.environment, environment) {
            (Some(facts), Some(value))
                if cx.read(value, |value| {
                    value.id() == facts.id && Arc::ptr_eq(&value.record().types, &facts.types)
                }) => {}
            (None, None) => {}
            _ => return Err(GcError::Representation),
        }
        match record.callable_target() {
            CallableTarget::Script(function) => {
                let parameters = &record
                    .implementation
                    .bytecode
                    .functions
                    .get(function.index())
                    .ok_or(GcError::Index)?
                    .metadata
                    .params;
                if captures.len() > parameters.len()
                    || captures
                        .iter()
                        .zip(parameters)
                        .any(|(value, ty)| !value.has_representation(*ty))
                {
                    return Err(GcError::Representation);
                }
            }
            CallableTarget::Native(_) if !captures.is_empty() => {
                return Err(GcError::Representation);
            }
            CallableTarget::Native(_) => {}
        }
        for value in captures {
            if !ValueProperties::of(cx, *value)?.storable {
                return Err(GcError::Representation);
            }
        }
        // SAFETY: typed supplying state and every captured carrier enter through
        // Init. Partial construction drops inert backing; no owner is published
        // until all fields exist. The scanner covers exactly these initialized edges.
        unsafe {
            cx.alloc_with(|mut init| {
                let body = if matches!(record.target, ClosureTarget::Named(_)) {
                    None
                } else {
                    Some(init.edge_scoped(body)?)
                };
                let environment = environment
                    .map(|value| init.edge_scoped(value))
                    .transpose()?;
                let mut stored = Vec::new();
                stored.try_reserve_exact(captures.len())?;
                for value in captures {
                    stored.push(StoredValue::initialize(&mut init, *value)?);
                }
                Ok(Self {
                    origin,
                    record,
                    body,
                    environment,
                    captures: stored,
                })
            })
        }
    }

    pub(crate) fn origin(&self) -> &CandidateOrigin {
        &self.origin
    }

    pub(crate) fn record(&self) -> &Arc<ClosureRecord> {
        &self.record
    }

    pub(crate) fn body<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Option<Gc<'gc, RevisionState>> {
        cx.follow(owner, |owner| owner.body.as_ref())
    }

    pub(crate) fn environment<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Option<Gc<'gc, EnvironmentObject>> {
        cx.follow(owner, |owner| owner.environment.as_ref())
    }

    pub(crate) fn capture<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        index: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        StoredValue::get_field(cx, owner, |owner| owner.captures.get(index))
    }
}

impl ManagedRepresentation for ClosureObject {
    const KIND: ObjectKind = ObjectKind::Closure;
}

// SAFETY: a closure owns its body, optional environment and every capture. Facts
// contain no managed references or Host root. The immutable scan frontier retries
// a pending coordinate, and destruction never dereferences a managed peer.
unsafe impl GcObject for ClosureObject {
    type ScanState = usize;
    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> usize {
        0
    }

    fn scan(
        &self,
        next: &mut usize,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        loop {
            if *next >= self.captures.len() + 2 {
                return Ok(ScanStatus::Complete);
            }
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: every selected coordinate is retained by this admitted provider.
            let status = unsafe {
                match *next {
                    0 => match &self.body {
                        Some(body) => visitor.try_visit_stored(body, budget)?,
                        None => ScanStatus::Complete,
                    },
                    1 => match &self.environment {
                        Some(environment) => visitor.try_visit_stored(environment, budget)?,
                        None => ScanStatus::Complete,
                    },
                    index => self.captures[index - 2].scan(budget, visitor)?,
                }
            };
            if status == ScanStatus::Pending {
                return Ok(status);
            }
            *next += 1;
        }
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self.captures.capacity() * size_of::<StoredValue>(),
            units: self.captures.len() + 2,
        }
    }
}
