use crate::{
    Runtime,
    error::RuntimeErrorKind,
    module::{ExecutableModule, VerifiedProgram, dispatch::CallRouteKind},
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        module::NativeModule,
        storage::NativeStorage,
        types::Type,
    },
    source::{
        FunctionSource, FunctionSourceTarget, ProgramSourceSnapshot, SourceDeclaration,
        SourceQuery, SourceUnavailableReason,
    },
    value::Value,
};
use kagari_bytecode::{
    artifact::{ArtifactCompatibility, KbcArtifact},
    module::CallableTarget,
    program::verified::VerifiedBytecodeProgram,
    source_package::VerifiedSourcePackage,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{map::DefinitionContext, scoped::DefinitionReferences},
};
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::{
        program::{lower_program_to_mir, lower_program_to_mir_with_demands},
        source_package::build_source_package,
    },
};
use kagari_contract::source::ExecutableOrigin;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use rstest::rstest;
use std::sync::Arc;

fn compiled(
    text: &str,
    baseline: Option<&ExecutableModule>,
) -> (VerifiedBytecodeProgram, VerifiedSourcePackage) {
    compiled_in(text, baseline, AnalysisDatabase::default())
}

fn compiled_in(
    text: &str,
    baseline: Option<&ExecutableModule>,
    mut analysis: AnalysisDatabase,
) -> (VerifiedBytecodeProgram, VerifiedSourcePackage) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("exact.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let checked = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mir = match baseline {
        Some(baseline) => {
            let demands = baseline.retained_demands();
            lower_program_to_mir_with_demands(
                &checked,
                &Default::default(),
                demands.metadata(),
                demands.implementation_headers(),
            )
        }
        None => lower_program_to_mir(&checked, &Default::default()),
    }
    .unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default()).unwrap();
    let package = build_source_package(&checked, &bytecode, &Default::default()).unwrap();
    (bytecode, package)
}

fn prepared(text: &str, baseline: Option<&ExecutableModule>) -> VerifiedProgram {
    let (bytecode, package) = compiled(text, baseline);
    VerifiedProgram::from_bytecode(bytecode)
        .with_source_package(package, &Default::default())
        .unwrap()
}

fn declaration(snapshot: &ProgramSourceSnapshot, name: &str) -> SourceDeclaration {
    snapshot
        .declarations()
        .iter()
        .find(|declaration| declaration.name() == name)
        .unwrap()
        .clone()
}

fn available(query: SourceQuery) -> FunctionSource {
    match query {
        SourceQuery::Available(source) => source,
        SourceQuery::Unavailable(reason) => panic!("expected source: {reason:?}"),
    }
}

#[test]
fn source_only_inventory_imports_without_reverification_and_honors_cancellation() {
    let text = "mod spare { struct Ghost {} fn unused<T>(x: T) -> T { x } } pub fn value() -> i32 { 1 } fn main() {}";
    let (bytecode, package) = compiled(text, None);
    let product = bytecode.product_identity();
    let code = VerifiedProgram::from_bytecode(bytecode);
    let cancel = CancellationToken::default();
    let attached = code
        .clone()
        .with_source_package(package.clone(), &cancel)
        .unwrap();
    assert!(code.same_version(&attached));
    assert!(
        code.modules()
            .iter()
            .zip(attached.modules())
            .all(|(a, b)| Arc::ptr_eq(a, b))
    );
    let context = DefinitionContext::new().unwrap();
    let imported = attached.normalized(&context, &cancel).unwrap();
    assert!(attached.same_version(&imported));
    let imported_package = imported.source_package().unwrap();
    imported_package
        .records()
        .visit_definition_ids(
            &mut |id| {
                assert_eq!(id.table(), imported.definitions().id());
                Ok(())
            },
            &cancel,
        )
        .unwrap();
    assert!(
        package
            .for_product(
                product,
                &DefinitionContext::new().unwrap().snapshot(),
                &cancel
            )
            .is_err()
    );
    let rebound = imported
        .clone()
        .with_source_package(package.clone(), &cancel)
        .unwrap();
    assert!(rebound.same_version(&attached));
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("inventory", imported)
        .unwrap();
    let snapshot = runtime.source_snapshot(&program).unwrap();
    assert_eq!(
        snapshot.declarations().len(),
        package.records().declarations.len()
    );
    for locator in snapshot.declarations() {
        assert!(matches!(
            snapshot.function(locator),
            SourceQuery::Available(_)
        ));
        runtime
            .resolve_source_declaration(&program, locator)
            .unwrap();
    }
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert_eq!(
        code.with_source_package(package.clone(), &cancelled)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(
        attached
            .normalized(&context, &cancelled)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(
        rebound
            .clone()
            .normalized(&context, &cancelled)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(
        rebound
            .with_source_package(package, &cancelled)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::Cancelled
    );
}

#[test]
fn current_routes_exact_values_frames_and_source_only_history() {
    let text = "pub fn value() -> i32 { 1 }\r\npub fn generated() -> fn() -> i32 { || 10 }\r\nfn private() -> i32 { 3 }\r\nfn main() {}";
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("exact", prepared(text, None))
        .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let old_key = old.key();
    let history = runtime.source_snapshot(&program).unwrap();
    let private = declaration(&history, "private");
    assert_eq!(
        available(history.function(&private)).text,
        "fn private() -> i32 { 3 }"
    );
    assert!(
        runtime
            .bind_function::<(), i32>(&program.root(), "private")
            .is_err()
    );
    let ordinary = runtime
        .bind_function::<(), i32>(&program.root(), "value")
        .unwrap()
        .source_target();
    let generated = old
        .bytecode
        .functions
        .iter()
        .find(|function| {
            old.bind_call_route(CallableTarget::Script(function.id))
                .unwrap()
                .kind()
                == CallRouteKind::BodyOwned
        })
        .unwrap();
    let root = runtime
        .root_value(
            runtime
                .make_closure(&old, generated.id, vec![], None)
                .unwrap(),
        )
        .unwrap();
    let exact = runtime.value_source_target(&root).unwrap();
    let frame_target = {
        let stack = runtime.enter_execution_stack(&old).unwrap();
        stack
            .push_closure(&runtime, &root.value(runtime.gc()).unwrap(), &[], None)
            .unwrap();
        let frame = stack.current().unwrap();
        assert!(Runtime::default().frame_source_target(&frame).is_err());
        runtime.frame_source_target(&frame).unwrap()
    };
    let next = format!(
        "// moved unchanged definitions\r\n{}",
        text.replace("{ 1 }", "{ 2 }").replace("|| 10", "|| 20")
    );
    let staged = runtime
        .stage_reload_verified_program(&program, None, prepared(&next, Some(&old)))
        .unwrap();
    let next_publication = staged.publication_token();
    assert_eq!(
        available(runtime.function_source(&ordinary).unwrap()).publication,
        history.publication()
    );
    runtime.publish_staged_reload(staged).unwrap();
    let current = runtime.source_snapshot(&program).unwrap();
    let current_private = available(current.function(&private));
    assert_eq!(current_private.last_source_change, history.publication());
    assert!(current_private.range.start > available(history.function(&private)).range.start);
    assert_eq!(
        runtime
            .resolve_source_declaration(&program, &private)
            .unwrap()
            .range,
        current_private.range
    );
    let changed = available(runtime.function_source(&ordinary).unwrap());
    assert_eq!(changed.publication, next_publication);
    assert_eq!(changed.last_source_change, next_publication);
    assert!(changed.text.ends_with("{ 2 }"));
    for target in [&exact, &frame_target] {
        let source = available(runtime.function_source(target).unwrap());
        assert_eq!(source.publication, history.publication());
        assert!(source.text.contains("|| 10"));
        assert!(source.executable.is_some());
        assert!(Runtime::default().function_source(target).is_err());
    }
    drop(root);
    drop(old);
    runtime.collect_garbage().unwrap();
    assert!(
        runtime.modules.loaded(old_key).is_none(),
        "diagnostics must not retain executable admission"
    );
    assert!(
        available(runtime.function_source(&exact).unwrap())
            .text
            .contains("|| 10")
    );
    assert_eq!(
        available(history.function(&declaration(&history, "value"))).text,
        "pub fn value() -> i32 { 1 }"
    );
}

#[rstest]
#[case::stripped(false)]
#[case::retained(true)]
fn source_free_packages_stripping_mismatch_and_locator_authority(#[case] retained: bool) {
    let text = "pub fn value() -> i32 { 1 } fn main() {}";
    let (bytecode, package) = compiled(text, None);
    let artifact =
        KbcArtifact::from_verified(bytecode, Default::default(), &Default::default()).unwrap();
    let artifact = if retained {
        KbcArtifact::with_source_package(artifact, &package, &Default::default()).unwrap()
    } else {
        artifact
    };
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let verified = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records()
        .into_verified(
            &ArtifactCompatibility::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
    let (bytecode, _, source) = verified.into_parts();
    let code = VerifiedProgram::from_bytecode(bytecode);
    let code = match source {
        Some(source) => code
            .with_source_package(source, &Default::default())
            .unwrap(),
        None => code,
    };
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("source-free", code.clone())
        .unwrap();
    let other = runtime.load_verified_program("other", code).unwrap();
    let snapshot = runtime.source_snapshot(&program).unwrap();
    let locator = declaration(&snapshot, "value");
    let query = runtime
        .function_source(
            &runtime
                .bind_function::<(), i32>(&program.root(), "value")
                .unwrap()
                .source_target(),
        )
        .unwrap();
    if retained {
        assert!(available(query).text.contains("{ 1 }"));
        assert!(
            runtime
                .resolve_source_declaration(&program, &locator)
                .is_ok()
        );
    } else {
        assert!(
            matches!(query, SourceQuery::Unavailable(reason) if reason.reason == SourceUnavailableReason::MissingPackage && reason.application.is_some())
        );
    }
    assert!(
        runtime
            .resolve_source_declaration(&other, &locator)
            .is_err()
    );
    assert!(
        matches!(runtime.source_snapshot(&other).unwrap().function(&locator), SourceQuery::Unavailable(reason) if reason.reason == SourceUnavailableReason::Mismatch)
    );
}

fn future_provider() -> NativeModule {
    let mut builder = ModuleBuilder::new("fixture::source", &Default::default()).unwrap();
    let mut future = builder.define_type("Future").unwrap();
    future.type_parameter("T").unwrap();
    future.native_storage(NativeStorage::future()).unwrap();
    let future = future.finish().unwrap();
    let output = future.apply([Type::i32()]).unwrap();
    let ready = builder
        .define_function(FunctionDecl::new("ready").returns(output.clone()))
        .unwrap();
    builder
        .bind(
            ready,
            NativeBinding::new(vec![], Codec::Exact(output), |_| {
                panic!("source inspection never executes a native entry")
            }),
        )
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn native_origins_never_fabricate_text() {
    let provider = future_provider();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![Arc::new(provider.to_declaration())]);
    let (bytecode, package) = compiled_in(
        "use fixture::source::ready; fn main() { ready(); }",
        None,
        analysis,
    );
    let mut runtime = Runtime::default();
    provider.install(&mut runtime).unwrap();
    let program = runtime
        .load_verified_program(
            "native",
            VerifiedProgram::from_bytecode(bytecode)
                .with_source_package(package, &Default::default())
                .unwrap(),
        )
        .unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let mut native = false;

    for member in owner.members() {
        let targets = member
            .bytecode
            .functions
            .iter()
            .map(|function| CallableTarget::Script(function.id))
            .chain((0..member.bytecode.native_imports.len()).map(|index| {
                CallableTarget::Native(kagari_bytecode::instruction::NativeImportId::new(index))
            }));
        for target in targets {
            let query = runtime
                .function_source(&FunctionSourceTarget::exact(&member, target))
                .unwrap();
            if let SourceQuery::Unavailable(source) = query
                && source.reason == SourceUnavailableReason::NativeOrigin
            {
                let association = source.executable.unwrap();
                assert_eq!(source.publication, Some(owner.publication_token()));
                assert!(source.target.is_some());
                match association.origin {
                    ExecutableOrigin::Native => {
                        native = true;
                        assert!(source.native_binding.is_some());
                    }

                    origin => panic!("wrong unavailable origin: {origin:?}"),
                }
            }
        }
    }
    assert!(native);
}

#[test]
fn cold_future_observation_is_exact_and_does_not_claim_the_body() {
    let text = "pub async fn work() -> i32 { 7 } fn main() {}";
    let provider = future_provider();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![Arc::new(provider.to_declaration())]);
    let (bytecode, package) = compiled_in(text, None, analysis);
    let mut runtime = Runtime::default();
    provider.install(&mut runtime).unwrap();
    let program = runtime
        .load_verified_program(
            "future",
            VerifiedProgram::from_bytecode(bytecode)
                .with_source_package(package, &Default::default())
                .unwrap(),
        )
        .unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let factory = owner
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "work")
        .unwrap();
    let resume = owner
        .bytecode
        .functions
        .iter()
        .find(|function| function.metadata.effects.may_suspend)
        .unwrap();
    let root = {
        let stack = runtime.enter_execution_stack(&owner).unwrap();
        stack
            .push(&runtime, owner.slot(), factory.id, &[], None)
            .unwrap();
        let value = stack
            .make_future(
                &runtime,
                resume.id,
                vec![],
                factory.metadata.semantic.result.as_ref().unwrap(),
            )
            .unwrap();
        runtime.root_value(value).unwrap()
    };
    let exact = runtime.value_source_target(&root).unwrap();
    assert!(
        available(runtime.function_source(&exact).unwrap())
            .text
            .contains("{ 7 }")
    );
    let Value::GcHandle(id) = root.value(runtime.gc()).unwrap() else {
        panic!("Future value")
    };
    assert!(
        runtime.gc().future_script(id).unwrap().is_some(),
        "inspection must not claim the Future"
    );
    drop(runtime.gc().take_cold_future(id).unwrap());
    let consumed = runtime.value_source_target(&root).unwrap();
    assert!(
        matches!(runtime.function_source(&consumed).unwrap(), SourceQuery::Unavailable(source) if source.reason == SourceUnavailableReason::NoObservedBody && source.publication == Some(owner.publication_token()))
    );
    assert!(
        available(runtime.function_source(&exact).unwrap())
            .text
            .contains("{ 7 }")
    );
}

#[test]
fn split_impl_locators_never_alias_authored_extents() {
    let text = "struct S {} impl S { fn one(self) -> i32 { 1 } } impl S { fn two(self) -> i32 { 2 } } fn main() {}";
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("split", prepared(text, None))
        .unwrap();
    let old = runtime.source_snapshot(&program).unwrap();
    let blocks: Vec<_> = old
        .declarations()
        .iter()
        .filter(|declaration| {
            declaration
                .path()
                .path
                .last()
                .is_some_and(|part| part.kind == kagari_common::identity::DefinitionKind::Impl)
        })
        .cloned()
        .collect();
    assert_eq!(blocks.len(), 2);
    let first = available(old.function(&blocks[0]));
    let second = available(old.function(&blocks[1]));
    assert_ne!(first.range, second.range);
    assert_ne!(first.text, second.text);
    assert_eq!(
        runtime
            .resolve_source_declaration(&program, &blocks[0])
            .unwrap()
            .range,
        first.range
    );
    let baseline = runtime.select_program(&program).unwrap();
    let staged = runtime
        .stage_reload_verified_program(
            &program,
            None,
            prepared(&format!("// shift\n{text}"), Some(&baseline)),
        )
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
    for block in &blocks {
        assert!(runtime.resolve_source_declaration(&program, block).is_err());
    }
    let method = declaration(&old, "one");
    assert!(
        runtime
            .resolve_source_declaration(&program, &method)
            .is_ok()
    );
}

#[test]
fn rejected_candidate_never_changes_active_source() {
    let text = "pub fn value() -> i32 { 1 } fn main() {}";
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("exact", prepared(text, None))
        .unwrap();
    let baseline = runtime.select_program(&program).unwrap();
    let target = FunctionSourceTarget::current(
        baseline
            .bind_call_route(CallableTarget::Script(
                baseline
                    .bytecode
                    .functions
                    .iter()
                    .find(|function| function.name == "value")
                    .unwrap()
                    .id,
            ))
            .unwrap(),
    );
    let before = available(runtime.function_source(&target).unwrap());
    let staged = runtime
        .stage_reload_verified_program(
            &program,
            None,
            prepared(&text.replace("{ 1 }", "{ 2 }"), Some(&baseline)),
        )
        .unwrap();
    drop(staged);
    runtime.collect_garbage().unwrap();
    let after = available(runtime.function_source(&target).unwrap());
    assert_eq!(before.publication, after.publication);
    assert_eq!(before.text, after.text);
}

#[test]
fn source_package_rejects_mismatched_executable_products() {
    let text = "pub fn value() -> i32 { 1 } fn main() {}";
    let (_, package) = compiled(text, None);
    let (different, _) = compiled(&text.replace("{ 1 }", "{ 2 }"), None);
    assert!(
        VerifiedProgram::from_bytecode(different)
            .with_source_package(package.clone(), &Default::default())
            .is_err()
    );
}
