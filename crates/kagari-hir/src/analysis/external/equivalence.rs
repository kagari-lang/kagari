//! Independent structured descriptors and fixed parsed text exercise one checker.

use crate::{
    analysis::{AnalysisDatabase, AnalysisSnapshot, FileAnalysis},
    compile_input::program::{AdmissionError, CheckedCompilation},
    hir::{
        expr::ExprKind,
        item::function::{FunctionImplementation, NativeContract},
    },
    types::TypeId,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_source::source_database::{CapturedSource, SourceDatabase, SourceLayer};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        FieldDef, FnDecl, Param, TypeDef, TypeDefKind,
        module::{ImplDecl, ModuleDecl},
        names::{ExportName, NameNamespace},
        native::NativeStorageLayout,
        requirement::NativeCallableRequirement,
    },
    range::RangeKind,
    reference::ReferenceAccess,
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
    visibility::Visibility,
};
use std::sync::Arc;

const SUPPORT: &str =
    include_str!("../../../tests/fixtures/analysis/external/equivalent_support.kgr");
const API: &str = include_str!("../../../tests/fixtures/analysis/external/equivalent_api.kgr");
const CLIENT: &str =
    include_str!("../../../tests/fixtures/analysis/external/equivalent_client.kgr");
const INVALID: &str =
    include_str!("../../../tests/fixtures/analysis/external/equivalent_invalid.kgr");

fn identity(package: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId(package.into()),
        path: vec![],
    }
}

fn nominal(declaration: DefinitionId) -> NominalTy {
    NominalTy {
        declaration,
        arguments: vec![],
        associated_types: Default::default(),
    }
}

fn function(
    name: &str,
    implementation: CallableImplementation,
    params: Vec<(&str, Ty)>,
    result: Ty,
) -> FnDecl {
    FnDecl {
        host_policy: None,
        method_policy: MethodPolicy::default(),
        name: name.into(),
        implementation,
        generic_params: vec![],
        bounds: vec![],
        params: params
            .into_iter()
            .map(|(name, ty)| Param {
                name: name.into(),
                ty,
                mutable: false,
            })
            .collect(),
        return_type: result,
    }
}

fn registration() -> Arc<DefinitionMetadata<ModuleDecl>> {
    let context = DefinitionContext::new().unwrap();
    let root = context.intern_root(&identity("api")).unwrap();
    let support = context.intern_root(&identity("support")).unwrap();
    let child = context
        .intern_child(support, DefinitionKind::Struct, "Child", 0)
        .unwrap();
    let read = context
        .intern_child(support, DefinitionKind::Trait, "Read", 0)
        .unwrap();
    let counter = context
        .intern_child(root, DefinitionKind::AssociatedType, "Counter", 0)
        .unwrap();
    let receiver = Ty::NativeObject(nominal(counter));
    let mut module = ModuleDecl::new(identity("api"));
    module.dependencies.insert(identity("support"));
    module.types.push(TypeDef {
        name: "Counter".into(),
        kind: TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
        generic_params: vec![],
        bounds: vec![],
        variants: vec![],
        fields: vec![
            FieldDef {
                host_policy: None,
                visibility: Visibility::Public,
                name: "value".into(),
                ty: Ty::Builtin(BuiltinType::I32),
                mutable: true,
            },
            FieldDef {
                host_policy: None,
                visibility: Visibility::Public,
                name: "child".into(),
                ty: Ty::Struct(nominal(child)),
                mutable: false,
            },
            FieldDef {
                host_policy: None,
                visibility: Visibility::Private,
                name: "secret".into(),
                ty: Ty::Builtin(BuiltinType::I32),
                mutable: false,
            },
        ],
    });
    let inherent = context
        .intern_child(root, DefinitionKind::Impl, "", 0)
        .unwrap();
    let new = context
        .intern_child(inherent, DefinitionKind::Method, "new", 0)
        .unwrap();
    let echo = context
        .intern_child(inherent, DefinitionKind::Method, "echo", 0)
        .unwrap();

    let generic = GenericParam {
        owner: echo,
        position: 0,
    };
    let mut echo_function = function(
        "echo",
        CallableImplementation::Native(echo),
        vec![("self", receiver.clone()), ("value", generic.as_type())],
        generic.as_type(),
    );
    echo_function.generic_params.push(generic);

    module.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: None,
        for_type: receiver.clone(),
        methods: vec![
            function(
                "new",
                CallableImplementation::Native(new),
                vec![],
                receiver.clone(),
            ),
            echo_function,
        ],
    });
    let implementation = context
        .intern_child(root, DefinitionKind::Impl, "", 1)
        .unwrap();
    let method = context
        .intern_child(implementation, DefinitionKind::Method, "read", 0)
        .unwrap();
    module.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: Some(nominal(read)),
        for_type: receiver.clone(),
        methods: vec![function(
            "read",
            CallableImplementation::Native(method),
            vec![("self", receiver)],
            Ty::Builtin(BuiltinType::I32),
        )],
    });

    let bounded = context
        .intern_child(root, DefinitionKind::Function, "bounded", 0)
        .unwrap();
    let generic = GenericParam {
        owner: bounded,
        position: 0,
    };
    let mut bounded_function = function(
        "bounded",
        CallableImplementation::Native(bounded),
        vec![("value", generic.as_type())],
        generic.as_type(),
    );
    bounded_function.bounds.push(GenericBound {
        ty: generic.as_type(),
        constraints: vec![nominal(read)],
    });
    bounded_function.generic_params.push(generic);
    module.functions.push(bounded_function);
    let operation = context
        .intern_child(support, DefinitionKind::Trait, "Operation", 0)
        .unwrap();
    let output = context
        .intern_child(operation, DefinitionKind::AssociatedType, "Output", 0)
        .unwrap();

    let requirement = |name, argument| NativeCallableRequirement {
        receiver: Ty::Struct(nominal(child)),
        interface: NominalTy {
            declaration: operation,
            arguments: vec![Ty::Builtin(BuiltinType::I32)],
            associated_types: [(output, Ty::Builtin(BuiltinType::I32))].into(),
        },
        member: context
            .intern_child(operation, DefinitionKind::Method, name, 0)
            .unwrap(),
        arguments: vec![Ty::Builtin(argument)],
    };
    let first = requirement("apply", BuiltinType::Bool);
    let mut deferred = requirement("again", BuiltinType::I32);
    deferred.arguments.clear();
    module.callable_requirements.insert(
        bounded,
        vec![
            first.clone(),
            requirement("again", BuiltinType::I32),
            first,
            deferred,
        ],
    );
    let produce = context
        .intern_child(root, DefinitionKind::Function, "produce", 0)
        .unwrap();
    module.functions.push(function(
        "produce",
        CallableImplementation::Native(produce),
        vec![],
        Ty::Trait(nominal(read)),
    ));
    module
        .concrete_results
        .insert(produce, Ty::NativeObject(nominal(counter)));
    let definitions = module.collect_definitions(&context).unwrap();
    Arc::new(DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap())
}

pub(super) fn constructor_registration() -> Arc<DefinitionMetadata<ModuleDecl>> {
    let context = DefinitionContext::new().unwrap();
    let root = context.intern_root(&identity("api")).unwrap();
    let mut module = ModuleDecl::new(identity("api"));
    for (name, package, path) in [("String", "alloc", "string"), ("Range", "core", "ops")] {
        let identity = kagari_stdlib::namespaces::module(package, path);
        let owner = context.intern_root(&identity).unwrap();
        let target = context
            .intern_child(owner, DefinitionKind::AssociatedType, name, 0)
            .unwrap();
        module.dependencies.insert(identity);
        module
            .exports
            .insert(ExportName::new(NameNamespace::Type, name), target);
    }
    module.types.push(TypeDef {
        name: "Target".into(),
        kind: TypeDefKind::Struct,
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    let target = context
        .intern_child(root, DefinitionKind::Struct, "Target", 0)
        .unwrap();
    let target = Ty::Struct(nominal(target));
    let owner = context
        .intern_child(root, DefinitionKind::Impl, "", 0)
        .unwrap();
    let methods = [
        ("shared", Some(ReferenceAccess::Shared)),
        ("mutable", Some(ReferenceAccess::Mutable)),
        ("plain", None),
    ]
    .into_iter()
    .map(|(name, access)| {
        let id = context
            .intern_child(owner, DefinitionKind::Method, name, 0)
            .unwrap();
        let mut method = function(
            name,
            CallableImplementation::Native(id),
            vec![("self", target.clone())],
            Ty::Builtin(BuiltinType::I32),
        );
        method.method_policy.receiver_access = access;
        method
    })
    .collect();
    module.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: None,
        for_type: target,
        methods,
    });
    for (name, params, result) in [
        (
            "text",
            vec![("value", Ty::Builtin(BuiltinType::String))],
            Ty::Builtin(BuiltinType::String),
        ),
        (
            "range",
            vec![(
                "value",
                Ty::Range(
                    Box::new(Ty::Builtin(BuiltinType::I32)),
                    RangeKind::Exclusive,
                ),
            )],
            Ty::Range(
                Box::new(Ty::Builtin(BuiltinType::I32)),
                RangeKind::Exclusive,
            ),
        ),
    ] {
        let declaration = context
            .intern_child(root, DefinitionKind::Function, name, 0)
            .unwrap();
        module.functions.push(function(
            name,
            CallableImplementation::Native(declaration),
            params,
            result,
        ));
    }
    let definitions = module.collect_definitions(&context).unwrap();
    Arc::new(DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap())
}

#[test]
fn structured_and_offline_apis_share_fields_calls_traits_and_diagnostic_rules() {
    let mut sources = SourceDatabase::new("/project").unwrap();
    sources
        .bind_module("support.kgr", identity("support"))
        .unwrap();
    sources
        .bind_module(
            "client.kgr",
            ModuleIdentity {
                package: PackageId("support".into()),
                path: vec!["client".into()],
            },
        )
        .unwrap();
    sources
        .set("support.kgr", SUPPORT.into(), SourceLayer::Base)
        .unwrap();
    let mut direct = AnalysisDatabase::new("/project").unwrap();
    direct.set_native_modules(vec![registration()]);
    let mut offline = AnalysisDatabase::new("/project").unwrap();
    for (text, valid) in [(CLIENT, true), (INVALID, false)] {
        let root = sources
            .set("client.kgr", text.into(), SourceLayer::Overlay)
            .unwrap();
        sources
            .replace_generated(vec![], &Default::default())
            .unwrap();
        let direct_snapshot = direct
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        offline.set_import_environment(
            direct_snapshot
                .declaration_snapshot()
                .import_environment()
                .clone(),
        );
        sources
            .replace_generated(
                vec![CapturedSource {
                    key: "api.kgr".into(),
                    display_name: "api.kgr".into(),
                    module: identity("api"),
                    text: API.into(),
                }],
                &Default::default(),
            )
            .unwrap();
        let offline_snapshot = offline
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let direct_file = direct_snapshot.file(root).unwrap();
        let offline_file = offline_snapshot.file(root).unwrap();
        let diagnostic_kinds = |file: &FileAnalysis| {
            file.result()
                .diagnostics()
                .iter()
                .map(|diagnostic| (diagnostic.kind.clone(), diagnostic.span))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            diagnostic_kinds(direct_file),
            diagnostic_kinds(offline_file)
        );
        assert_eq!(
            direct_snapshot
                .check_program(root, &Default::default())
                .is_ok(),
            valid,
            "{:?}",
            direct_file.result().diagnostics()
        );
        assert_eq!(
            offline_snapshot
                .check_program(root, &Default::default())
                .is_ok(),
            valid,
            "{:?}",
            offline_file.result().diagnostics()
        );
        let projection = |file: &FileAnalysis| {
            let facts = file.result().facts();
            facts
                .lowered
                .module
                .nodes
                .expressions()
                .map(|(id, expression)| {
                    let span = facts.lowered.module.expr(id).origin.diagnostic_span();
                    let ty = facts
                        .typed
                        .type_table()
                        .expr_type(id)
                        .map(|ty| ty.display_name(file.definitions()));
                    let call = facts
                        .typed
                        .type_table()
                        .call_resolution(id)
                        .and_then(|call| {
                            call.signature.as_ref().map(|signature| {
                                (
                                    signature
                                        .params
                                        .iter()
                                        .map(|ty| ty.display_name(file.definitions()))
                                        .collect::<Vec<_>>(),
                                    signature.return_type.display_name(file.definitions()),
                                )
                            })
                        });
                    (
                        span,
                        matches!(expression.kind, ExprKind::Call { .. }),
                        ty,
                        call,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(projection(direct_file), projection(offline_file));
        let context = DefinitionContext::new().unwrap();
        for name in ["bounded", "produce"] {
            let project = |snapshot: &AnalysisSnapshot| {
                let file = snapshot
                    .files()
                    .find(|file| file.source().module_identity() == &identity("api"))
                    .unwrap();
                let function = file
                    .result()
                    .facts()
                    .lowered
                    .module
                    .functions
                    .iter()
                    .find(|function| function.name == name)
                    .unwrap();
                let FunctionImplementation::Native(contract) = &function.implementation else {
                    panic!("Native contract must remain Native");
                };
                DefinitionMetadata::checked(
                    file.definitions().clone(),
                    contract.clone(),
                    &Default::default(),
                )
                .unwrap()
                .import_into(&context, &Default::default())
                .unwrap()
                .into_records()
            };
            let direct = project(&direct_snapshot);
            assert_eq!(project(&offline_snapshot), NativeContract::default());
            if name == "bounded" {
                assert_eq!(direct.callable_requirements.len(), 4);
                assert_eq!(
                    direct.callable_requirements[0],
                    direct.callable_requirements[2]
                );
                let definitions = context.snapshot();
                for (requirement, (member, argument)) in direct.callable_requirements.iter().zip([
                    ("apply", Some(BuiltinType::Bool)),
                    ("again", Some(BuiltinType::I32)),
                    ("apply", Some(BuiltinType::Bool)),
                    ("again", None),
                ]) {
                    let Ty::Struct(target) = &requirement.receiver else {
                        panic!("operation target");
                    };
                    assert_eq!(
                        definitions
                            .resolve(target.declaration)
                            .unwrap()
                            .last()
                            .unwrap()
                            .name,
                        "Child"
                    );
                    assert_eq!(
                        definitions.resolve(target.declaration).unwrap().module(),
                        &identity("support")
                    );
                    assert_eq!(
                        definitions
                            .resolve(requirement.interface.declaration)
                            .unwrap()
                            .last()
                            .unwrap()
                            .name,
                        "Operation"
                    );
                    assert_eq!(
                        requirement.interface.arguments,
                        vec![Ty::Builtin(BuiltinType::I32)]
                    );
                    let outputs = &requirement.interface.associated_types;
                    assert_eq!(outputs.len(), 1);
                    let (output, value) = outputs.first_key_value().unwrap();
                    assert_eq!(
                        definitions.parent(*output).unwrap(),
                        Some(requirement.interface.declaration)
                    );
                    assert_eq!(
                        definitions.resolve(*output).unwrap().last().unwrap().name,
                        "Output"
                    );
                    assert_eq!(value, &Ty::Builtin(BuiltinType::I32));
                    assert_eq!(
                        definitions.parent(requirement.member).unwrap(),
                        Some(requirement.interface.declaration)
                    );
                    assert_eq!(
                        definitions
                            .resolve(requirement.member)
                            .unwrap()
                            .last()
                            .unwrap()
                            .name,
                        member
                    );
                    assert_eq!(
                        requirement.arguments,
                        argument.into_iter().map(Ty::Builtin).collect::<Vec<_>>()
                    );
                }
                assert!(direct.concrete_result.is_none());
            } else {
                let Some(Ty::NativeObject(target)) = direct.concrete_result else {
                    panic!("concrete object result");
                };
                assert_eq!(
                    context
                        .snapshot()
                        .resolve(target.declaration)
                        .unwrap()
                        .last()
                        .unwrap()
                        .name,
                    "Counter"
                );
                assert!(direct.callable_requirements.is_empty());
            }
        }
        if valid {
            let program = direct_snapshot
                .check_program(root, &Default::default())
                .unwrap();
            assert!(matches!(
                CheckedCompilation::from_program(&program, &Default::default()),
                Err(AdmissionError::InvalidModule(
                    "Native interface fields have no executable adapters"
                ))
            ));
            let facts = offline_file.result().facts();
            let field = facts
                .aggregates
                .native_types()
                .find(|item| item.declaration.name == "Counter")
                .unwrap();
            assert_eq!(field.fields[0].ty, TypeId::Builtin(BuiltinType::I32));
            assert!(matches!(field.fields[1].ty, TypeId::Struct(_)));
        }
    }
}
