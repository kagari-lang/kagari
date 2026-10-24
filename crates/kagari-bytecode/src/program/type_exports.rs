//! Source-free export authority survives encoding; forged leaves never gain a seal.

use crate::{
    artifact::KbcArtifact,
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef, verify_program},
    verifier::BytecodeVerificationError,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId},
};
use kagari_contract::{
    exports::{
        ExportNamespace, ProgramExports, TypeExport,
        type_validation::{TypeExportModule, TypeExportValidator},
    },
    layout::{StructFieldLayout, StructLayout},
    types::{PublicItem, TraitContract, proofs::ProofCatalog},
};
use kagari_types::{
    declaration::{
        FieldDef, TraitDef, TypeDeclaration, TypeDef, TypeDefKind, native::NativeStorageLayout,
    },
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty, substitution::TypeTransformError},
    visibility::Visibility,
};

use std::cell::Cell;

thread_local! {
    static CANCEL_TYPE_PROOF: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn before_type_validation(cancel: &CancellationToken) {
    if CANCEL_TYPE_PROOF.replace(false) {
        cancel.cancel();
    }
}

fn application(declaration: DefinitionId, arguments: Vec<Ty>) -> NominalTy {
    NominalTy {
        declaration,
        arguments,
        associated_types: Default::default(),
    }
}

fn fixture() -> (DefinitionContext, BytecodeProgram, DefinitionId) {
    let context = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("api.kgr");
    context.intern_root(&identity).unwrap();
    let hidden = ModuleIdentity::single_file("hidden.kgr");
    let root = context.intern_root(&hidden).unwrap();
    let id = context
        .intern_child(root, DefinitionKind::Struct, "Cell", 0)
        .unwrap();
    let field = context
        .intern_child(id, DefinitionKind::Field, "value", 0)
        .unwrap();
    let parameter = GenericParam {
        owner: id,
        position: 0,
    };
    let int = Ty::Builtin(BuiltinType::I32);
    let program = BytecodeProgram {
        update_evidence: crate::program::updates::empty(),
        root: ModuleRef::new(0),
        exports: ProgramExports {
            namespaces: [(
                identity.clone(),
                ExportNamespace {
                    types: [
                        ("Cell".into(), TypeExport::Declaration(id)),
                        (
                            "IntCell".into(),
                            TypeExport::Closed(Ty::Struct(application(id, vec![int.clone()]))),
                        ),
                        ("Erased".into(), TypeExport::OpenAlias { arity: 1 }),
                    ]
                    .into(),
                    ..Default::default()
                },
            )]
            .into(),
            entry: None,
        },
        modules: vec![
            BytecodeModule {
                identity,
                dependencies: vec![ModuleRef::new(1)],
                ..Default::default()
            },
            BytecodeModule {
                identity: hidden,
                public_items: vec![PublicItem::Type(TypeDef {
                    name: "Cell".into(),
                    kind: TypeDefKind::Struct,
                    generic_params: vec![parameter.clone()],
                    bounds: vec![],
                    variants: vec![],
                    fields: vec![FieldDef {
                        host_policy: None,
                        visibility: Visibility::Private,
                        name: "value".into(),
                        ty: parameter.as_type(),
                        mutable: true,
                    }],
                })],
                structures: vec![StructLayout {
                    declaration: id,
                    arguments: vec![int.clone()],
                    fields: vec![StructFieldLayout {
                        visibility: Visibility::Private,
                        declaration: field,
                        name: "value".into(),
                        ty: int,
                        mutable: true,
                    }],
                }],
                ..Default::default()
            },
        ],
    };
    (context, program, id)
}

fn replace(program: &mut BytecodeProgram, export: TypeExport) {
    let root = program.modules[0].identity.clone();
    program.exports.namespaces.get_mut(&root).unwrap().types = [("Target".into(), export)].into();
}

#[test]
fn type_exports_roundtrip_without_source_functions_or_public_owner_navigation() {
    let (context, program, _) = fixture();
    let artifact =
        KbcArtifact::from_program(program, Default::default(), &context, &Default::default())
            .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let imported = DefinitionContext::new().unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&imported, &Default::default())
        .unwrap()
        .into_records();
    decoded
        .validate_for_loader(&Default::default(), &imported, &Default::default())
        .unwrap();
    assert_eq!(decoded.to_bytes(&imported.snapshot()).unwrap(), bytes);
    assert!(
        decoded
            .program
            .modules
            .iter()
            .all(|module| module.functions.is_empty())
    );
    assert_eq!(decoded.program.exports.namespaces.len(), 1);
}

#[test]
fn type_exports_reject_nonpublic_wrong_kind_arity_and_nonconcrete_targets() {
    let (context, program, id) = fixture();
    verify_program(&program, &context.snapshot(), &Default::default()).unwrap();
    let owner = context.intern_root(&program.modules[1].identity).unwrap();
    let private = context
        .intern_child(owner, DefinitionKind::Struct, "Private", 0)
        .unwrap();
    let occurrence = context
        .intern_child(owner, DefinitionKind::Struct, "Cell", 1)
        .unwrap();
    let nested = context
        .intern_child(id, DefinitionKind::Struct, "Nested", 0)
        .unwrap();
    let trait_id = context
        .intern_child(owner, DefinitionKind::Trait, "Missing", 0)
        .unwrap();
    let member = context
        .intern_child(trait_id, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    let int = Ty::Builtin(BuiltinType::I32);
    let mut associated = application(id, vec![int.clone()]);
    associated.associated_types.insert(member, int.clone());
    let forged = [
        TypeExport::Declaration(private),
        TypeExport::Declaration(occurrence),
        TypeExport::Declaration(nested),
        TypeExport::Closed(Ty::Struct(application(private, vec![]))),
        TypeExport::Closed(Ty::Enum(application(id, vec![int.clone()]))),
        TypeExport::Closed(Ty::Struct(application(id, vec![]))),
        TypeExport::Closed(Ty::Struct(application(id, vec![int.clone(), int.clone()]))),
        TypeExport::Closed(Ty::Struct(associated)),
        TypeExport::Closed(Ty::Tuple(vec![Ty::Parameter {
            owner: id,
            position: 0,
        }])),
        TypeExport::Closed(Ty::Array(Box::new(Ty::SelfType(trait_id)))),
        TypeExport::Closed(Ty::Projection {
            arguments: vec![],
            receiver: Box::new(int),
            interface: Box::new(application(trait_id, vec![])),
            member,
        }),
    ];
    for export in forged {
        let mut forged = program.clone();
        replace(&mut forged, export.clone());
        assert!(
            matches!(
                verify_program(&forged, &context.snapshot(), &Default::default()),
                Err(BytecodeVerificationError::InvalidProgramExports)
            ),
            "{export:?}"
        );
    }
}

#[test]
fn type_exports_require_exact_closed_layout_and_declaration_owned_templates() {
    let (context, program, id) = fixture();
    let int = Ty::Builtin(BuiltinType::I32);
    let boolean = Ty::Builtin(BuiltinType::Bool);
    let mut missing = program.clone();
    missing.modules[1].structures.clear();
    assert!(matches!(
        verify_program(&missing, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidProgramExports)
    ));
    replace(&mut missing, TypeExport::Declaration(id));
    verify_program(&missing, &context.snapshot(), &Default::default()).unwrap();

    let mut template = program.clone();
    replace(&mut template, TypeExport::Declaration(id));
    let parameter = GenericParam {
        owner: id,
        position: 0,
    }
    .as_type();
    template.modules[1].structures[0].arguments = vec![parameter.clone()];
    template.modules[1].structures[0].fields[0].ty = parameter;
    verify_program(&template, &context.snapshot(), &Default::default()).unwrap();
    let root = context.intern_root(&program.modules[1].identity).unwrap();
    let other = context
        .intern_child(root, DefinitionKind::Function, "unrelated", 0)
        .unwrap();
    template.modules[1].structures[0].arguments[0] = GenericParam {
        owner: other,
        position: 0,
    }
    .as_type();
    assert!(verify_program(&template, &context.snapshot(), &Default::default()).is_err());

    let mut mismatched = program.clone();
    mismatched.modules[1].structures[0].fields[0].ty = boolean.clone();
    assert!(verify_program(&mismatched, &context.snapshot(), &Default::default()).is_err());
    let mut wrong_application = program.clone();
    replace(
        &mut wrong_application,
        TypeExport::Closed(Ty::Struct(application(id, vec![boolean]))),
    );
    assert!(matches!(
        verify_program(&wrong_application, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidProgramExports)
    ));

    let mut nongeneric = program;
    if let PublicItem::Type(record) = &mut nongeneric.modules[1].public_items[0] {
        record.generic_params.clear();
        record.fields[0].ty = int;
    }
    nongeneric.modules[1].structures.clear();
    replace(&mut nongeneric, TypeExport::Declaration(id));
    assert!(matches!(
        verify_program(&nongeneric, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidProgramExports)
    ));
}

#[test]
fn type_exports_cannot_borrow_function_assumptions_for_nominal_bounds() {
    let (context, mut program, id) = fixture();
    let root = context.intern_root(&program.modules[1].identity).unwrap();
    let interface = context
        .intern_child(root, DefinitionKind::Trait, "Required", 0)
        .unwrap();
    program.modules[1]
        .public_items
        .push(PublicItem::Trait(TraitDef {
            name: "Required".into(),
            associated_consts: vec![],
            supertraits: vec![],
            generic_params: vec![],
            bounds: vec![],
            methods: vec![],
            associated_types: vec![],
            conversion_adapter: None,
        }));
    if let PublicItem::Type(record) = &mut program.modules[1].public_items[0] {
        record.bounds.push(GenericBound {
            ty: GenericParam {
                owner: id,
                position: 0,
            }
            .as_type(),
            constraints: vec![application(interface, vec![])],
        });
    }
    replace(
        &mut program,
        TypeExport::Closed(Ty::Struct(application(
            id,
            vec![Ty::Builtin(BuiltinType::I32)],
        ))),
    );
    assert!(matches!(
        verify_program(&program, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidProgramExports)
    ));
}

fn marker_trait(name: &str) -> TraitDef {
    TraitDef {
        name: name.into(),
        associated_consts: vec![],
        supertraits: vec![],
        generic_params: vec![],
        bounds: vec![],
        methods: vec![],
        associated_types: vec![],
        conversion_adapter: None,
    }
}

#[test]
fn type_exports_open_declaration_keeps_private_layout_arguments_private() {
    let (context, mut program, cell) = fixture();
    let root = context.intern_root(&program.modules[1].identity).unwrap();
    let hidden = context
        .intern_child(root, DefinitionKind::Struct, "Hidden", 0)
        .unwrap();
    let argument = Ty::Struct(application(hidden, vec![]));
    program.modules[1].structures[0].arguments = vec![argument.clone()];
    program.modules[1].structures[0].fields[0].ty = argument.clone();
    program.modules[1].structures.push(StructLayout {
        declaration: hidden,
        arguments: vec![],
        fields: vec![],
    });
    program.modules[1].private_types.push(TypeDeclaration {
        declaration: hidden,
        definition: TypeDef {
            name: "Hidden".into(),
            kind: TypeDefKind::Struct,
            generic_params: vec![],
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        },
    });
    // Private declaration evidence admits the specialization without adding a
    // public type path or requiring an exported closed alias to Hidden.
    program.exports = ProgramExports::default();
    verify_program(&program, &context.snapshot(), &Default::default()).unwrap();
    program.exports.namespaces.insert(
        program.modules[0].identity.clone(),
        ExportNamespace::default(),
    );
    replace(&mut program, TypeExport::Declaration(cell));
    verify_program(&program, &context.snapshot(), &Default::default())
        .expect("an open public declaration must admit a checked private specialization");
    let artifact = KbcArtifact::from_program(
        program.clone(),
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let imported = DefinitionContext::new().unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&imported, &Default::default())
        .unwrap()
        .into_records();
    decoded
        .validate_for_loader(&Default::default(), &imported, &Default::default())
        .unwrap();
    assert_eq!(decoded.program.modules[1].private_types.len(), 1);
    assert_eq!(decoded.program.modules[1].public_items.len(), 1);
    assert_eq!(decoded.to_bytes(&imported.snapshot()).unwrap(), bytes);

    let mut mismatched = program.clone();
    let field = context
        .intern_child(hidden, DefinitionKind::Field, "extra", 0)
        .unwrap();
    mismatched.modules[1].structures[1]
        .fields
        .push(StructFieldLayout {
            visibility: Visibility::Private,
            declaration: field,
            name: "extra".into(),
            ty: Ty::Builtin(BuiltinType::I32),
            mutable: false,
        });
    assert!(verify_program(&mismatched, &context.snapshot(), &Default::default()).is_err());
    for export in [
        TypeExport::Declaration(hidden),
        TypeExport::Closed(argument.clone()),
        TypeExport::Closed(Ty::Struct(application(cell, vec![argument]))),
    ] {
        // Prove Cell's private specialization first, then ensure that cached
        // dependency evidence cannot authorize a later public target.
        program
            .exports
            .namespaces
            .values_mut()
            .next()
            .unwrap()
            .types
            .insert("ZHidden".into(), export);
        assert!(matches!(
            verify_program(&program, &context.snapshot(), &Default::default()),
            Err(BytecodeVerificationError::InvalidProgramExports)
        ));
    }
}

#[test]
fn type_exports_check_nested_bounds_in_open_declaration_layouts() {
    let (context, mut program, id) = fixture();
    let root = context.intern_root(&program.modules[1].identity).unwrap();
    let required = context
        .intern_child(root, DefinitionKind::Trait, "Required", 0)
        .unwrap();
    let restricted = context
        .intern_child(root, DefinitionKind::Struct, "Restricted", 0)
        .unwrap();
    let parameter = GenericParam {
        owner: restricted,
        position: 0,
    };
    program.modules[1].public_items.extend([
        PublicItem::Trait(marker_trait("Required")),
        PublicItem::Type(TypeDef {
            name: "Restricted".into(),
            kind: TypeDefKind::Struct,
            generic_params: vec![parameter.clone()],
            bounds: vec![GenericBound {
                ty: parameter.as_type(),
                constraints: vec![application(required, vec![])],
            }],
            fields: vec![],
            variants: vec![],
        }),
    ]);
    let nested = Ty::Struct(application(restricted, vec![Ty::Builtin(BuiltinType::I32)]));
    program.modules[1].structures[0].arguments = vec![nested.clone()];
    program.modules[1].structures[0].fields[0].ty = nested;
    program.modules[1].structures.push(StructLayout {
        declaration: restricted,
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
        fields: vec![],
    });
    replace(&mut program, TypeExport::Declaration(id));
    for private in [false, true] {
        let mut candidate = program.clone();
        if private {
            let PublicItem::Type(record) = candidate.modules[1].public_items.remove(2) else {
                panic!("nested nominal fixture");
            };
            candidate.modules[1].private_types.push(TypeDeclaration {
                declaration: restricted,
                definition: record,
            });
        }
        assert!(
            matches!(
                verify_program(&candidate, &context.snapshot(), &Default::default()),
                Err(BytecodeVerificationError::InvalidProgramExports)
            ),
            "private dependency: {private}"
        );
        if private {
            candidate.modules[1].private_types[0]
                .definition
                .bounds
                .clear();
        } else if let PublicItem::Type(record) = &mut candidate.modules[1].public_items[2] {
            record.bounds.clear();
        }
        verify_program(&candidate, &context.snapshot(), &Default::default()).unwrap();
    }
}

#[test]
fn type_exports_public_trait_alias_can_retain_private_supertrait_proofs() {
    let (context, mut program, _) = fixture();
    let root = context.intern_root(&program.modules[1].identity).unwrap();
    let sealed = context
        .intern_child(root, DefinitionKind::Trait, "Sealed", 0)
        .unwrap();
    let api = context
        .intern_child(root, DefinitionKind::Trait, "Api", 0)
        .unwrap();
    program.modules[1].trait_contracts.push(TraitContract {
        declaration: sealed,
        abi: marker_trait("Sealed"),
    });
    let mut record = marker_trait("Api");
    record.supertraits.push(application(sealed, vec![]));
    program.modules[1]
        .public_items
        .push(PublicItem::Trait(record));
    replace(
        &mut program,
        TypeExport::Closed(Ty::Trait(application(api, vec![]))),
    );
    verify_program(&program, &context.snapshot(), &Default::default()).unwrap();
    for export in [
        TypeExport::Declaration(sealed),
        TypeExport::Closed(Ty::Trait(application(sealed, vec![]))),
    ] {
        replace(&mut program, export);
        assert!(matches!(
            verify_program(&program, &context.snapshot(), &Default::default()),
            Err(BytecodeVerificationError::InvalidProgramExports)
        ));
    }
}

#[test]
fn type_exports_index_many_layouts_and_reuse_alias_proofs_with_bounded_work() {
    let context = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("many_types.kgr");
    let root = context.intern_root(&identity).unwrap();
    let mut items = vec![];
    let mut layouts = vec![];
    for index in 0..32_768 {
        let name = format!("Record{index}");
        let id = context
            .intern_child(root, DefinitionKind::Struct, &name, 0)
            .unwrap();
        items.push(PublicItem::Type(TypeDef {
            name,
            kind: TypeDefKind::Struct,
            generic_params: vec![],
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        }));
        layouts.push(StructLayout {
            declaration: id,
            arguments: vec![],
            fields: vec![],
        });
    }
    let cancel = CancellationToken::default();
    let catalog = ProofCatalog::new(vec![], [], [], [], &context.snapshot(), &cancel).unwrap();
    // Isolate the shared export owner; unrelated graph passes have their own costs.
    let validator = TypeExportValidator::new(
        [TypeExportModule {
            identity: &identity,
            public_items: &items,
            private_types: &[],
            structures: &layouts,
        }],
        &catalog,
        &cancel,
    )
    .unwrap();
    let last = layouts.last().unwrap().declaration;
    let declaration = TypeExport::Declaration(last);
    let closed = TypeExport::Closed(Ty::Struct(application(last, vec![])));
    for _ in 0..32_768 {
        validator.validate(&declaration).unwrap();
        validator.validate(&closed).unwrap();
    }
    let wide = TypeExport::Closed(Ty::Tuple(vec![Ty::Builtin(BuiltinType::Unit); 4095]));
    let mut exhausted = false;
    for _ in 0..2048 {
        if validator.validate(&wide) == Err(TypeTransformError::LimitExceeded) {
            exhausted = true;
            break;
        }
    }
    assert!(
        exhausted,
        "one shared budget must bound repeated wide aliases"
    );
    cancel.cancel();
    assert_eq!(
        validator.validate(&closed),
        Err(TypeTransformError::Cancelled)
    );
    assert!(matches!(
        TypeExportValidator::new(
            [TypeExportModule {
                identity: &identity,
                public_items: &items,
                private_types: &[],
                structures: &layouts,
            }],
            &catalog,
            &cancel
        ),
        Err(TypeTransformError::Cancelled)
    ));
}

#[test]
fn type_exports_bytecode_preserves_cancellation_inside_leaf_proof() {
    let (context, program, _) = fixture();
    let cancel = CancellationToken::default();
    let definitions = context.snapshot();
    let catalog = ProofCatalog::new(vec![], [], [], [], &definitions, &cancel).unwrap();
    CANCEL_TYPE_PROOF.set(true);
    // Call the export boundary directly: the outer graph verifier's cancellation
    // check must not conceal a lost cancellation error in this callback.
    let result = super::exports::verify(&program, &definitions, &catalog, &cancel);
    assert!(matches!(result, Err(BytecodeVerificationError::Cancelled)));
    assert!(!CANCEL_TYPE_PROOF.get());
}

#[test]
fn type_exports_keep_native_storage_distinct_from_script_structs() {
    let context = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("native.kgr");
    let root = context.intern_root(&identity).unwrap();
    let id = context
        .intern_child(root, DefinitionKind::AssociatedType, "Object", 0)
        .unwrap();
    let mut program = BytecodeProgram {
        update_evidence: crate::program::updates::empty(),
        root: ModuleRef::new(0),
        exports: ProgramExports {
            namespaces: [(identity.clone(), ExportNamespace::default())].into(),
            entry: None,
        },
        modules: vec![BytecodeModule {
            identity,
            public_items: vec![PublicItem::Type(TypeDef {
                name: "Object".into(),
                kind: TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
                generic_params: vec![],
                bounds: vec![],
                fields: vec![],
                variants: vec![],
            })],
            ..Default::default()
        }],
    };
    for export in [
        TypeExport::Declaration(id),
        TypeExport::Closed(Ty::NativeObject(application(id, vec![]))),
    ] {
        replace(&mut program, export);
        verify_program(&program, &context.snapshot(), &Default::default()).unwrap();
    }
    replace(
        &mut program,
        TypeExport::Closed(Ty::Struct(application(id, vec![]))),
    );
    assert!(matches!(
        verify_program(&program, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidProgramExports)
    ));
}
