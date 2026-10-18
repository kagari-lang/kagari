//! Scalar declarations own checked targets; Rust algorithm reuse is not target sharing.
use crate::{
    Runtime,
    frame::driver::ReturnAction,
    module::LoadedModule,
    native::{
        assembly::{ModuleAssembly, ModuleDefinition, ModuleSetBuilder},
        binding::{Codec, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        context::{CallContext, ScriptCall},
        declarations::MethodDecl,
        module::NativeModule,
        types::{Type, same_definition},
    },
    value::Value,
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::callable::CallableImplementation;
use std::sync::Arc;

// One Rust function, not two per-scalar implementations or a spelling dispatcher.
// The binding for each declaration still states its own exact conversion contract.
fn identity(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.argument(0)
}

fn assembly(invalid_second_codec: bool) -> ModuleAssembly {
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(
        ModuleIdentity {
            package: PackageId("kagari-core".into()),
            path: vec!["num".into()],
        },
        move |module| {
            for (index, scalar) in [Type::i32(), Type::i64()].into_iter().enumerate() {
                let codec = if invalid_second_codec && index == 1 {
                    Type::i32()
                } else {
                    scalar.clone()
                };
                module.inherent(scalar.clone(), |methods| {
                    methods.define(
                        MethodDecl::instance("identity")
                            .returns(scalar)
                            .implementation(NativeBinding::new(
                                vec![Codec::Exact(codec.clone())],
                                Codec::Exact(codec),
                                identity,
                            )),
                    )?;
                    Ok(())
                })?;
            }
            Ok(())
        },
    ))
    .unwrap();
    set.finish().unwrap()
}

fn no_script_reentry(
    _: &Runtime,
    _: &LoadedModule,
    _: ScriptCall<'_>,
    _: &[Value],
) -> NativeResult<Value> {
    panic!("scalar identity does not reenter script")
}

#[test]
fn distinct_scalar_impls_reuse_one_algorithm_without_merging_checked_or_applied_targets() {
    let assembly = assembly(false);
    let snapshot = assembly.declarations();
    let module = snapshot.modules()[0].records();
    let definitions = snapshot.modules()[0].definitions();
    let declarations = module.native_declarations(definitions).unwrap();
    assert_eq!(module.implementations.len(), 2);
    assert_eq!(declarations.len(), 2);
    assert_ne!(declarations[0].declaration, declarations[1].declaration);
    for (declaration, scalar) in declarations.iter().zip([Type::i32(), Type::i64()]) {
        assert_eq!(declaration.function.name, "identity");
        assert_eq!(declaration.function.params[0].ty, *scalar.abi());
        assert_eq!(declaration.function.return_type, *scalar.abi());
        assert_eq!(
            declaration.function.implementation,
            CallableImplementation::Native(declaration.declaration),
            "each declaration owns its binding target even though its callback is the same Rust function"
        );
    }
    let modules = assembly.into_modules().unwrap();
    assert_eq!(
        modules[0]
            .declaration()
            .native_declarations(modules[0].definitions())
            .unwrap(),
        declarations,
        "checked module preparation must preserve both exact declaration/target contracts"
    );
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(snapshot.modules().iter().cloned().map(Arc::new).collect());
    let mut sources = SourceDatabase::default();
    let root = sources.set("scalar-targets.kgr", "fn main() -> i64 { val first: i32 = 7; val second: i64 = 9; first.identity(); second.identity() }".into(), SourceLayer::Base).unwrap();
    let checked = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir).unwrap();
    let mut runtime = Runtime::default();
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let loaded = runtime.load_program("scalar-targets", bytecode).unwrap();
    let mut applied = vec![];
    for (declaration, value) in declarations.iter().zip([Value::I32(7), Value::I64(9)]) {
        let matches = loaded
            .members()
            .flat_map(|owner| {
                owner
                    .bytecode
                    .native_imports
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, import)| {
                        same_definition(
                            import.instance.declaration,
                            owner.definitions(),
                            declaration.declaration,
                            definitions,
                        )
                        .then_some((owner.clone(), NativeImportId::new(slot)))
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            matches.len(),
            1,
            "each scalar has its own compiler-selected native import"
        );
        let (owner, slot) = &matches[0];
        let import = &owner.bytecode.native_imports[slot.index()];
        assert_eq!(import.binding, import.instance.declaration);
        assert_eq!(
            import.signature.params,
            [declaration.function.params[0].ty.clone()]
        );
        assert_eq!(import.signature.result, declaration.function.return_type);
        assert!(import.structurally_valid(owner.definitions()));
        let linked = runtime.modules.native_binding(owner, *slot).unwrap();
        assert_eq!(linked.declaration, import.instance.declaration);
        assert_eq!(linked.signature, import.signature);
        let signature = linked.type_signature().unwrap();
        assert_eq!(signature.params[0].ty(), &declaration.function.params[0].ty);
        assert_eq!(signature.result.ty(), &declaration.function.return_type);
        assert!(
            matches!(&linked.binding.arguments[0], Codec::Exact(ty) if ty.abi() == &declaration.function.params[0].ty)
        );
        assert!(
            matches!(&linked.binding.result, Codec::Exact(ty) if ty.abi() == &declaration.function.return_type)
        );
        applied.push((import.instance.clone(), import.binding));
        let stack = runtime.enter_execution_stack(owner).unwrap();
        stack
            .push_callable(
                &runtime,
                owner.clone(),
                CallableTarget::Native(*slot),
                std::slice::from_ref(&value),
                None,
            )
            .unwrap();
        stack
            .start_native_entry(&runtime, no_script_reentry)
            .unwrap();
        let ReturnAction::Complete(result) = stack.finish_native_return(&runtime).unwrap() else {
            panic!("scalar return")
        };
        assert_eq!(result, value);
    }
    assert_ne!(
        applied[0].0, applied[1].0,
        "Rust function reuse must not merge applied identities"
    );
    assert_ne!(
        applied[0].1, applied[1].1,
        "Rust function reuse must not merge binding targets"
    );
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn reusing_a_checked_scalar_callback_does_not_skip_the_second_targets_codec_check() {
    let assembly = assembly(true);
    let snapshot = assembly.declarations();
    assert_eq!(snapshot.catalog().declarations.values().count(), 2);
    let error = assembly.into_modules().expect_err(
        "the i32 callback's successful check cannot authorize an i64 target with i32 codecs",
    );
    assert!(
        error
            .to_string()
            .contains("native codec does not match the Kagari declaration"),
        "{error}"
    );
}
