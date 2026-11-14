//! One selected code generation owns materialized constants and linked state.
use crate::{
    execution_metadata::managed::{Dependencies, ProtectedMetadata, sources::MetadataError},
    module::{
        ExecutableModule,
        managed::{InstallationState, cell::StaticCellObject, statics::StaticState},
        retirement::RetirementTicket,
    },
    resource::ResourceState,
    value::{
        carriers::{ScopedValue, StoredValue},
        strings::StringObject,
    },
};
use kagari_bytecode::{
    instruction::{ConstantId, ConstantOperand, ModuleSlot},
    program::ModuleRef,
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Edge, Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};

/// Code/type descriptors are immutable and edge-free. Constants, static bindings
/// and fixed linked dependencies are actual collector edges, not Arc-only roots.
#[derive(Debug)]
pub(crate) struct RevisionState {
    _retirement: RetirementTicket,
    code: ExecutableModule,
    installation: Edge<InstallationState>,
    statics: Option<Edge<StaticState>>,
    constants: Vec<StoredValue>,
    initialized: Vec<bool>,
    offsets: Vec<MemberOffsets>,
    dependencies: Dependencies,
    metadata_ready: bool,
    bytes: usize,
    units: usize,
}

/// Prefixes are prepared once from verified module layouts, never recomputed by
/// a warm constant/static read. The final entry holds both total lengths.
#[derive(Debug, Clone, Copy)]
struct MemberOffsets {
    constant: usize,
    static_slot: usize,
}

impl RevisionState {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        installation: Gc<'gc, InstallationState>,
        code: ExecutableModule,
        retirement: RetirementTicket,
    ) -> Result<Gc<'gc, Self>, GcError> {
        if cx.read(installation, InstallationState::identity)
            != (code.registry_owner, code.installation_id())
        {
            return Err(GcError::Representation);
        }
        let code = code.program_root();
        let mut offsets = Vec::new();
        let mut count = 0usize;
        let mut static_count = 0usize;
        for member in code.members() {
            offsets.try_reserve(1)?;
            offsets.push(MemberOffsets {
                constant: count,
                static_slot: static_count,
            });
            count = count
                .checked_add(member.bytecode.constants.len())
                .ok_or(GcError::Limit("revision constants"))?;
            static_count = static_count
                .checked_add(member.bytecode.module_slots.len())
                .ok_or(GcError::Limit("revision static bindings"))?;
        }
        offsets.try_reserve(1)?;
        offsets.push(MemberOffsets {
            constant: count,
            static_slot: static_count,
        });
        let mut constants = Vec::new();
        constants.try_reserve_exact(count)?;
        constants.resize_with(count, StoredValue::unit);
        let mut initialized = Vec::new();
        initialized.try_reserve_exact(count)?;
        initialized.resize(count, false);
        let bytes = owned_bytes(
            constants.capacity(),
            initialized.capacity(),
            offsets.capacity(),
        )?;
        // SAFETY: every cell is initialized Unit; the protected installation is
        // imported by Init. Linking and constant writes below retain full tracing.
        unsafe {
            cx.alloc_with(|mut init| {
                Ok(Self {
                    _retirement: retirement,
                    code,
                    installation: init.edge_scoped(installation)?,
                    statics: None,
                    constants,
                    initialized,
                    offsets,
                    dependencies: Dependencies::empty(0)?,
                    metadata_ready: false,
                    bytes,
                    units: count,
                })
            })
        }
    }

    pub(crate) fn code(&self) -> &ExecutableModule {
        &self.code
    }

    pub(crate) fn installation<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Gc<'gc, InstallationState> {
        cx.follow(owner, |revision| Some(&revision.installation))
            .expect("revision installation edge")
    }

    pub(crate) fn statics<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Option<Gc<'gc, StaticState>> {
        cx.follow(owner, |revision| revision.statics.as_ref())
    }

    /// Publish the finite closed-call dependencies selected while linking this
    /// exact version. Dynamic applications/historical contexts never enter here.
    pub(crate) fn link_metadata<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        dependencies: &[ProtectedMetadata<'gc>],
        resources: &ResourceState,
    ) -> Result<(), MetadataError> {
        if cx.read(owner, |owner| owner.metadata_ready) {
            return Err(GcError::Storage("revision metadata was already linked").into());
        }
        let prepared = Dependencies::empty(dependencies.len())?;
        let charge = cx.read(owner, |owner| {
            Ok::<_, GcError>(ObjectCharge {
                bytes: owner
                    .bytes
                    .checked_add(prepared.charge().bytes)
                    .ok_or(GcError::Limit("revision metadata backing"))?,
                units: owner.units,
            })
        })?;
        // SAFETY: an unpublished candidate owns this complete empty container;
        // reserve/admit its charge before replacing the fixed metadata field.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                mutation.prepare_charge(charge)?;
                owner.dependencies = prepared;
                owner.metadata_ready = true;
                Ok(())
            })?;
            Dependencies::publish(
                cx,
                owner,
                |owner| &mut owner.dependencies,
                dependencies,
                resources,
            )?;
        }
        Ok(())
    }

    pub(crate) fn metadata<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        resources: &ResourceState,
    ) -> Result<Vec<ProtectedMetadata<'gc>>, MetadataError> {
        Dependencies::follow(cx, owner, |owner| &owner.dependencies, resources)
    }

    /// Static binding is independent of fixed executable metadata preparation.
    /// It never replaces the already published closed-call graph.
    pub(crate) fn link<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        statics: Gc<'gc, StaticState>,
    ) -> Result<(), GcError> {
        if cx.read(owner, |owner| owner.statics.is_some()) {
            return Err(GcError::Storage("revision was already linked"));
        }
        if !Self::installation(cx, owner).ptr_eq(StaticState::installation(cx, statics)) {
            return Err(GcError::Representation);
        }
        if cx.read(statics, StaticState::len)
            != cx.read(owner, |owner| {
                owner
                    .offsets
                    .last()
                    .expect("revision offset sentinel")
                    .static_slot
            })
        {
            return Err(GcError::Representation);
        }
        // SAFETY: the protected static container is published before the coupled
        // commit; fixed metadata and its charge are unchanged. Restart scanning.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let statics = mutation.edge_scoped(statics)?;
                owner.statics = Some(statics);
                Ok(())
            })
        }
    }

    pub(crate) fn static_cell<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        module: ModuleRef,
        slot: ModuleSlot,
    ) -> Result<Gc<'gc, StaticCellObject>, GcError> {
        let position = cx.read(owner, |owner| {
            let begin = owner
                .offsets
                .get(module.index())
                .ok_or(GcError::Index)?
                .static_slot;
            let end = owner
                .offsets
                .get(module.index().checked_add(1).ok_or(GcError::Index)?)
                .ok_or(GcError::Index)?
                .static_slot;
            let position = begin.checked_add(slot.index()).ok_or(GcError::Index)?;
            if position >= end {
                return Err(GcError::Index);
            }
            Ok(position)
        })?;
        let statics =
            Self::statics(cx, owner).ok_or(GcError::Storage("revision statics are not linked"))?;
        StaticState::get(cx, statics, position)
    }

    /// Resolve one literal through this already protected revision. Warm reads
    /// register no root and allocate no copy of the constant buffer.
    pub(crate) fn cached_constant<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        module: ModuleRef,
        constant: ConstantId,
    ) -> Result<Option<ScopedValue<'gc>>, GcError> {
        let position = cx.read(owner, |owner| owner.position(module, constant))?;
        if !cx.read(owner, |owner| owner.initialized[position]) {
            return Ok(None);
        }
        StoredValue::get_field(cx, owner, |owner| owner.constants.get(position)).map(Some)
    }

    /// Materialize a literal only outside a closed interpreter region.
    pub(crate) fn constant<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        module: ModuleRef,
        constant: ConstantId,
    ) -> Result<ScopedValue<'gc>, GcError> {
        let (position, source) = cx.read(owner, |owner| {
            let position = owner.position(module, constant)?;
            let source = if owner.initialized[position] {
                None
            } else {
                Some(
                    owner
                        .code
                        .member(module)
                        .ok_or(GcError::Index)?
                        .bytecode
                        .constants
                        .get(constant.index())
                        .ok_or(GcError::Index)?
                        .clone(),
                )
            };
            Ok::<_, GcError>((position, source))
        })?;
        let Some(source) = source else {
            return StoredValue::get_field(cx, owner, |owner| owner.constants.get(position));
        };
        let value = match source {
            ConstantOperand::Unit => ScopedValue::unit(),
            ConstantOperand::Bool(value) => ScopedValue::from(value),
            ConstantOperand::Char(value) => ScopedValue::from(value),
            ConstantOperand::I32(value) => ScopedValue::from(value),
            ConstantOperand::I64(value) => ScopedValue::from(value),
            ConstantOperand::U64(value) => ScopedValue::from(value),
            ConstantOperand::F32(value) => ScopedValue::from(value),
            ConstantOperand::F64(value) => ScopedValue::from(value),
            ConstantOperand::Str(text) => ScopedValue::managed(StringObject::alloc(cx, text)?),
        };
        // SAFETY: fixed initialized cells, same-scope input and published coupled
        // replacement. There is no callback/reentry between lookup and commit.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                if owner.initialized[position] {
                    return Err(GcError::Storage("constant changed during exclusive access"));
                }
                let stored = StoredValue::publish_field(mutation, value)?;
                owner.constants[position] = stored;
                owner.initialized[position] = true;
                Ok(())
            })?;
        }
        Ok(value)
    }

    fn position(&self, module: ModuleRef, constant: ConstantId) -> Result<usize, GcError> {
        let begin = self
            .offsets
            .get(module.index())
            .ok_or(GcError::Index)?
            .constant;
        let end = self
            .offsets
            .get(module.index().checked_add(1).ok_or(GcError::Index)?)
            .ok_or(GcError::Index)?
            .constant;
        let position = begin.checked_add(constant.index()).ok_or(GcError::Index)?;
        if position >= end {
            return Err(GcError::Index);
        }
        Ok(position)
    }
}

fn owned_bytes(constants: usize, initialized: usize, offsets: usize) -> Result<usize, GcError> {
    [
        (constants, size_of::<StoredValue>()),
        (initialized, size_of::<bool>()),
        (offsets, size_of::<MemberOffsets>()),
    ]
    .into_iter()
    .try_fold(0usize, |bytes, (capacity, size)| {
        capacity
            .checked_mul(size)
            .and_then(|extra| bytes.checked_add(extra))
            .ok_or(GcError::Limit("revision backing"))
    })
}

// SAFETY: installation, statics, all initialized constant cells and every linked
// dependency are covered. Cursor is a numeric coordinate, repaired by default
// restart on edits. Cached charge is committed with backing changes. Immutable
// shared code backing is excluded; Drop releases no managed peer or Runtime.
unsafe impl GcObject for RevisionState {
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
        loop {
            let status = match next.0 {
                0 => {
                    if !budget.take(1) {
                        return Ok(ScanStatus::Pending);
                    }
                    // SAFETY: the admitted owner retains its installation edge.
                    unsafe { visitor.try_visit_stored(&self.installation, budget)? }
                }
                1 => {
                    if !budget.take(1) {
                        return Ok(ScanStatus::Pending);
                    }
                    match &self.statics {
                        // SAFETY: this initialized optional edge remains retained.
                        Some(edge) => unsafe { visitor.try_visit_stored(edge, budget)? },
                        None => ScanStatus::Complete,
                    }
                }
                position => {
                    let index = position - 2;
                    if let Some(value) = self.constants.get(index) {
                        if !budget.take(1) {
                            return Ok(ScanStatus::Pending);
                        }
                        // SAFETY: initialized scalar/reference cell in this owner.
                        unsafe { value.scan(budget, visitor)? }
                    } else {
                        // SAFETY: this admitted revision owns the complete fixed
                        // metadata container, including any partial staging prefix.
                        return unsafe { self.dependencies.scan(&mut next.1, budget, visitor) };
                    }
                }
            };
            if status == ScanStatus::Pending {
                return Ok(status);
            }
            next.0 += 1;
        }
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self.bytes + self.dependencies.charge().bytes,
            units: self.units + self.dependencies.charge().units,
        }
    }
}
