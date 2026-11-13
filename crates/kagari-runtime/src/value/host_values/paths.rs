//! Immutable path capabilities keep base and dynamic argument edges in the heap.
use crate::{
    host::{HostPathDescriptorId, HostRootHandle, HostSchemaEpoch},
    metadata::TypeId,
    value::{
        carriers::{ManagedRepresentation, ScopedValue, StoredValue},
        kind::ObjectKind,
        properties::ValueProperties,
    },
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Edge, Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_types::declaration::host::PathAccess;

/// Inert identity/access facts; HostRegistry validates the complete chain before
/// access. These fields contain no Rust host state or permission to dereference it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PathFacts {
    pub(crate) root: HostRootHandle,
    pub(crate) descriptor: HostPathDescriptorId,
    pub(crate) result: TypeId,
    pub(crate) access: PathAccess,
    pub(crate) epoch: HostSchemaEpoch,
}

#[derive(Debug)]
struct Argument {
    ty: TypeId,
    value: StoredValue,
}

#[derive(Debug)]
pub(crate) struct HostPathObject {
    facts: PathFacts,
    base: Option<Edge<Self>>,
    arguments: Vec<Argument>,
}

impl HostPathObject {
    /// Physical construction admits storable fields. Registry/type/access policy
    /// belongs to the semantic path owner, before any Host operation occurs.
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        facts: PathFacts,
        base: Option<Gc<'gc, Self>>,
        arguments: &[(TypeId, ScopedValue<'gc>)],
    ) -> Result<Gc<'gc, Self>, GcError> {
        for (_, value) in arguments {
            if !ValueProperties::of(cx, *value)?.storable {
                return Err(GcError::Representation);
            }
        }
        // SAFETY: all inputs are protected scoped handles; every initialized
        // edge remains in this immutable provider and its finite scanner.
        unsafe {
            cx.alloc_with(|mut init| {
                let base = base.map(|base| init.edge_scoped(base)).transpose()?;
                let mut stored = Vec::new();
                stored.try_reserve_exact(arguments.len())?;
                for (ty, value) in arguments {
                    stored.push(Argument {
                        ty: *ty,
                        value: StoredValue::initialize(&mut init, *value)?,
                    });
                }
                Ok(Self {
                    facts,
                    base,
                    arguments: stored,
                })
            })
        }
    }

    pub(crate) fn facts(&self) -> PathFacts {
        self.facts
    }

    pub(crate) fn len(&self) -> usize {
        self.arguments.len()
    }

    pub(crate) fn argument_type(&self, index: usize) -> Option<TypeId> {
        self.arguments.get(index).map(|argument| argument.ty)
    }

    pub(crate) fn base<'gc>(cx: &Scope<'gc>, owner: Gc<'gc, Self>) -> Option<Gc<'gc, Self>> {
        cx.follow(owner, |owner| owner.base.as_ref())
    }

    pub(crate) fn argument<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        index: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        StoredValue::get_field(cx, owner, |owner| {
            owner.arguments.get(index).map(|argument| &argument.value)
        })
    }
}

impl ManagedRepresentation for HostPathObject {
    const KIND: ObjectKind = ObjectKind::HostPath;
}

// SAFETY: fixed base and immutable coupled argument fields are the complete
// managed graph. Finite numeric positions retry pending targets without advance.
// Charge reads owned capacity/length; Drop releases no host state or managed peer.
unsafe impl GcObject for HostPathObject {
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
            let argument = if *next == 0 {
                None
            } else {
                match self.arguments.get(*next - 1) {
                    Some(argument) => Some(argument),
                    None => return Ok(ScanStatus::Complete),
                }
            };
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let status = match argument {
                // SAFETY: the admitted provider owns the initialized field.
                Some(argument) => unsafe { argument.value.scan(budget, visitor)? },
                // SAFETY: the admitted immutable path owns the optional base edge.
                None => match &self.base {
                    Some(base) => unsafe { visitor.try_visit_stored(base, budget)? },
                    None => ScanStatus::Complete,
                },
            };
            if status == ScanStatus::Pending {
                return Ok(status);
            }
            *next += 1;
        }
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            units: self.arguments.len() + 1,
            bytes: self.arguments.capacity() * size_of::<Argument>(),
        }
    }
}
