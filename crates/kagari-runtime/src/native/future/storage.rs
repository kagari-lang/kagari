//! One-shot Future edges detach into one genuine computation owner.
use crate::{
    execution_metadata::managed::{Dependencies, ProtectedMetadata, sources::MetadataError},
    module::managed::revision::RevisionState,
    native::future::ColdFuture,
    resource::ResourceState,
    value::{
        carriers::{ScopedValue, StoredValue},
        kind::ObjectKind,
        properties::ValueProperties,
    },
};
use kagari_gc::{
    access::{EdgeField, Scope},
    error::GcError,
    object::{Edge, Gc, GcObject, ObjectCharge},
    root::Root,
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct ColdObject {
    recipe: Arc<ColdFuture>,
    body: Edge<RevisionState>,
    captures: Vec<StoredValue>,
    dependencies: Dependencies,
}

impl ColdObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        recipe: ColdFuture,
        body: Gc<'gc, RevisionState>,
        captures: &[ScopedValue<'gc>],
        metadata: &[ProtectedMetadata<'gc>],
        resources: &ResourceState,
    ) -> Result<Gc<'gc, Self>, MetadataError> {
        if captures.len() != recipe.capture_count()
            || (matches!(recipe, ColdFuture::Script { .. })
                && captures
                    .first()
                    .is_none_or(|value| value.kind() != Some(ObjectKind::Closure)))
        {
            return Err(GcError::Representation.into());
        }
        if cx.read(body, |body| body.code().program_identity()) != recipe.owner().program_identity()
        {
            return Err(GcError::Representation.into());
        }
        if metadata
            .iter()
            .any(|source| matches!(source, ProtectedMetadata::Cold(_)))
        {
            // Retain fixed executable evidence rather than unrelated captures
            // from another computation that supplied the conversion's metadata.
            return Err(GcError::Representation.into());
        }
        for (index, value) in captures.iter().enumerate() {
            if index.is_multiple_of(1024) {
                resources.poll_closed()?;
            }
            if !ValueProperties::of(cx, *value)?.storable {
                return Err(GcError::Representation.into());
            }
        }
        // SAFETY: every captured field and exact body comes from protected input;
        // metadata publication below retains/scans every incomplete prefix.
        let object = unsafe {
            cx.alloc_with(|mut init| {
                let body = init.edge_scoped(body)?;
                let mut values = Vec::new();
                values.try_reserve_exact(captures.len())?;
                for value in captures {
                    values.push(StoredValue::initialize(&mut init, *value)?);
                }
                Ok(Self {
                    recipe: Arc::new(recipe),
                    body,
                    captures: values,
                    dependencies: Dependencies::empty(metadata.len())?,
                })
            })?
        };
        // SAFETY: object is private and scans all initialized dependency prefixes.
        unsafe {
            Dependencies::publish(
                cx,
                object,
                |object| &mut object.dependencies,
                metadata,
                resources,
            )?;
        }
        Ok(object)
    }

    pub(crate) fn recipe(&self) -> &Arc<ColdFuture> {
        &self.recipe
    }

    pub(crate) fn len(&self) -> usize {
        self.captures.len()
    }

    pub(crate) fn capture<'gc>(
        cx: &Scope<'gc>,
        object: Gc<'gc, Self>,
        index: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        StoredValue::get_field(cx, object, |object| object.captures.get(index))
    }

    /// Retire decoded/submitted inputs while the selected body/type evidence
    /// remains owned by the pending producer. No arbitrary payload is extracted.
    pub(crate) fn release_captures<'gc>(
        cx: &mut Scope<'gc>,
        object: Gc<'gc, Self>,
    ) -> Result<(), GcError> {
        let charge = cx.read(object, |object| {
            let mut charge = object.dependencies.charge();
            charge.bytes += object.recipe.retained_bytes();
            charge.units += 1;
            charge
        });
        // SAFETY: charge admission precedes clearing. All dropped carriers are
        // inert. Body/dependency positions and their independent cursor stay fixed;
        // a captured numeric position beyond the now-empty buffer is already done.
        unsafe {
            cx.with_object_mut(object, |object, mutation| {
                mutation.prepare_charge(charge)?;
                object.captures = Vec::new();
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    pub(crate) fn body<'gc>(cx: &Scope<'gc>, object: Gc<'gc, Self>) -> Gc<'gc, RevisionState> {
        cx.follow(object, |object| Some(&object.body))
            .expect("initialized cold body")
    }

    pub(crate) fn metadata<'gc>(
        cx: &Scope<'gc>,
        object: Gc<'gc, Self>,
        resources: &ResourceState,
    ) -> Result<Vec<ProtectedMetadata<'gc>>, MetadataError> {
        let mut metadata =
            Dependencies::follow(cx, object, |object| &object.dependencies, resources)?;
        metadata.try_reserve(1)?;
        metadata.push(ProtectedMetadata::Revision(Self::body(cx, object)));
        Ok(metadata)
    }
}

// SAFETY: fixed exact-body edge, immutable captures and metadata adjacency are
// the complete graph. State contains only finite numeric positions; Drop reads no peer.
unsafe impl GcObject for ColdObject {
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
            // SAFETY: admitted owner retains its initialized body field.
            if unsafe { visitor.try_visit_stored(&self.body, budget)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            next.0 = 1;
        }
        while let Some(value) = self.captures.get(next.0 - 1) {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: all admitted immutable cells remain in this provider.
            if unsafe { value.scan(budget, visitor)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            next.0 += 1;
        }
        // SAFETY: the same owner retains the complete adjacency.
        unsafe { self.dependencies.scan(&mut next.1, budget, visitor) }
    }

    fn charge(&self) -> ObjectCharge {
        let mut charge = self.dependencies.charge();
        charge.bytes +=
            self.captures.capacity() * size_of::<StoredValue>() + self.recipe.retained_bytes();
        charge.units += self.captures.len() + 1;
        charge
    }
}

#[derive(Debug)]
pub(crate) struct FuturePayload {
    cold: Option<Edge<ColdObject>>,
}

impl FuturePayload {
    // SAFETY: fixed optional edge; replacing it leaves charge and scan positions unchanged.
    const COLD: EdgeField<Self, ColdObject> =
        unsafe { EdgeField::new(|value| &value.cold, |value| &mut value.cold) };

    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        cold: Gc<'gc, ColdObject>,
    ) -> Result<Gc<'gc, Self>, GcError> {
        // SAFETY: sole initialized field imported from protected construction.
        unsafe {
            cx.alloc_with(|mut init| {
                Ok(Self {
                    cold: Some(init.edge_scoped(cold)?),
                })
            })
        }
    }

    pub(crate) fn cold<'gc>(cx: &Scope<'gc>, object: Gc<'gc, Self>) -> Option<Gc<'gc, ColdObject>> {
        Self::COLD.get(cx, object)
    }

    /// Reserve ownership before the infallible fixed-edge commit. Root exhaustion
    /// leaves Cold intact; a consumed Future never exposes captures again.
    pub(crate) fn claim<'gc>(
        cx: &mut Scope<'gc>,
        object: Gc<'gc, Self>,
    ) -> Result<Option<Root<ColdObject>>, GcError> {
        let Some(cold) = Self::cold(cx, object) else {
            return Ok(None);
        };
        let root = cx.retain(cold)?;
        Self::COLD.set(cx, object, None);
        Ok(Some(root))
    }

    /// Both states and root reservations precede either one-shot commit.
    pub(crate) fn claim_pair<'gc>(
        cx: &mut Scope<'gc>,
        objects: [Gc<'gc, Self>; 2],
    ) -> Result<Option<[Root<ColdObject>; 2]>, GcError> {
        let [left, right] = objects;
        if left.ptr_eq(right) {
            return Err(GcError::Representation);
        }
        let Some(left_cold) = Self::cold(cx, left) else {
            return Ok(None);
        };
        let Some(right_cold) = Self::cold(cx, right) else {
            return Ok(None);
        };
        let roots = [cx.retain(left_cold)?, cx.retain(right_cold)?];
        Self::COLD.set(cx, left, None);
        Self::COLD.set(cx, right, None);
        Ok(Some(roots))
    }
}

// SAFETY: fixed optional edge, checked publication and constant charge. Empty
// is the semantic consumed state, not extraction/tombstoning of arbitrary payloads.
unsafe impl GcObject for FuturePayload {
    type ScanState = bool;
    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) -> bool {
        false
    }

    fn scan(
        &self,
        next: &mut bool,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !*next {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            if let Some(cold) = &self.cold {
                // SAFETY: admitted object retains this initialized optional edge.
                if unsafe { visitor.try_visit_stored(cold, budget)? } == ScanStatus::Pending {
                    return Ok(ScanStatus::Pending);
                }
            }
            *next = true;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { bytes: 0, units: 1 }
    }
}
