//! Host retention and complete collection timing for an explicit array graph.
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_contract::updates::UpdateEvidence;
use kagari_runtime::Runtime;
use kagari_runtime::module::VerifiedProgram;
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn main() {
    const LEAVES: usize = 10_000;
    const OBJECTS: usize = LEAVES + 1;
    println!("repeat,objects,live_collect_ns,dead_collect_ns,retired_owners,freed_provider_units");
    for repeat in 0..5 {
        let mut runtime = Runtime::default();
        let context = DefinitionContext::new().unwrap();
        let owner_program = runtime
            .load_verified_program(
                "arrays",
                VerifiedProgram::new(
                    DefinitionMetadata::checked(
                        context.snapshot(),
                        BytecodeProgram {
                            // This owner has no declarations or executable bodies.
                            update_evidence: UpdateEvidence {
                                source_manifest: None,
                                sources: vec![],
                                bodies: vec![],
                                initializers: vec![],
                                targets: vec![],
                                selections: vec![],
                            },
                            exports: Default::default(),
                            root: ModuleRef::new(0),
                            modules: vec![BytecodeModule::default()],
                        },
                        &Default::default(),
                    )
                    .unwrap(),
                    &context,
                )
                .unwrap(),
            )
            .unwrap();
        let owner = runtime.select_program(&owner_program).unwrap();
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        // Host allocations return genuine roots. After parent construction only
        // its root retains the complete graph; child Host leases are released.
        let element = Ty::Builtin(BuiltinType::I32);
        let mut leaves = Vec::with_capacity(LEAVES);
        for _ in 0..LEAVES {
            leaves.push(
                runtime
                    .alloc_array(&owner, element.clone(), &[1i32.into()])
                    .unwrap(),
            );
        }
        let retained = runtime
            .alloc_array(&owner, Ty::Array(Box::new(element)), &leaves)
            .unwrap();
        drop(leaves);
        let live = runtime.collect_garbage().unwrap();
        let live_stats = runtime.gc_stats();
        assert_eq!(live_stats.objects, baseline.objects + OBJECTS);
        assert_eq!(
            live_stats.root_registrations,
            baseline.root_registrations + 1
        );
        drop(retained);
        let dead = runtime.collect_garbage().unwrap();
        let dead_stats = runtime.gc_stats();
        assert_eq!(dead.retired_owners, OBJECTS);
        assert_eq!(dead_stats.objects, baseline.objects);
        assert_eq!(dead_stats.root_registrations, baseline.root_registrations);
        // Times include complete Runtime collection and metadata maintenance.
        // They are demo observations, not matched performance acceptance.
        println!(
            "{repeat},{OBJECTS},{},{},{},{}",
            live.elapsed.as_nanos(),
            dead.elapsed.as_nanos(),
            dead.retired_owners,
            live_stats.provider_units - dead_stats.provider_units,
        );
    }
}
