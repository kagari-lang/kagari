//! Regression witnesses for ordinary default selection and exact expression targets.

use crate::module::VerifiedProgram;
use crate::{
    Runtime,
    native::{
        binding::{Codec, NativeBinding},
        catalog::DeclarationCatalog,
        declarations::SelectedCall,
        module::NativeModule,
    },
    test_support::bytecode::load_bytecode,
    value::view::ValueView,
};
use crate::{executor::Executor, native::conversion::arguments::ArgumentValues};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ConstantId, ConstantOperand},
    program::{
        BytecodeProgram,
        updates::{self, EvidenceInputs},
    },
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::types::PublicItem;
use kagari_hir::{
    analysis::AnalysisDatabase, compile_input::program::CheckedCompilation, program::CheckedProgram,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        AssociatedTypeDef, FnDecl, Param, TraitDef,
        module::{ImplDecl, ModuleDecl},
        requirement::NativeCallableRequirement,
        script::ScriptExpr,
    },
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn scalar() -> Ty {
    Ty::Builtin(BuiltinType::I32)
}

fn function(
    name: &str,
    implementation: CallableImplementation,
    params: Vec<Param>,
    result: Ty,
) -> FnDecl {
    FnDecl {
        name: name.into(),
        implementation,
        params,
        return_type: result,
        generic_params: vec![],
        bounds: vec![],
        method_policy: Default::default(),
        host_policy: None,
    }
}

fn parameter(name: &str, ty: Ty) -> Param {
    Param {
        name: name.into(),
        ty,
        mutable: false,
    }
}

fn module() -> (DeclarationCatalog, ModuleDecl) {
    (
        DeclarationCatalog::default(),
        ModuleDecl::new(ModuleIdentity {
            package: PackageId("api".into()),
            path: vec![],
        }),
    )
}

fn compile_registered(
    module: &NativeModule,
    source: &str,
    rendered: bool,
) -> DefinitionMetadata<BytecodeProgram> {
    let metadata = Arc::new(module.to_declaration());
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![metadata.clone()]);
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("regression.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    if rendered {
        let declarations = analysis
            .declarations(sources.snapshot(), &Default::default())
            .unwrap();
        let document = declarations
            .render_native_document(&metadata, &Default::default())
            .unwrap();
        sources
            .bind_module(&document.uri, metadata.records().identity.clone())
            .unwrap();
        sources
            .set(&document.uri, document.text, SourceLayer::Base)
            .unwrap();
        analysis = AnalysisDatabase::default();
        analysis.set_import_environment(declarations.import_environment().clone());
    }
    let checked = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    compile(&checked)
}

fn run(module: NativeModule, source: &str, rendered: bool) -> i32 {
    let bytecode = compile_registered(&module, source, rendered);
    run_decoded(&module, bytecode)
}

fn run_decoded(module: &NativeModule, bytecode: DefinitionMetadata<BytecodeProgram>) -> i32 {
    let context = DefinitionContext::new().unwrap();
    let imported = bytecode.import_into(&context, &Default::default()).unwrap();
    let artifact = KbcArtifact::from_program(
        imported.into_records(),
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let decoded =
        KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap()).unwrap();
    let bytecode = DefinitionMetadata::checked(
        decoded.definitions().clone(),
        decoded.records().program.clone(),
        &Default::default(),
    )
    .unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    execute(&mut runtime, bytecode)
}

fn compile(checked: &CheckedProgram) -> DefinitionMetadata<BytecodeProgram> {
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified()
}

fn execute(runtime: &mut Runtime, bytecode: DefinitionMetadata<BytecodeProgram>) -> i32 {
    let loaded = load_bytecode(runtime, "regression", bytecode).unwrap();
    let selected = runtime.select_program(&loaded).unwrap();
    let (owner, entry) = selected.entry_function().unwrap();
    Executor::new(runtime, &owner, entry, ArgumentValues::scalars(&[]))
        .unwrap()
        .run(|runtime, stack, value| Ok(stack.retain_result(runtime, value)?.as_i32().unwrap()))
        .unwrap()
}

#[test]
fn default_selection_agrees_for_registration_and_generated_source() {
    let catalog = DeclarationCatalog::default();
    let context = catalog.types.context();
    let package = PackageId("digits".into());
    let mut api = ModuleDecl::new(ModuleIdentity {
        package: package.clone(),
        path: vec!["api".into()],
    });
    let root = context.intern_root(&api.identity).unwrap();
    let owner = context
        .intern_child(root, DefinitionKind::Trait, "Pair", 0)
        .unwrap();
    let swapped = context
        .intern_child(owner, DefinitionKind::Method, "swapped", 0)
        .unwrap();
    let params = vec![
        parameter("self", Ty::SelfType(owner)),
        parameter(
            "combine",
            Ty::Function {
                params: vec![scalar(), scalar()],
                result: Box::new(scalar()),
            },
        ),
        parameter("first", scalar()),
        parameter("second", scalar()),
    ];
    api.traits.push(TraitDef {
        name: "Pair".into(),
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        conversion_adapter: None,
        methods: vec![function(
            "swapped",
            CallableImplementation::Script,
            params,
            scalar(),
        )],
    });
    api.script_bodies.insert(
        swapped,
        ScriptExpr::Call {
            callee: Box::new(ScriptExpr::Name("combine".into())),
            type_arguments: vec![],
            arguments: ["second", "first"]
                .map(|name| ScriptExpr::Name(name.into()))
                .into(),
        },
    );
    let cancel = Default::default();
    let module = NativeModule::checked(
        api,
        vec![],
        Default::default(),
        vec![],
        Default::default(),
        &catalog,
    )
    .unwrap();
    let registration = Arc::new(module.to_declaration());
    let mut client = SourceDatabase::default();
    client
        .bind_module(
            "digits.kgr",
            ModuleIdentity {
                package,
                path: vec![],
            },
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![registration]);
    let declarations = analysis.declarations(client.snapshot(), &cancel).unwrap();
    let [provider] = declarations.native_modules() else {
        panic!("expected only the Pair provider")
    };
    let document = declarations
        .render_native_document(provider, &cancel)
        .unwrap();
    let mut offline = AnalysisDatabase::default();
    offline.set_import_environment(declarations.import_environment().clone());
    for (route, db, document) in [
        ("registered", &mut analysis, None),
        ("generated", &mut offline, Some(document)),
    ] {
        if let Some(document) = document {
            client
                .bind_module(&document.uri, provider.records().identity.clone())
                .unwrap();
            client
                .set(&document.uri, document.text, SourceLayer::Base)
                .unwrap();
            assert!(db.inputs().native_modules().is_empty());
        }
        let root = client
            .set(
                "digits.kgr",
                include_str!("../../../../tests/fixtures/native/swapped-default.kgr").into(),
                SourceLayer::Base,
            )
            .unwrap();
        let snapshot = db.snapshot(client.snapshot(), &cancel).unwrap();
        let program = snapshot
            .check_program(root, &cancel)
            .unwrap_or_else(|err| panic!("{route}: {err:?}"));
        let product = compile(&program);
        let selected = if route == "registered" {
            run_decoded(&module, product)
        } else {
            execute(&mut Runtime::default(), product)
        };
        assert_eq!(selected, 4224, "{route}");
        let invalid = client
            .set(
                "digits.kgr",
                include_str!("../../../../tests/fixtures/native/mismatched-default.kgr").into(),
                SourceLayer::Base,
            )
            .unwrap();
        let snapshot = db.snapshot(client.snapshot(), &cancel).unwrap();
        let diagnostics = snapshot.file(invalid).unwrap().result().diagnostics();
        assert!(diagnostics.iter().any(|d| matches!(&d.kind,
            DiagnosticKind::TraitMethodMismatch { method_name, reason, .. }
                if method_name == "swapped" && reason == "return type expected `i32`, found `bool`")), "{route}: {diagnostics:?}");
        assert!(snapshot.check_program(invalid, &cancel).is_err());
    }
}

#[test]
fn explicit_definition_calls_the_module_helper_even_when_a_parameter_shadows_it() {
    for rendered in [false, true] {
        let (catalog, mut declaration) = module();
        let context = catalog.types.context();
        let root = context.intern_root(&declaration.identity).unwrap();
        let helper = context
            .intern_child(root, DefinitionKind::Function, "helper", 0)
            .unwrap();
        let run_id = context
            .intern_child(root, DefinitionKind::Function, "run", 0)
            .unwrap();
        let identity = context
            .intern_child(root, DefinitionKind::Function, "identity", 0)
            .unwrap();
        let generic = GenericParam {
            owner: identity,
            position: 0,
        };
        let mut identity_function = function(
            "identity",
            CallableImplementation::Native(identity),
            vec![parameter("value", generic.as_type())],
            generic.as_type(),
        );
        identity_function.generic_params.push(generic);
        declaration.functions.push(identity_function);
        declaration.functions.push(function(
            "helper",
            CallableImplementation::Native(helper),
            vec![],
            scalar(),
        ));
        declaration.private_functions.insert(helper);
        declaration.functions.push(function(
            "run",
            CallableImplementation::Script,
            vec![parameter(
                "helper",
                Ty::Function {
                    params: vec![],
                    result: Box::new(scalar()),
                },
            )],
            scalar(),
        ));
        declaration.script_bodies.insert(
            run_id,
            ScriptExpr::Call {
                callee: Box::new(ScriptExpr::Definition(identity)),
                type_arguments: vec![],
                arguments: vec![ScriptExpr::Call {
                    callee: Box::new(ScriptExpr::Definition(helper)),
                    type_arguments: vec![],
                    arguments: vec![],
                }],
            },
        );
        let binding = NativeBinding::new(
            vec![],
            Codec::Exact(crate::native::types::Type::i32()),
            |_| Ok(ValueView::from(42_i32)),
        );
        let module = NativeModule::checked(
            declaration,
            vec![
                (helper, binding),
                (
                    identity,
                    NativeBinding::new(vec![Codec::Value], Codec::Value, |call| call.argument(0)),
                ),
            ],
            Default::default(),
            vec![],
            Default::default(),
            &catalog,
        )
        .unwrap();
        assert_eq!(
            run(
                module,
                include_str!("../../../../tests/fixtures/native/exact-definition.kgr"),
                rendered
            ),
            42
        );
    }
}

#[test]
fn inherited_generic_native_defaults_execute_direct_and_interface_calls() {
    let (catalog, mut declaration) = module();
    let context = catalog.types.context();
    let root = context.intern_root(&declaration.identity).unwrap();
    let owner = context
        .intern_child(root, DefinitionKind::Trait, "Echo", 0)
        .unwrap();
    let method = context
        .intern_child(owner, DefinitionKind::Method, "echo", 0)
        .unwrap();
    let trait_parameter = GenericParam { owner, position: 0 };
    let method_parameter = GenericParam {
        owner: method,
        position: 0,
    };
    let mut echo = function(
        "echo",
        CallableImplementation::Native(method),
        vec![
            parameter("self", Ty::SelfType(owner)),
            parameter("unused", trait_parameter.as_type()),
            parameter("value", method_parameter.as_type()),
        ],
        method_parameter.as_type(),
    );
    echo.generic_params.push(method_parameter);
    declaration.traits.push(TraitDef {
        name: "Echo".into(),
        generic_params: vec![trait_parameter],
        bounds: vec![],
        methods: vec![echo],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        conversion_adapter: None,
    });
    declaration.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: Some(NominalTy {
            declaration: owner,
            arguments: vec![Ty::Builtin(BuiltinType::Bool)],
            associated_types: Default::default(),
        }),
        for_type: scalar(),
        methods: vec![],
    });
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let binding = NativeBinding::new(vec![Codec::Value; 3], Codec::Value, move |call| {
        calls.fetch_add(1, Ordering::SeqCst);
        call.argument(2)
    });
    let module = NativeModule::checked(
        declaration,
        vec![(method, binding)],
        Default::default(),
        vec![],
        Default::default(),
        &catalog,
    )
    .unwrap();
    assert_eq!(
        run(
            module,
            include_str!("../../../../tests/fixtures/native/inherited-native-default.kgr"),
            false
        ),
        42
    );
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn native_callback_executes_an_omitted_native_default_requirement() {
    let (catalog, mut declaration) = module();
    let context = catalog.types.context();
    let root = context.intern_root(&declaration.identity).unwrap();
    let owner = context
        .intern_child(root, DefinitionKind::Trait, "Read", 0)
        .unwrap();
    let method = context
        .intern_child(owner, DefinitionKind::Method, "read", 0)
        .unwrap();
    let invoke = context
        .intern_child(root, DefinitionKind::Function, "invoke", 0)
        .unwrap();
    declaration.traits.push(TraitDef {
        name: "Read".into(),
        generic_params: vec![],
        bounds: vec![],
        methods: vec![function(
            "read",
            CallableImplementation::Native(method),
            vec![parameter("self", Ty::SelfType(owner))],
            scalar(),
        )],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        conversion_adapter: None,
    });
    let interface = NominalTy {
        declaration: owner,
        arguments: vec![],
        associated_types: Default::default(),
    };
    declaration.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: Some(interface.clone()),
        for_type: scalar(),
        methods: vec![],
    });
    declaration.functions.push(function(
        "invoke",
        CallableImplementation::Native(invoke),
        vec![parameter("value", scalar())],
        scalar(),
    ));
    declaration.callable_requirements.insert(
        invoke,
        vec![NativeCallableRequirement {
            receiver: scalar(),
            interface,
            member: method,
            arguments: vec![],
        }],
    );
    let definitions = declaration.collect_definitions(context).unwrap();
    let native = declaration
        .native_declarations(&definitions)
        .unwrap()
        .into_iter()
        .find(|entry| entry.declaration == invoke)
        .unwrap();
    let native = DefinitionMetadata::checked(definitions, native, &Default::default()).unwrap();
    let token = SelectedCall::from_declaration(&native, &native.records().callable_requirements[0])
        .unwrap();
    let binding = NativeBinding::declared(&native, move |cx, (value,): (i32,)| {
        let selected = cx.selected_method::<(i32,), i32>(&token)?;
        selected.call(cx, (value,))
    });
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let selected = NativeBinding::new(
        vec![Codec::Value],
        Codec::Exact(crate::native::types::Type::i32()),
        move |cx| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(ValueView::from(cx.argument(0)?.as_i32().unwrap() + 2))
        },
    );
    let module = NativeModule::checked(
        declaration,
        vec![(method, selected), (invoke, binding)],
        Default::default(),
        vec![],
        Default::default(),
        &catalog,
    )
    .unwrap();
    assert_eq!(
        run(
            module,
            include_str!("../../../../tests/fixtures/native/required-native-default.kgr"),
            false
        ),
        42
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn native_associated_method_keeps_its_non_receiver_first_argument() {
    for inherited in [false, true] {
        let (catalog, mut declaration) = module();
        let context = catalog.types.context();
        let root = context.intern_root(&declaration.identity).unwrap();
        let owner = context
            .intern_child(root, DefinitionKind::Trait, "Make", 0)
            .unwrap();
        let method = context
            .intern_child(owner, DefinitionKind::Method, "make", 0)
            .unwrap();
        let implementation = context
            .intern_child(root, DefinitionKind::Impl, "", 0)
            .unwrap();
        let explicit = context
            .intern_child(implementation, DefinitionKind::Method, "make", 0)
            .unwrap();
        let target = if inherited { method } else { explicit };
        let params = vec![parameter("value", Ty::Builtin(BuiltinType::Bool))];
        declaration.traits.push(TraitDef {
            name: "Make".into(),
            generic_params: vec![],
            bounds: vec![],
            methods: vec![function(
                "make",
                if inherited {
                    CallableImplementation::Native(method)
                } else {
                    CallableImplementation::Required
                },
                params.clone(),
                scalar(),
            )],
            supertraits: vec![],
            associated_types: vec![],
            associated_consts: vec![],
            conversion_adapter: None,
        });
        declaration.implementations.push(ImplDecl {
            generic_params: vec![],
            bounds: vec![],
            trait_type: Some(NominalTy {
                declaration: owner,
                arguments: vec![],
                associated_types: Default::default(),
            }),
            for_type: scalar(),
            methods: if inherited {
                vec![]
            } else {
                vec![function(
                    "make",
                    CallableImplementation::Native(explicit),
                    params,
                    scalar(),
                )]
            },
        });
        let count = Arc::new(AtomicUsize::new(0));
        let calls = count.clone();
        let binding = NativeBinding::new(
            vec![Codec::Exact(crate::native::types::Type::bool())],
            Codec::Exact(crate::native::types::Type::i32()),
            move |cx| {
                calls.fetch_add(1, Ordering::SeqCst);
                assert_eq!(cx.argument(0)?.as_bool(), Some(true));
                Ok(ValueView::from(21_i32))
            },
        );
        let module = NativeModule::checked(
            declaration,
            vec![(target, binding)],
            Default::default(),
            vec![],
            Default::default(),
            &catalog,
        )
        .unwrap();
        assert_eq!(
            run(
                module,
                include_str!("../../../../tests/fixtures/native/associated-native-default.kgr"),
                false
            ),
            42
        );
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}

fn assert_inherited_body_authenticated(
    module: &NativeModule,
    bytecode: &DefinitionMetadata<BytecodeProgram>,
) {
    let mut forged = bytecode.records().clone();
    let placement = forged
        .modules
        .iter()
        .flat_map(|owner| &owner.public_items)
        .find_map(|item| match item {
            PublicItem::InterfaceTable(table) => {
                table.method_origins.first().map(|origin| origin.placement)
            }
            _ => None,
        })
        .unwrap();
    let owner = forged
        .modules
        .iter_mut()
        .find(|owner| {
            owner.functions.iter().any(|function| {
                function
                    .identity
                    .as_ref()
                    .is_some_and(|identity| identity.declaration == placement)
            })
        })
        .unwrap();
    let constant = ConstantId::new(owner.constants.len());
    owner.constants.push(ConstantOperand::I32(7));
    let function = owner
        .functions
        .iter_mut()
        .find(|function| {
            function
                .identity
                .as_ref()
                .is_some_and(|identity| identity.declaration == placement)
        })
        .unwrap();
    let (instruction, dst) = function
        .instructions
        .iter()
        .enumerate()
        .find_map(|(index, instruction)| {
            let BytecodeInstruction::Call { dst: Some(dst), .. } = instruction else {
                return None;
            };
            (function.metadata.semantic.slots.get(&dst.index()) == Some(&scalar()))
                .then_some((index, *dst))
        })
        .expect("canonical i32 helper call");
    function.instructions[instruction] = BytecodeInstruction::LoadConst { dst, constant };
    // Keep all ordinary origins and regenerate both the baseline and actual
    // instruction evidence. A stale certificate must not be the rejection oracle.
    let previous = &forged.update_evidence;
    forged.update_evidence = updates::build(
        &forged.modules,
        EvidenceInputs {
            sources: previous.sources.clone(),
            initializers: previous.initializers.clone(),
            selections: previous.selections.clone(),
            classifications: previous
                .bodies
                .iter()
                .map(|body| (body.requirement.origin.clone(), body.classification.clone()))
                .collect(),
        },
        bytecode.definitions(),
        &Default::default(),
    )
    .unwrap();
    let forged =
        DefinitionMetadata::checked(bytecode.definitions().clone(), forged, &Default::default())
            .unwrap();
    let context = DefinitionContext::new().unwrap();
    let verified = VerifiedProgram::new(forged, &context)
        .expect("forgery is internally valid, including its fresh instruction evidence");
    let mut recipient = Runtime::default();
    module.install(&mut recipient).unwrap();
    let error = recipient
        .load_verified_program("forged", verified)
        .unwrap_err();
    assert!(
        error.to_string().contains("installed canonical body"),
        "{error}"
    );
}

#[test]
fn source_free_inherited_script_rejects_a_same_signature_replacement() {
    let (catalog, mut declaration) = module();
    let context = catalog.types.context();
    let root = context.intern_root(&declaration.identity).unwrap();
    let owner = context
        .intern_child(root, DefinitionKind::Trait, "Read", 0)
        .unwrap();
    let method = context
        .intern_child(owner, DefinitionKind::Method, "read", 0)
        .unwrap();
    let helper = context
        .intern_child(root, DefinitionKind::Function, "helper", 0)
        .unwrap();
    let item = context
        .intern_child(owner, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    let item_method = context
        .intern_child(owner, DefinitionKind::Method, "item", 0)
        .unwrap();
    let identity = context
        .intern_child(root, DefinitionKind::Function, "identity", 0)
        .unwrap();
    let helper_item = GenericParam {
        owner: helper,
        position: 0,
    };
    let helper_method = GenericParam {
        owner: helper,
        position: 1,
    };
    let read_parameter = GenericParam {
        owner: method,
        position: 0,
    };
    let item_parameter = GenericParam {
        owner: item_method,
        position: 0,
    };
    let identity_parameter = GenericParam {
        owner: identity,
        position: 0,
    };
    let projection = Ty::Projection {
        receiver: Box::new(Ty::SelfType(owner)),
        interface: Box::new(NominalTy {
            declaration: owner,
            arguments: vec![],
            associated_types: Default::default(),
        }),
        member: item,
        arguments: vec![],
    };
    let mut helper_function = function(
        "helper",
        CallableImplementation::Native(helper),
        vec![parameter("value", helper_method.as_type())],
        scalar(),
    );
    helper_function.generic_params = vec![helper_item, helper_method];
    declaration.functions.push(helper_function);
    let mut identity_function = function(
        "identity",
        CallableImplementation::Native(identity),
        vec![parameter("value", identity_parameter.as_type())],
        identity_parameter.as_type(),
    );
    identity_function.generic_params.push(identity_parameter);
    declaration.functions.push(identity_function);
    declaration.private_functions.extend([helper, identity]);
    let mut item_function = function(
        "item",
        CallableImplementation::Script,
        vec![
            parameter("self", Ty::SelfType(owner)),
            parameter("value", projection.clone()),
            parameter("other", item_parameter.as_type()),
        ],
        projection.clone(),
    );
    item_function.generic_params.push(item_parameter);
    declaration.script_bodies.insert(
        item_method,
        ScriptExpr::Call {
            callee: Box::new(ScriptExpr::Definition(identity)),
            type_arguments: vec![],
            arguments: vec![ScriptExpr::Name("value".into())],
        },
    );
    let mut read = function(
        "read",
        CallableImplementation::Script,
        vec![
            parameter("self", Ty::SelfType(owner)),
            parameter("value", read_parameter.as_type()),
        ],
        scalar(),
    );
    read.generic_params.push(read_parameter.clone());
    declaration.traits.push(TraitDef {
        name: "Read".into(),
        methods: vec![read, item_function],
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![AssociatedTypeDef {
            declaration: item,
            generic_params: vec![],
            parameter_bounds: vec![],
            bounds: vec![],
        }],
        associated_consts: vec![],
        conversion_adapter: None,
    });
    declaration.script_bodies.insert(
        method,
        ScriptExpr::Call {
            callee: Box::new(ScriptExpr::Definition(helper)),
            type_arguments: vec![projection, read_parameter.as_type()],
            arguments: vec![ScriptExpr::Name("value".into())],
        },
    );
    declaration.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: Some(NominalTy {
            declaration: owner,
            arguments: vec![],
            associated_types: [(item, Ty::Builtin(BuiltinType::Bool))].into(),
        }),
        for_type: scalar(),
        methods: vec![],
    });
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let binding = NativeBinding::new(
        vec![Codec::Value],
        Codec::Exact(crate::native::types::Type::i32()),
        move |call| {
            calls.fetch_add(1, Ordering::SeqCst);
            match call.argument_type(0)? {
                Ty::Builtin(BuiltinType::I32) => assert_eq!(call.argument(0)?.as_i32(), Some(20)),
                Ty::Builtin(BuiltinType::I64) => assert_eq!(call.argument(0)?.as_i64(), Some(22)),
                ty => panic!("wrong applied method binder: {ty:?}"),
            }
            Ok(ValueView::from(21_i32))
        },
    );
    let identities = Arc::new(AtomicUsize::new(0));
    let identity_calls = identities.clone();
    let identity_binding = NativeBinding::new(vec![Codec::Value], Codec::Value, move |call| {
        identity_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(call.argument_type(0)?, &Ty::Builtin(BuiltinType::Bool));
        call.argument(0)
    });
    let module = NativeModule::checked(
        declaration,
        vec![(helper, binding), (identity, identity_binding)],
        Default::default(),
        vec![],
        Default::default(),
        &catalog,
    )
    .unwrap();
    let bytecode = compile_registered(
        &module,
        include_str!("../../../../tests/fixtures/native/inherited-script-default.kgr"),
        false,
    );
    assert_inherited_body_authenticated(&module, &bytecode);
    assert_eq!(run_decoded(&module, bytecode), 42);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(identities.load(Ordering::SeqCst), 2);
}
