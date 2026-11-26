//! Real Native providers expose private alias mutation and scan containment.
use crate::{
    Runtime,
    interpreter_tests::common::compile_with_provider,
    module::{ExecutableModule, InstalledProgram},
    native::{
        builder::ModuleBuilder,
        module::NativeModule,
        payload::NativeType,
        storage::{NativePayload, NativeStorage},
        typed::NativeContext,
    },
    test_support::{bytecode::load_bytecode, update_programs::load_empty},
    value::{carriers::RootedValue, native::NativeObject},
};
use kagari_gc::{
    access::EdgeField,
    error::GcError,
    object::{AnyObject, Edge, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Debug, Default)]
pub(crate) struct Probe {
    pub(crate) scans: AtomicUsize,
    pub(crate) drops: AtomicUsize,
    pub(crate) fail: AtomicBool,
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

pub(crate) struct Graphs {
    _program: InstalledProgram,
    provider: NativeModule,
    owner: ExecutableModule,
    ty: NativeType<Link>,
}

impl Graphs {
    pub(crate) fn install(runtime: &mut Runtime) -> Self {
        let mut builder = ModuleBuilder::new("graph", &Default::default()).unwrap();
        let mut link = builder.define_type("External").unwrap();
        link.native_storage(NativeStorage::payload::<Link>())
            .unwrap();
        let link = link.finish().unwrap();
        let provider = builder.finish().unwrap();
        provider.install(runtime).unwrap();
        let program = load_empty(runtime, "parking-graph");
        let owner = runtime.select_program(&program).unwrap();
        let ty = runtime
            .bind_native_type::<Link>(&program.root(), &link, &[])
            .unwrap();
        Self {
            _program: program,
            provider,
            owner,
            ty,
        }
    }

    pub(crate) fn allocate(
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

    pub(crate) fn interface(&self, runtime: &mut Runtime, receiver: &RootedValue) -> RootedValue {
        let code = compile_with_provider(
            include_str!("../../tests/fixtures/metadata/host-interface-receiver.kgr"),
            Some(&self.provider),
        );
        let program = load_bytecode(runtime, "parking-receiver", code).unwrap();
        let owner = runtime.select_program(&program).unwrap();
        runtime.make_interface(&owner, 0, receiver).unwrap()
    }

    /// Mutate the actual private provider edge through a second genuine root.
    pub(crate) fn set_child(
        &self,
        runtime: &mut Runtime,
        value: &RootedValue,
        child: &RootedValue,
    ) {
        runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let value = value.get(cx)?;
                let header = cx.downcast::<NativeObject>(value.object().unwrap())?;
                let payload = NativeObject::payload(cx, header);
                let link = cx.downcast::<Link>(payload)?;
                let child = child.get(cx)?.object();
                Link::NEXT.set(cx, link, child);
                Ok(())
            })
            .unwrap();
    }
}
