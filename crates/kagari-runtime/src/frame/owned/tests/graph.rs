//! Real Native providers expose private alias mutation and scan containment.
use crate::{
    Runtime,
    host::{HostObjectId, HostSchemaEpoch},
    metadata::TypeId,
    module::{ExecutableModule, InstalledProgram},
    native::{
        builder::ModuleBuilder,
        payload::NativeType,
        storage::{NativePayload, NativeStorage},
        typed::NativeContext,
    },
    test_support::update_programs::load_empty,
    value::{carriers::RootedValue, host_values::HostRootObject, native::NativeObject},
};
use kagari_gc::{
    access::EdgeField,
    error::GcError,
    object::{AnyObject, Edge, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_types::declaration::host::{
    HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Debug, Default)]
pub(super) struct Probe {
    pub(super) scans: AtomicUsize,
    pub(super) drops: AtomicUsize,
    pub(super) fail: AtomicBool,
}

#[derive(Debug)]
struct Link {
    next: Option<Edge<AnyObject>>,
    probe: Arc<Probe>,
}

impl Link {
    // SAFETY: one initialized optional edge; checked edits publish before commit.
    const NEXT: EdgeField<Self, AnyObject> =
        unsafe { EdgeField::new(|link| &link.next, |link| &mut link.next) };
}

impl NativePayload for Link {
    fn registration_storage() -> NativeStorage {
        NativeStorage::payload::<Self>()
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.probe.drops.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: one fixed optional edge, no owned backing and no peer-aware destructor.
// The fail flag deliberately violates the scanner contract to test containment.
unsafe impl GcObject for Link {
    type ScanState = bool;
    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { units: 1, bytes: 0 }
    }

    fn begin_scan(&self) -> bool {
        self.probe.scans.fetch_add(1, Ordering::Relaxed);
        false
    }

    fn scan(
        &self,
        visited: &mut bool,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        assert!(
            !self.probe.fail.load(Ordering::Relaxed),
            "parking scan failure"
        );
        if !*visited {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            if let Some(next) = &self.next {
                // SAFETY: the admitted Link owns this initialized field.
                if unsafe { visitor.try_visit_stored(next, budget)? } == ScanStatus::Pending {
                    return Ok(ScanStatus::Pending);
                }
            }
            *visited = true;
        }
        Ok(ScanStatus::Complete)
    }
}

pub(super) struct Graphs {
    _program: InstalledProgram,
    owner: ExecutableModule,
    ty: NativeType<Link>,
    external: TypeId,
}

impl Graphs {
    pub(super) fn install(runtime: &mut Runtime) -> Self {
        let mut builder = ModuleBuilder::new("parking_graph", &Default::default()).unwrap();
        let mut link = builder.define_type("Link").unwrap();
        link.native_storage(NativeStorage::payload::<Link>())
            .unwrap();
        let link = link.finish().unwrap();
        let mut external = builder.define_type("External").unwrap();
        external
            .external(HostTypePolicy {
                ownership: HostTypeOwnership::HostRoot,
                reflection: HostReflectionPolicy::Hidden,
                path_access: PathAccess::ReadOnly,
            })
            .unwrap();
        external.finish().unwrap();
        builder.finish().unwrap().install(runtime).unwrap();
        let program = load_empty(runtime, "parking-graph");
        let owner = runtime.select_program(&program).unwrap();
        let ty = runtime
            .bind_native_type::<Link>(&program.root(), &link, &[])
            .unwrap();
        let external = runtime
            .types()
            .get_by_name("parking_graph::External")
            .unwrap()
            .id;
        Self {
            _program: program,
            owner,
            ty,
            external,
        }
    }

    pub(super) fn allocate(
        &self,
        runtime: &mut Runtime,
        size: usize,
        cyclic: bool,
        probe: &Arc<Probe>,
    ) -> RootedValue {
        assert!(size > 0);
        let mut context = NativeContext::new(runtime, &self.owner).unwrap();
        let object = self
            .ty
            .create_with(&mut context, |init| {
                let mut first = None;
                let mut last = None;
                for _ in 0..size {
                    // SAFETY: all incoming edges are scoped and installed by Init;
                    // the scanner owns the only possible field at a fixed position.
                    let next = unsafe {
                        init.scope().alloc_with(|mut init| {
                            Ok(Link {
                                next: last.map(|value| init.edge_scoped(value)).transpose()?,
                                probe: probe.clone(),
                            })
                        })
                    }?;
                    first.get_or_insert(next);
                    last = Some(next.erase());
                }
                let last = init.scope().downcast::<Link>(last.unwrap())?;
                if cyclic {
                    Link::NEXT.set(init.scope(), first.unwrap(), Some(last.erase()));
                }
                Ok(last)
            })
            .unwrap();
        let value = context
            .conversion
            .encode_value(self.ty.type_argument(), object)
            .unwrap();
        context.conversion.retain(&value).unwrap()
    }

    /// Mutate the actual private provider edge through a second genuine root.
    /// The Host object is valid in this Runtime but cannot enter parked data.
    pub(super) fn hide_host_root(&self, runtime: &mut Runtime, value: &RootedValue) {
        let host = runtime
            .register_host_root(HostObjectId(7), self.external, HostSchemaEpoch::new(0))
            .unwrap();
        runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let value = value.get(cx)?;
                let header = cx.downcast::<NativeObject>(value.object().unwrap())?;
                let payload = NativeObject::payload(cx, header);
                let link = cx.downcast::<Link>(payload)?;
                let host = HostRootObject::alloc(cx, host)?;
                Link::NEXT.set(cx, link, Some(host.erase()));
                Ok(())
            })
            .unwrap();
    }
}
