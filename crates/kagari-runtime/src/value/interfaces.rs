//! Interface receiver identity and fixed executable metadata share one provider.
use crate::{
    execution_metadata::managed::{
        Dependencies, ProtectedMetadata, groups::OperationGroupObject, sources::MetadataError,
    },
    objects::interfaces::description::InterfaceDescription,
    resource::{ResourceState, candidate::CandidateOrigin},
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
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct InterfaceObject {
    origin: CandidateOrigin,
    description: Arc<InterfaceDescription>,
    data: StoredValue,
    parents: Vec<Option<Edge<Self>>>,
    dependencies: Dependencies,
    receiver_operations: Option<Edge<OperationGroupObject>>,
}

impl InterfaceObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        description: Arc<InterfaceDescription>,
        data: ScopedValue<'gc>,
        protected: &[ProtectedMetadata<'gc>],
        resources: &ResourceState,
    ) -> Result<Gc<'gc, Self>, MetadataError> {
        resources.poll_closed()?;
        // Durable external root descriptors remain allowed receivers. Scoped
        // Host views/loans and runtime temporaries cannot escape in an interface.
        if data.kind() != Some(ObjectKind::HostRoot) && !ValueProperties::of(cx, data)?.storable {
            return Err(GcError::Representation.into());
        }
        let edges = Dependencies::collect(resources, |visit| description.trace(resources, visit))?;
        let dependencies = Dependencies::resolve(cx, edges, protected, resources)?;
        let mut parents = Vec::new();
        parents.try_reserve_exact(description.parents.len())?;
        parents.resize_with(description.parents.len(), || None);
        // SAFETY: receiver initialized through Init; parent holes and metadata
        // adjacency are valid and scanned even before complete private publication.
        let object = unsafe {
            cx.alloc_with(|mut init| {
                Ok(Self {
                    origin: resources.candidate_origin(),
                    description,
                    receiver_operations: None,
                    data: StoredValue::initialize(&mut init, data)?,
                    parents,
                    dependencies: Dependencies::empty(dependencies.len())?,
                })
            })?
        };
        // SAFETY: no semantic handle exists yet; bounded publication retains and
        // scans every prefix using the metadata container's mutation barriers.
        unsafe {
            Dependencies::publish(
                cx,
                object,
                |owner| &mut owner.dependencies,
                &dependencies,
                resources,
            )?;
        }
        Ok(object)
    }

    pub(crate) fn origin(&self) -> &CandidateOrigin {
        &self.origin
    }

    pub(crate) fn description(&self) -> &Arc<InterfaceDescription> {
        &self.description
    }

    pub(crate) fn data<'gc>(
        cx: &Scope<'gc>,
        object: Gc<'gc, Self>,
    ) -> Result<ScopedValue<'gc>, GcError> {
        StoredValue::get_field(cx, object, |object| Some(&object.data))
    }

    pub(crate) fn parent<'gc>(
        cx: &Scope<'gc>,
        object: Gc<'gc, Self>,
        slot: usize,
    ) -> Result<Option<Gc<'gc, Self>>, GcError> {
        if slot >= cx.read(object, |object| object.parents.len()) {
            return Err(GcError::Index);
        }
        Ok(cx.follow(object, |object| object.parents[slot].as_ref()))
    }

    /// Publish once after the complete expected relationship and identical receiver
    /// have been admitted. Same-scope typed targets exclude foreign/stale IDs.
    pub(crate) fn publish_parent<'gc>(
        cx: &mut Scope<'gc>,
        object: Gc<'gc, Self>,
        slot: usize,
        parent: Gc<'gc, Self>,
    ) -> Result<(), GcError> {
        if object.ptr_eq(parent) {
            return Err(GcError::Representation);
        }
        let valid = cx.read(object, |object| {
            let Some(expected) = object.description.parents.get(slot) else {
                return false;
            };
            if object.parents[slot].is_some() {
                return false;
            }
            cx.read(parent, |parent| {
                parent.description.interface_type() == &expected.interface
                    && parent.description.implementation.key() == expected.binding.owner.key()
                    && parent.description.receiver_table.table == expected.binding.table
            })
        });
        if !valid {
            return Err(GcError::Representation);
        }
        let receiver = Self::data(cx, object)?;
        let parent_receiver = Self::data(cx, parent)?;
        let same = match (receiver.object(), parent_receiver.object()) {
            (Some(left), Some(right)) => {
                receiver.kind() == parent_receiver.kind() && left.ptr_eq(right)
            }
            (None, None) => {
                receiver.scalar_payload().is_some()
                    && receiver.scalar_payload() == parent_receiver.scalar_payload()
            }
            _ => false,
        };
        if !same {
            return Err(GcError::Representation);
        }
        // SAFETY: target is branded/protected and published before the coupled
        // field commit. This fixed scan position/charge does not move; publication
        // shades an incoming edge even if the old empty position was scanned.
        unsafe {
            cx.with_object_mut(object, |object, mutation| {
                let edge = mutation.edge_scoped(parent)?;
                object.parents[slot] = Some(edge);
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    pub(crate) fn receiver_operations<'gc>(
        cx: &Scope<'gc>,
        object: Gc<'gc, Self>,
    ) -> Option<Gc<'gc, OperationGroupObject>> {
        cx.follow(object, |object| object.receiver_operations.as_ref())
    }

    /// Install the dictionary prepared from this exact checked description.
    /// The policy caller binds its table and authenticates every dependency first.
    pub(crate) fn publish_receiver_operations<'gc>(
        cx: &mut Scope<'gc>,
        object: Gc<'gc, Self>,
        expected: &Arc<InterfaceDescription>,
        dictionary: Gc<'gc, OperationGroupObject>,
    ) -> Result<(), GcError> {
        if !cx.read(object, |object| {
            Arc::ptr_eq(&object.description, expected) && object.receiver_operations.is_none()
        }) {
            return Err(GcError::Representation);
        }
        // SAFETY: the dictionary is same-scope protected and the fixed optional
        // position is scanned. Shade before commit, including after a prior hole.
        unsafe {
            cx.with_object_mut(object, |object, mutation| {
                let edge = mutation.edge_scoped(dictionary)?;
                object.receiver_operations = Some(edge);
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    /// Metadata lookup follows only fixed relationships and receiver-specific
    /// parent views. It never searches the arbitrary mutable receiver graph.
    pub(crate) fn metadata<'gc>(
        cx: &Scope<'gc>,
        object: Gc<'gc, Self>,
        resources: &ResourceState,
    ) -> Result<Vec<ProtectedMetadata<'gc>>, MetadataError> {
        let mut children =
            Dependencies::follow(cx, object, |object| &object.dependencies, resources)?;
        let count = cx.read(object, |object| object.parents.len());
        children.try_reserve(count.checked_add(1).ok_or(GcError::Allocation)?)?;
        if let Some(dictionary) = Self::receiver_operations(cx, object) {
            children.push(ProtectedMetadata::Operations(dictionary));
        }
        for index in 0..count {
            if index.is_multiple_of(1024) {
                resources.poll_closed()?;
            }
            if let Some(parent) = cx.follow(object, |object| object.parents[index].as_ref()) {
                children.push(ProtectedMetadata::Interface(parent));
            }
        }
        Ok(children)
    }
}

impl ManagedRepresentation for InterfaceObject {
    const KIND: ObjectKind = ObjectKind::Interface;
}

// SAFETY: the receiver, fixed metadata adjacency and finite parent view positions
// are the complete managed graph. Shared descriptions contain immutable facts and
// no managed references. The scanner retains each pending position; Drop reads no peer.
unsafe impl GcObject for InterfaceObject {
    type ScanState = (usize, usize);
    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> Self::ScanState {
        (0, 0)
    }

    fn scan(
        &self,
        next: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if next.0 == 0 {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: the admitted object owns its initialized receiver.
            if unsafe { self.data.scan(budget, visitor)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            next.0 = 1;
        }
        while let Some(parent) = self.parents.get(next.0 - 1) {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            if let Some(parent) = parent {
                // SAFETY: this initialized optional edge belongs to the provider.
                if unsafe { visitor.try_visit_stored(parent, budget)? } == ScanStatus::Pending {
                    return Ok(ScanStatus::Pending);
                }
            }
            next.0 += 1;
        }
        if next.0 == self.parents.len() + 1 {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            if let Some(dictionary) = &self.receiver_operations {
                // SAFETY: this provider owns the initialized dictionary edge.
                if unsafe { visitor.try_visit_stored(dictionary, budget)? } == ScanStatus::Pending {
                    return Ok(ScanStatus::Pending);
                }
            }
            next.0 += 1;
        }
        // SAFETY: the admitted object retains this complete metadata container.
        unsafe { self.dependencies.scan(&mut next.1, budget, visitor) }
    }

    fn charge(&self) -> ObjectCharge {
        let mut charge = self.dependencies.charge();
        charge.bytes += self.parents.capacity() * size_of::<Option<Edge<Self>>>();
        charge.units += self.parents.len() + 2;
        charge
    }
}
