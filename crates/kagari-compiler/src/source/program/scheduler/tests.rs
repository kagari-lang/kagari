//! Program closure tables do not introduce module-local layout obligations.

use crate::source::{
    lower::{MirLoweringError, instances::MirLoweringOptions},
    program::{lower_program_to_mir, scheduler::Scheduler, update_requests::RequestQueue},
};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_contract::updates::DemandRequests;
use kagari_hir::{analysis::AnalysisDatabase, program::CheckedProgram};
use kagari_source::{
    diagnostic::DiagnosticKind,
    identity::FileId,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog;
use kagari_types::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

fn insert(sources: &mut SourceDatabase, name: &str, text: &str) -> FileId {
    let path = format!("mem://{name}");
    sources
        .bind_module(
            &path,
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec![name.into()],
            },
        )
        .unwrap();
    sources.set(&path, text.into(), SourceLayer::Base).unwrap()
}

fn shared_tuple_bridge() -> CheckedProgram {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "provider",
        r#"
        use std::hash::Hash;
        pub trait Erase {
            fn erase<T: Hash>(self, value: T) -> Hash { value }
        }
        pub trait Wrap {
            fn bridge<T: Hash>(self, eraser: Erase, value: T) -> Hash {
                eraser.erase((value, true))
            }
        }
        struct Source {}
        impl Erase for Source {}
        impl Wrap for Source {}
        pub fn source() -> Wrap { Source {} }
        pub fn eraser() -> Erase { Source {} }
        "#,
    );
    let root = insert(
        &mut sources,
        "root",
        r#"
        use pkg::provider;
        use std::hash::Hash;
        struct Key<T> { val value: T }
        impl<T: Eq + Hash> PartialEq for Key<T> {
            fn eq(self, other: Self) -> bool { self.value == other.value }
        }
        impl<T: Eq + Hash> Eq for Key<T> {}
        impl<T: Eq + Hash> Hash for Key<T> {
            fn hash(self) -> i64 { self.value.hash() }
        }
        fn main() { val view = provider::source().bridge(provider::eraser(), Key { value: 7 }); }
        "#,
    );
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(catalog::shared().unwrap());
    analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap()
}

#[test]
fn closed_tuple_bridge_does_not_root_caller_private_layout_in_provider() {
    let checked = shared_tuple_bridge();
    lower_program_to_mir(&checked, &Default::default()).unwrap();

    let context = checked.root().definition_context();
    let root_identity = checked.root().lowered.source.module_identity();
    let modules = checked
        .modules()
        .iter()
        .map(|module| {
            module
                .to_unverified(&Default::default())
                .unwrap()
                .into_records()
        })
        .collect::<Vec<_>>();
    let root_index = modules
        .iter()
        .position(|module| module.lowered.source.module_identity() == root_identity)
        .unwrap();
    let provider_index = modules
        .iter()
        .position(|module| module.lowered.source.module_identity().path == ["provider"])
        .unwrap();
    let catalog = &modules[root_index].aggregates;
    let options = MirLoweringOptions::default();
    let mut scheduler = Scheduler::new(
        &modules,
        catalog,
        &context,
        root_identity,
        &options,
        RequestQueue::default(),
    );
    scheduler.process_pending().unwrap();
    let before = scheduler.states[provider_index].usage;
    scheduler
        .discovery
        .scan(
            &scheduler.states,
            &modules,
            &context.snapshot(),
            &mut scheduler.requests,
            root_identity,
            &options,
        )
        .unwrap();
    scheduler.close().unwrap();
    let root = scheduler.states[root_index].state.products();
    let provider = scheduler.states[provider_index].state.products();
    let key = root
        .structures
        .iter()
        .find(|layout| {
            layout.arguments == [Ty::Builtin(BuiltinType::I32)]
                && context
                    .snapshot()
                    .resolve(layout.declaration)
                    .unwrap()
                    .last()
                    .is_some_and(|part| part.name == "Key")
        })
        .expect("the caller still owns its concrete private layout");

    assert!(
        provider.interface_instances.iter().any(|instance| {
            instance.arguments.iter().any(|argument| {
                matches!(argument, Ty::Struct(nominal)
                if nominal.declaration == key.declaration && nominal.arguments == key.arguments)
            })
        }),
        "the provider must retain the closed tuple table"
    );
    assert!(
        !provider.structures.iter().any(|layout| {
            layout.declaration == key.declaration && layout.arguments == key.arguments
        }),
        "closing a shared bridge table must not add a provider layout or its generic charge",
    );
    let after = scheduler.states[provider_index].usage;
    assert_eq!(after.generic_admissions, before.generic_admissions);
    assert_eq!(after.generic_products, before.generic_products + 1);
    assert_eq!(
        after.deferred_generic_products,
        before.deferred_generic_products + 1
    );
    let key = Ty::Struct(NominalTy {
        declaration: key.declaration,
        arguments: key.arguments.clone(),
        associated_types: Default::default(),
    });
    let mut available = options.clone();
    available.max_generic_instances = before.generic_admissions;
    let provider = &mut scheduler.states[provider_index].state;
    provider.check_budget(&available).unwrap();
    // A real, later layout demand must still be admitted and charged normally.
    let requests = DemandRequests {
        layouts: vec![key],
        ..Default::default()
    };
    let usage = provider
        .process(
            &requests,
            &scheduler.inputs,
            &options,
            &mut scheduler.bridges,
        )
        .unwrap();
    assert_eq!(usage.generic_admissions, after.generic_admissions + 1);
    assert_eq!(usage.generic_products, after.generic_products + 1);
    assert_eq!(
        usage.deferred_generic_products,
        after.deferred_generic_products
    );
    assert!(matches!(provider.check_budget(&available),
        Err(MirLoweringError::Diagnostic(diagnostic))
            if matches!(diagnostic.kind, DiagnosticKind::CompileLimitExceeded {
                resource: "generic instances", limit
            } if limit == before.generic_admissions)
    ));
}
