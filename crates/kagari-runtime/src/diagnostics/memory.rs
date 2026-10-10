//! Manual retained-memory probes: checked inputs, no script execution or VM frames.
mod applications;
mod layouts;

use crate::{
    Runtime,
    diagnostics::allocations::{self, Counts},
    execution_metadata::MetadataRoot,
    frame::types::EnvironmentRecord,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        declarations::FunctionDecl,
        module::NativeModule,
        types::Type,
    },
    value::Value,
};
use kagari_bytecode::{instruction::NativeImportId, program::BytecodeProgram};
use kagari_common::identity::table::DefinitionId;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::Arc;

fn compile(source: &str, provider: &NativeModule) -> BytecodeProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("memory.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![Arc::new(provider.to_declaration().unwrap())]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    lower_program_to_bytecode(&mir).unwrap()
}

fn provider() -> NativeModule {
    let mut builder = ModuleBuilder::new("test::memory", &DeclarationCatalog::default());
    let read = builder
        .define_function(FunctionDecl::new("read").returns(Type::i32()))
        .unwrap();
    builder
        .bind_with(
            read,
            NativeBinding::new(vec![], Codec::Value, |_| Ok(Value::I32(3))),
        )
        .unwrap();
    let pass = builder.define_function(FunctionDecl::new("pass")).unwrap();
    builder
        .function(&pass, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("value", item.clone());
            function.returns(item);
            Ok(())
        })
        .unwrap();
    builder
        .bind_with(
            pass,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |cx| cx.argument(0)),
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

fn preparation(runtime: &Runtime, mut prepare: impl FnMut()) -> (Counts, Counts, Counts, Counts) {
    runtime.collect_garbage().unwrap();
    let setup = snapshot();
    prepare();
    runtime.collect_garbage().unwrap();
    let cold = snapshot();
    for _ in 0..2 {
        prepare();
        runtime.collect_garbage().unwrap();
    }
    let warm = snapshot();
    for _ in 0..30 {
        prepare();
        runtime.collect_garbage().unwrap();
    }
    (setup, cold, warm, snapshot())
}

fn native_lifecycle(
    provider: &NativeModule,
    code: &BytecodeProgram,
    applications: usize,
    prepare: bool,
) -> (Phases, Counts) {
    allocations::measure(|| {
        let mut runtime = Runtime::default();
        provider.install(&mut runtime).unwrap();
        let loaded = runtime.load_program("memory", code.clone()).unwrap();
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
        let environments = (0..applications)
            .map(|index| {
                let ty: Ty<DefinitionId> =
                    Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); index + 1]);
                let arguments = runtime.resolve_type_arguments(&owner, &[ty]).unwrap();
                runtime
                    .gc
                    .alloc_environment(
                        EnvironmentRecord::new(
                            runtime.definition_context(),
                            body.parameters.clone(),
                            arguments,
                        )
                        .unwrap(),
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let roots = runtime
            .root_metadata(
                environments
                    .iter()
                    .map(|environment| MetadataRoot::Environment(environment.id))
                    .collect(),
            )
            .unwrap();
        runtime.collect_garbage().unwrap();
        let setup = snapshot();
        let prepare_all = || {
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
                    runtime
                        .prepare_native_application(
                            &owner,
                            NativeImportId::new(generic),
                            environment.clone(),
                        )
                        .unwrap();
                }
            }
            runtime.collect_garbage().unwrap();
        };
        prepare_all();
        let cold = snapshot();
        prepare_all();
        prepare_all();
        let warm = snapshot();
        for _ in 0..30 {
            prepare_all();
        }
        let repeated = snapshot();
        let staged = runtime
            .stage_reload_program(&loaded, "memory", code.clone())
            .unwrap();
        drop(runtime.publish_staged_reload(staged).unwrap());
        drop(roots);
        drop(environments);
        drop(owner);
        drop(loaded);
        runtime.collect_garbage().unwrap();
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
        &provider,
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
