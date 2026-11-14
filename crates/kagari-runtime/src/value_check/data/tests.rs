use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind, trap::TrapReason},
    execution::roots::ExecutionRoots,
    frame::ExecutionStack,
    host::{HostObjectId, HostSchemaEpoch},
    module::VerifiedProgram,
    native::builder::ModuleBuilder,
    resource::candidate::CandidateOrigin,
    test_support::update_programs::load_empty,
    value::{
        aggregates::tuples::TupleObject, carriers::ScopedValue, cells::CellObject,
        ephemeral::EphemeralValueId, host_values::HostRootObject, strings::StringObject,
    },
    value_check::data::{AsyncDataWalk, DataWalk, async_scoped},
};
use kagari_abi::representation::ValueType;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_gc::{
    access::{EdgeField, Scope},
    error::GcError,
    object::{AnyObject, Edge, Gc, GcObject},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::declaration::host::{
    HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
};
use std::sync::Arc;

/// An unknown physical payload can hide descendants without a registered schema.
/// This is the data-walk owner, not a substitute Native invocation harness.
struct PrivateLink {
    next: Option<Edge<AnyObject>>,
}

impl PrivateLink {
    // SAFETY: one optional field; checked mutation publishes before replacement.
    const NEXT: EdgeField<Self, AnyObject> =
        unsafe { EdgeField::new(|link| &link.next, |link| &mut link.next) };

    fn alloc<'gc>(cx: &mut Scope<'gc>) -> Result<Gc<'gc, Self>, GcError> {
        // SAFETY: initially empty; the scanner owns the only possible edge.
        unsafe { cx.alloc_with(|_| Ok(Self { next: None })) }
    }
}

// SAFETY: one fixed field, no hidden edges/backing/interior mutation. The scanner
// preserves its coordinate on Pending; its state and destructor read no peers.
unsafe impl GcObject for PrivateLink {
    type ScanState = bool;
    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) -> bool {
        false
    }

    fn scan(
        &self,
        visited: &mut bool,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !*visited {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            if let Some(edge) = &self.next {
                // SAFETY: the admitted object owns this initialized edge.
                if unsafe { visitor.try_visit_stored(edge, budget)? } == ScanStatus::Pending {
                    return Ok(ScanStatus::Pending);
                }
            }
            *visited = true;
        }
        Ok(ScanStatus::Complete)
    }
}

#[test]
fn async_data_walk_rechecks_mutable_private_cycles_and_hidden_capabilities() {
    let mut runtime = Runtime::default();
    let mut module = ModuleBuilder::new("graph", &Default::default()).unwrap();
    let mut external = module.define_type("External").unwrap();
    external
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadOnly,
        })
        .unwrap();
    external.finish().unwrap();
    module.finish().unwrap().install(&mut runtime).unwrap();
    let ty = runtime.types().get_by_name("graph::External").unwrap().id;
    let handle = runtime
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let Runtime {
        memory, resources, ..
    } = &mut runtime;
    memory
        .scope(resources, |cx, _| {
            let text = StringObject::alloc(cx, "private data".into())?;
            let alias = PrivateLink::alloc(cx)?;
            let payload = PrivateLink::alloc(cx)?;
            PrivateLink::NEXT.set(cx, payload, Some(alias.erase()));
            PrivateLink::NEXT.set(cx, alias, Some(text.erase()));
            assert!(
                DataWalk::default()
                    .object(cx, payload.erase(), resources)
                    .unwrap()
            );
            PrivateLink::NEXT.set(cx, alias, Some(alias.erase()));
            assert!(
                DataWalk::default()
                    .object(cx, payload.erase(), resources)
                    .unwrap()
            );
            let host = HostRootObject::alloc(cx, handle)?;
            PrivateLink::NEXT.set(cx, alias, Some(host.erase()));
            assert!(
                !DataWalk::default()
                    .object(cx, payload.erase(), resources)
                    .unwrap()
            );
            let ephemeral = ScopedValue::runtime_ephemeral(EphemeralValueId(1));
            let tuple = TupleObject::alloc(cx, &[ephemeral])?;
            assert!(!async_scoped(cx, &[ScopedValue::managed(tuple)], resources).unwrap());
            PrivateLink::NEXT.set(cx, alias, Some(tuple.erase()));
            assert!(
                !DataWalk::default()
                    .object(cx, payload.erase(), resources)
                    .unwrap()
            );
            PrivateLink::NEXT.set(cx, alias, Some(text.erase()));
            assert!(
                DataWalk::default()
                    .object(cx, payload.erase(), resources)
                    .unwrap()
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 0);
}

#[test]
fn async_execution_checks_receiver_data_owned_only_by_an_admitted_computation() {
    let mut builder = ModuleBuilder::new("graph", &Default::default()).unwrap();
    let mut external = builder.define_type("External").unwrap();
    external
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadOnly,
        })
        .unwrap();
    external.finish().unwrap();
    let provider = builder.finish().unwrap();
    let mut sources = SourceDatabase::default();
    let source = sources
        .set(
            "host-receiver.kgr",
            include_str!("../../../tests/fixtures/metadata/host-interface-receiver.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![Arc::new(provider.to_declaration())]);
    let checked = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(source, &Default::default())
        .unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let program = VerifiedProgram::from_bytecode(
        lower_program_to_bytecode(&mir, &Default::default()).unwrap(),
    );
    let mut runtime = Runtime::default();
    provider.install(&mut runtime).unwrap();
    let installed = runtime
        .load_verified_program("host-receiver", program)
        .unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let ty = runtime.types().get_by_name("graph::External").unwrap().id;
    let handle = runtime
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    let value = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let object = HostRootObject::alloc(cx, handle)?;
            ScopedValue::managed(object).retain(cx)
        })
        .unwrap();
    // Normal boxing and admission establish the installed nominal relationship;
    // this negative input reaches data policy without corrupting a provider.
    let interface = runtime.make_interface(&owner, 0, &value).unwrap();
    let description = runtime.interface_description(&interface).unwrap();
    let method = runtime
        .resolve_interface_method_slot(&interface, description.interface_type(), 0, &[])
        .unwrap();
    let method = method.admit(&mut runtime).unwrap();
    let entry = owner
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap()
        .id;
    let execution_owner = runtime
        .start_owned_execution(&owner, entry, &[], Default::default())
        .unwrap();
    let execution = execution_owner.id;
    runtime
        .application_in(
            execution,
            method.invocation.computation().unwrap().id(),
            &method.sources().unwrap(),
        )
        .unwrap();
    drop((method, interface, value));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    let Runtime {
        memory, resources, ..
    } = &mut runtime;
    let error = memory
        .scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok(AsyncDataWalk::default().remaining(slots, execution, resources))
            })
        })
        .unwrap()
        .unwrap_err()
        .into_runtime(&runtime);
    assert_eq!(
        error,
        RuntimeError::from(TrapReason::AsyncStateRetainsAnInvalidOrScopedValue)
    );
    // A scalar frame alone cannot authorize data-free parking: its additional
    // application dependency is the only remaining owner of the Host receiver.
    let mut activation = Some(runtime.resume_owned_execution(&execution_owner).unwrap());
    let error = ExecutionStack::try_park_ready(&mut activation).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    assert!(error.to_string().contains("async state retains"));
    assert!(activation.is_some());
    drop(activation);
    runtime.finish_owned_execution(&execution_owner).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    assert!(!runtime.is_quarantined());
}

#[test]
fn candidate_walk_checks_birth_identity_and_rechecks_private_mutable_aliases() {
    let mut runtime = Runtime::default();
    let installed = load_empty(&mut runtime, "candidate-data");
    let owner = runtime.select_program(&installed).unwrap();
    let other_installed = load_empty(&mut runtime, "candidate-data");
    let other = runtime.select_program(&other_installed).unwrap();
    let origin = CandidateOrigin::new(owner.program_identity());
    let roots = runtime.gc_stats().root_registrations;
    let Runtime {
        memory, resources, ..
    } = &mut runtime;
    memory
        .scope(resources, |cx, _| {
            let text = StringObject::alloc(cx, "ordinary immutable input".into())?;
            let value = ScopedValue::managed(text);
            let candidate = CellObject::alloc(cx, ValueType::Generic, value, origin)?;
            let external = CellObject::alloc(cx, ValueType::Generic, value, Default::default())?;
            let alias = PrivateLink::alloc(cx)?;
            PrivateLink::NEXT.set(cx, alias, Some(candidate.erase()));
            assert!(
                DataWalk::candidate(owner.program_identity())
                    .object(cx, alias.erase(), resources)
                    .unwrap()
            );
            assert!(
                !DataWalk::candidate(other.program_identity())
                    .object(cx, alias.erase(), resources)
                    .unwrap()
            );
            PrivateLink::NEXT.set(cx, alias, Some(alias.erase()));
            assert!(
                DataWalk::candidate(owner.program_identity())
                    .object(cx, alias.erase(), resources)
                    .unwrap()
            );
            // A fresh traversal must observe an alias edit, even after a successful
            // cyclic walk. Async admission still permits this ordinary heap object.
            PrivateLink::NEXT.set(cx, alias, Some(external.erase()));
            assert!(
                !DataWalk::candidate(owner.program_identity())
                    .object(cx, alias.erase(), resources)
                    .unwrap()
            );
            assert!(
                DataWalk::default()
                    .object(cx, alias.erase(), resources)
                    .unwrap()
            );
            PrivateLink::NEXT.set(cx, alias, Some(candidate.erase()));
            assert!(
                DataWalk::candidate(owner.program_identity())
                    .object(cx, alias.erase(), resources)
                    .unwrap()
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
}
