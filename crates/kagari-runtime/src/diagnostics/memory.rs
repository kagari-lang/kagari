//! Manual retained-memory probes: checked inputs, no script execution or VM frames.
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
mod applications;
mod execution;
mod layouts;

use crate::{
    Runtime,
    diagnostics::allocations::{self, Counts},
    execution::roots::{ExecutionRoots, SessionId},
    execution_metadata::managed::sources::MetadataSource,
    frame::types::EnvironmentRecord,
    module::managed::source::RevisionSource,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        declarations::FunctionDecl,
        module::NativeModule,
        types::Type,
    },
    value::view::ValueView,
};
use kagari_bytecode::{instruction::NativeImportId, program::BytecodeProgram};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_gc::heap::HeapStats;
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::Arc;

fn compile(source: &str, provider: Option<&NativeModule>) -> DefinitionMetadata<BytecodeProgram> {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("memory.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    if let Some(provider) = provider {
        analysis.set_native_modules(vec![Arc::new(provider.to_declaration())]);
    }
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
}

fn provider() -> NativeModule {
    let mut builder = ModuleBuilder::new("test::memory", &DeclarationCatalog::default()).unwrap();
    let read = builder
        .define_function(FunctionDecl::new("read").returns(Type::i32()))
        .unwrap();
    builder
        .bind(
            read,
            NativeBinding::scoped(vec![], Codec::Exact(Type::i32()), |call| {
                call.set_result(ValueView::from(3_i32))
            }),
        )
        .unwrap();
    let pass = builder.define_function(FunctionDecl::new("pass")).unwrap();
    builder
        .function(&pass, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("value", item.clone())?;
            function.returns(item)?;
            Ok(())
        })
        .unwrap();
    builder
        .bind(
            pass,
            NativeBinding::scoped(vec![Codec::Value], Codec::Value, |call| {
                let value = call.argument(0)?;
                call.set_result(value)
            }),
        )
        .unwrap();
    builder.finish().unwrap()
}

#[derive(Debug)]
struct Phases {
    setup: Counts,
    cold: Counts,
    warm: Counts,
    repeated: Counts,
    retired: Counts,
}

fn snapshot() -> Counts {
    allocations::current().unwrap()
}

fn compare(label: &str, mut lifecycle: impl FnMut(bool) -> (Phases, Counts)) {
    lifecycle(true);
    let (control, control_total) = lifecycle(false);
    let (prepared, prepared_total) = lifecycle(true);
    println!(
        "{label} control={control:?} prepared={prepared:?} control_total={control_total:?} prepared_total={prepared_total:?}"
    );
    println!(
        "{label} extra_net_bytes setup={} cold={} warm={} repeated={} retired={} teardown={}",
        prepared.setup.net_bytes - control.setup.net_bytes,
        prepared.cold.net_bytes - control.cold.net_bytes,
        prepared.warm.net_bytes - control.warm.net_bytes,
        prepared.repeated.net_bytes - control.repeated.net_bytes,
        prepared.retired.net_bytes - control.retired.net_bytes,
        prepared_total.net_bytes - control_total.net_bytes
    );
    assert_eq!(control_total.net_bytes, 0, "control runtime teardown");
    assert_eq!(prepared_total.net_bytes, 0, "prepared runtime teardown");
}

fn preparation(
    runtime: &mut Runtime,
    mut prepare: impl FnMut(&mut Runtime),
) -> (Counts, Counts, Counts, Counts) {
    runtime.collect_garbage().unwrap();
    let setup = snapshot();
    prepare(runtime);
    runtime.collect_garbage().unwrap();
    let cold = snapshot();
    for _ in 0..2 {
        prepare(runtime);
        runtime.collect_garbage().unwrap();
    }
    let warm = snapshot();
    for _ in 0..30 {
        prepare(runtime);
        runtime.collect_garbage().unwrap();
    }
    (setup, cold, warm, snapshot())
}

/// The caller retains cold prepared products while each repeated request owns
/// only a temporary suffix. No detached metadata ID or per-copy Host root pack
/// can retain or reopen these actual providers.
struct PreparationSource {
    execution: SessionId,
    prepared: bool,
}

impl PreparationSource {
    fn new(runtime: &mut Runtime) -> Self {
        Self {
            execution: runtime.memory.enter_execution(&runtime.resources).unwrap(),
            prepared: false,
        }
    }

    fn run(&mut self, runtime: &mut Runtime, prepare: impl FnOnce(&mut Runtime, SessionId, bool)) {
        let mark = runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| slots.mark_locals(self.execution))
            })
            .unwrap();
        prepare(runtime, self.execution, !self.prepared);
        if self.prepared {
            runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| slots.retire_locals(mark))
                })
                .unwrap();
        }
        self.prepared = true;
    }

    fn retire(self, runtime: &mut Runtime) {
        runtime
            .memory
            .retire_execution(&runtime.resources, self.execution)
            .unwrap();
    }
}

fn assert_reclaimed(runtime: &mut Runtime, baseline: HeapStats) {
    runtime.collect_garbage().unwrap();
    let after = runtime.gc_stats();
    assert_eq!(after.objects, baseline.objects);
    assert_eq!(after.provider_units, baseline.provider_units);
    assert_eq!(after.root_registrations, baseline.root_registrations);
}

fn native_lifecycle(
    provider: &NativeModule,
    code: &DefinitionMetadata<BytecodeProgram>,
    applications: usize,
    prepare: bool,
) -> (Phases, Counts) {
    allocations::measure(|| {
        let mut runtime = Runtime::default();
        provider.install(&mut runtime).unwrap();
        let program = load_bytecode(&mut runtime, "memory", code.clone()).unwrap();
        let loaded = runtime.select_program(&program).unwrap();
        let owner = loaded
            .members()
            .find(|member| !member.bytecode.native_imports.is_empty())
            .unwrap();
        let generic = owner
            .bytecode
            .native_imports
            .iter()
            .position(|import| import.generic.is_some())
            .unwrap();
        let closed = owner
            .bytecode
            .native_imports
            .iter()
            .position(|import| import.generic.is_none())
            .unwrap();
        let body = owner.bytecode.native_imports[generic]
            .generic
            .as_ref()
            .unwrap();
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        let input_source = PreparationSource::new(&mut runtime);
        let mut prepared_source = PreparationSource::new(&mut runtime);
        let environments = (0..applications)
            .map(|index| {
                let ty: Ty = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); index + 1]);
                let arguments = runtime.resolve_type_arguments(&owner, &[ty]).unwrap();
                runtime
                    .alloc_environment(
                        input_source.execution,
                        EnvironmentRecord::new(
                            &runtime.definition_context().snapshot(),
                            body.parameters.clone(),
                            arguments,
                        )
                        .unwrap(),
                        &[MetadataSource::Revision {
                            owner: &owner,
                            source: RevisionSource::Current,
                        }],
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let (setup, cold, warm, repeated) = preparation(&mut runtime, |runtime| {
            prepared_source.run(runtime, |runtime, execution, _first| {
                if prepare {
                    if applications == 0 {
                        runtime
                            .modules
                            .native_binding(&owner, NativeImportId::new(closed))
                            .unwrap()
                            .type_signature()
                            .unwrap();
                    }
                    for environment in &environments {
                        let mut protected = vec![
                            MetadataSource::Revision {
                                owner: &owner,
                                source: RevisionSource::Current,
                            },
                            MetadataSource::Environment(environment.source()),
                        ];
                        runtime
                            .prepare_native_application(
                                execution,
                                &owner,
                                NativeImportId::new(generic),
                                environment.facts().clone(),
                                &mut protected,
                            )
                            .unwrap();
                    }
                }
            });
        });
        let staged = runtime
            .stage_reload_program(&program, None, code.clone())
            .unwrap();
        runtime.publish_staged_reload(staged).unwrap();
        prepared_source.retire(&mut runtime);
        input_source.retire(&mut runtime);
        drop(environments);
        drop(owner);
        drop(loaded);
        assert_reclaimed(&mut runtime, baseline);
        let retired = snapshot();
        // Runtime and all measured inputs drop before the allocator's final snapshot.
        Phases {
            setup,
            cold,
            warm,
            repeated,
            retired,
        }
    })
}

#[test]
#[ignore = "manual descriptor allocation accounting; run release with --nocapture"]
fn native_descriptor_retention() {
    let provider = provider();
    let code = compile(
        r#"
        use test::memory::{read, pass};
        trait Forward { fn forward<T>(self, value: T) -> T { pass(value) } }
        impl Forward for i32 {}
        fn main() -> i32 { val f: Forward = 0; f.forward(read()) }
        "#,
        Some(&provider),
    );
    // Initialize process/thread state before either side of the comparison.
    native_lifecycle(&provider, &code, 1, true);
    for applications in [0, 1, 4, 160] {
        let (control, control_total) = native_lifecycle(&provider, &code, applications, false);
        let (prepared, prepared_total) = native_lifecycle(&provider, &code, applications, true);
        println!(
            "native applications={applications} control={control:?} prepared={prepared:?} control_total={control_total:?} prepared_total={prepared_total:?}"
        );
        println!(
            "native applications={applications} extra_net_bytes setup={} cold={} warm={} repeated={} retired={} teardown={}",
            prepared.setup.net_bytes - control.setup.net_bytes,
            prepared.cold.net_bytes - control.cold.net_bytes,
            prepared.warm.net_bytes - control.warm.net_bytes,
            prepared.repeated.net_bytes - control.repeated.net_bytes,
            prepared.retired.net_bytes - control.retired.net_bytes,
            prepared_total.net_bytes - control_total.net_bytes
        );
        assert_eq!(control_total.net_bytes, 0, "control runtime teardown");
        assert_eq!(prepared_total.net_bytes, 0, "prepared runtime teardown");
    }
}
