use crate::{bytecode::lower_program_to_bytecode, tests::common};
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_contract::{
    statics::{StaticContract, StaticDescriptor},
    types::{PublicItem, TraitContract},
};
use kagari_mir::{
    codec::{decode_program, encode_program},
    program::verify_program as verify_mir_program,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, TraitDef},
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};

const SOURCE: &str = r#"
pub struct Player {
    val secret: i32,
    pub(super) val internal: i32,
    pub(crate) val package_value: i32,
    pub var hp: i32,
}
pub fn make() -> Player { Player { secret: 1, internal: 2, package_value: 3, hp: 100 } }
"#;

#[test]
fn field_visibility_survives_source_free_artifacts() {
    let context = DefinitionContext::new().unwrap();
    let mir = common::mir_ok(SOURCE);
    let bytes = encode_program(&mir, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&decoded, &Default::default())
        .map(|program| program.into_unverified())
        .map(|metadata| common::adopt_bytecode(&context, metadata))
        .unwrap();
    let artifact =
        KbcArtifact::from_program(bytecode, Default::default(), &context, &Default::default())
            .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes)
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap()
        .into_verified(&Default::default(), &context, &Default::default())
        .unwrap();
    let program = decoded.bytecode().program();
    let module = &program.modules[program.root.index()];
    let layout = module
        .structures
        .iter()
        .find(|s| s.fields.len() == 4)
        .unwrap();
    let expected = [
        Visibility::Private,
        Visibility::PublicSuper,
        Visibility::PublicCrate,
        Visibility::Public,
    ];
    assert_eq!(
        layout
            .fields
            .iter()
            .map(|f| f.visibility)
            .collect::<Vec<_>>(),
        expected
    );
    let declaration = module
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::Type(ty) if ty.name == "Player" => Some(ty),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        declaration
            .fields
            .iter()
            .map(|f| f.visibility)
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn executable_and_declared_field_access_must_agree() {
    let context = DefinitionContext::new().unwrap();
    let original = common::adopt_bytecode(&context, common::bytecode_ok(SOURCE));
    for change_declaration in [false, true] {
        let mut forged = original.clone();
        let module = &mut forged.modules[forged.root.index()];
        if change_declaration {
            let declaration = module
                .public_items
                .iter_mut()
                .find_map(|item| match item {
                    PublicItem::Type(ty) if ty.name == "Player" => Some(ty),
                    _ => None,
                })
                .unwrap();
            declaration.fields[0].visibility = Visibility::Public;
        } else {
            let layout = module
                .structures
                .iter_mut()
                .find(|s| s.fields.len() == 4)
                .unwrap();
            layout.fields[0].visibility = Visibility::Public;
        }
        assert!(verify_program(&forged, &context.snapshot(), &Default::default()).is_err());
        assert!(
            KbcArtifact::from_program(forged, Default::default(), &context, &Default::default())
                .is_err()
        );
    }
}

const METHODS: &str = r#"
pub struct Player { pub var hp: i32 }
impl Player {
    pub fn damage(self, amount: i32) -> i32 { self.hp = self.hp - amount; self.hp }
    fn secret(self) -> i32 { self.hp }
    pub(super) fn internal(self) -> i32 { self.secret() }
    pub(crate) fn package_only(self) -> i32 { self.hp }
    pub fn create(hp: i32) -> Player { Player { hp: hp } }
}
pub fn main() -> i32 { val player = Player::create(100); player.damage(20) + player.internal() }
"#;

#[test]
fn inherent_visibility_and_receiver_survive_source_free_artifacts() {
    let context = DefinitionContext::new().unwrap();
    let mir = common::mir_ok(METHODS);
    let decoded = decode_program(
        &encode_program(&mir, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let bytecode = lower_program_to_bytecode(&decoded, &Default::default())
        .map(|program| program.into_unverified())
        .map(|metadata| common::adopt_bytecode(&context, metadata))
        .unwrap();
    let artifact =
        KbcArtifact::from_program(bytecode, Default::default(), &context, &Default::default())
            .unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap()
        .into_verified(&Default::default(), &context, &Default::default())
        .unwrap();
    let program = decoded.bytecode().program();
    let module = &program.modules[program.root.index()];
    let tables = module
        .public_items
        .iter()
        .filter_map(|item| match item {
            PublicItem::InherentTable(table) => Some(table),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tables.len(), 1);
    let table = tables[0];
    assert_eq!(
        table
            .methods
            .iter()
            .map(|method| method.name.as_str())
            .collect::<Vec<_>>(),
        ["damage", "create"]
    );
    assert_eq!(table.methods[0].params[0].ty, table.for_type);
    assert_eq!(table.methods[1].return_type, table.for_type);
}

#[test]
fn inherent_declaration_receiver_and_executable_signature_are_checked() {
    let context = DefinitionContext::new().unwrap();
    let original = common::adopt_bytecode(&context, common::bytecode_ok(METHODS));
    for change_receiver in [false, true] {
        let mut forged = original.clone();
        let module = &mut forged.modules[forged.root.index()];
        let table = module
            .public_items
            .iter_mut()
            .find_map(|item| match item {
                PublicItem::InherentTable(table) => Some(table),
                _ => None,
            })
            .unwrap();
        if change_receiver {
            table.for_type = Ty::Builtin(BuiltinType::I32);
        } else {
            table.methods[0].params[1].ty = Ty::Builtin(BuiltinType::Bool);
        }
        assert!(verify_program(&forged, &context.snapshot(), &Default::default()).is_err());
        assert!(
            KbcArtifact::from_program(forged, Default::default(), &context, &Default::default())
                .is_err()
        );
    }
    let mir = common::mir_ok(METHODS);
    let root = mir.root().clone();
    let mut modules = common::adopt(&context, mir.into_unverified());
    let module = modules
        .modules
        .iter_mut()
        .find(|module| module.identity == root)
        .unwrap();
    let table = module
        .abi
        .public_items
        .iter_mut()
        .find_map(|item| match item {
            PublicItem::InherentTable(table) => Some(table),
            _ => None,
        })
        .unwrap();
    table.methods[0].return_type = Ty::Builtin(BuiltinType::Bool);
    assert!(verify_mir_program(modules, &context, &Default::default()).is_err());
}

#[test]
fn initializer_cannot_alias_inherent_export_without_call_sites() {
    reject_callable_initializer(false);
}

#[test]
fn initializer_cannot_alias_trait_default_without_call_sites() {
    reject_callable_initializer(true);
}

#[test]
fn initializer_privacy_uses_declaration_identity_not_diagnostic_name() {
    let context = DefinitionContext::new().unwrap();
    let mir = common::mir_ok("pub fn make() -> i32 { 42 }");
    let root = mir.root().clone();
    let mut bytecode = common::adopt_bytecode(
        &context,
        lower_program_to_bytecode(&mir, &Default::default())
            .unwrap()
            .into_unverified(),
    );
    let mut records = common::adopt(&context, mir.into_unverified());
    records.exports = Default::default();
    bytecode.exports = Default::default();
    let module = records
        .modules
        .iter_mut()
        .find(|module| module.identity == root)
        .unwrap();
    let initializer = module
        .functions
        .iter_mut()
        .find(|function| function.name == "make")
        .unwrap();
    let initializer_id = initializer.id;
    initializer.name = "hidden_diagnostic_name".into();
    let module = &mut bytecode.modules[bytecode.root.index()];
    let initializer = module
        .functions
        .iter_mut()
        .find(|function| function.name == "make")
        .unwrap();
    let bytecode_initializer_id = initializer.id;
    initializer.name = "hidden_diagnostic_name".into();
    module.function_table[bytecode_initializer_id.index()].name = initializer.name.clone();
    // Diagnostic labels and export paths cannot determine ABI callable authority.
    verify_mir_program(records.clone(), &context, &Default::default()).unwrap();
    verify_program(&bytecode, &context.snapshot(), &Default::default()).unwrap();
    let contract = StaticContract {
        declaration: context
            .intern_child(
                context.intern_root(&root).unwrap(),
                DefinitionKind::Static,
                "state",
                0,
            )
            .unwrap(),
        supplier: root.clone(),
        name: "state".into(),
        ty: Ty::Builtin(BuiltinType::I32),
        mutable: false,
        visibility: Visibility::Private,
    };
    records
        .modules
        .iter_mut()
        .find(|module| module.identity == root)
        .unwrap()
        .module_slots
        .push(StaticDescriptor {
            contract: contract.clone(),
            initializer: initializer_id,
        });
    bytecode.modules[bytecode.root.index()]
        .module_slots
        .push(StaticDescriptor {
            contract,
            initializer: bytecode_initializer_id,
        });
    let mir_rejected = verify_mir_program(records, &context, &Default::default()).is_err();
    let bytecode_rejected =
        verify_program(&bytecode, &context.snapshot(), &Default::default()).is_err();
    assert!(
        mir_rejected && bytecode_rejected,
        "public declaration accepted as initializer: MIR rejected={mir_rejected}, bytecode rejected={bytecode_rejected}"
    );
}

fn reject_callable_initializer(trait_default: bool) {
    let context = DefinitionContext::new().unwrap();
    let mir = common::mir_ok("pub struct Widget {} impl Widget { pub fn make() -> i32 { 42 } }");
    let root = mir.root().clone();
    let mut bytecode = common::adopt_bytecode(
        &context,
        lower_program_to_bytecode(&mir, &Default::default())
            .unwrap()
            .into_unverified(),
    );
    let mut records = common::adopt(&context, mir.into_unverified());
    if trait_default {
        let module = records
            .modules
            .iter_mut()
            .find(|module| module.identity == root)
            .unwrap();
        let root_id = context.intern_root(&root).unwrap();
        let declaration = context
            .intern_child(root_id, DefinitionKind::Trait, "Factory", 0)
            .unwrap();
        let method = context
            .intern_child(declaration, DefinitionKind::Method, "make", 0)
            .unwrap();
        let contract = TraitContract {
            declaration,
            abi: TraitDef {
                name: "Factory".into(),
                generic_params: vec![],
                bounds: vec![],
                supertraits: vec![],
                associated_consts: vec![],
                associated_types: vec![],
                conversion_adapter: None,
                methods: vec![FnDecl {
                    name: "make".into(),
                    implementation: CallableImplementation::Script,
                    host_policy: None,
                    method_policy: Default::default(),
                    generic_params: vec![],
                    bounds: vec![],
                    params: vec![],
                    return_type: Ty::Builtin(BuiltinType::I32),
                }],
            },
        };
        module
            .abi
            .public_items
            .retain(|item| !matches!(item, PublicItem::InherentTable(_)));
        module.abi.trait_contracts.push(contract.clone());
        let function = module
            .functions
            .iter_mut()
            .find(|function| {
                context
                    .snapshot()
                    .resolve(function.instance.declaration)
                    .unwrap()
                    .last()
                    .is_some_and(|part| part.kind == DefinitionKind::Method && part.name == "make")
            })
            .unwrap();
        function.instance.declaration = method;
        let module = &mut bytecode.modules[bytecode.root.index()];
        module
            .public_items
            .retain(|item| !matches!(item, PublicItem::InherentTable(_)));
        module.trait_contracts.push(contract);
        let function = module
            .functions
            .iter_mut()
            .find(|function| {
                function.identity.as_ref().is_some_and(|identity| {
                    context
                        .snapshot()
                        .resolve(identity.declaration)
                        .unwrap()
                        .last()
                        .is_some_and(|part| {
                            part.kind == DefinitionKind::Method && part.name == "make"
                        })
                })
            })
            .unwrap();
        function.identity.as_mut().unwrap().declaration = method;
        module.function_table[function.id.index()].identity = function.identity.clone();
    }
    // Both ordinary callable forms are valid before assigning initializer authority.
    verify_mir_program(records.clone(), &context, &Default::default()).unwrap();
    verify_program(&bytecode, &context.snapshot(), &Default::default()).unwrap();
    let module = records
        .modules
        .iter_mut()
        .find(|module| module.identity == root)
        .unwrap();
    let target = module
        .functions
        .iter()
        .find(|function| {
            context
                .snapshot()
                .resolve(function.instance.declaration)
                .unwrap()
                .last()
                .is_some_and(|part| part.kind == DefinitionKind::Method && part.name == "make")
        })
        .unwrap();
    let initializer = target.id;
    assert!(target.params.is_empty());
    let root_id = context.intern_root(&module.identity).unwrap();
    let contract = StaticContract {
        declaration: context
            .intern_child(root_id, DefinitionKind::Static, "state", 0)
            .unwrap(),
        supplier: module.identity.clone(),
        name: "state".into(),
        ty: Ty::Builtin(BuiltinType::I32),
        mutable: false,
        visibility: Visibility::Private,
    };
    module.module_slots.push(StaticDescriptor {
        contract: contract.clone(),
        initializer,
    });
    let module = &mut bytecode.modules[bytecode.root.index()];
    let initializer = module
        .functions
        .iter()
        .find(|function| {
            function.identity.as_ref().is_some_and(|identity| {
                context
                    .snapshot()
                    .resolve(identity.declaration)
                    .unwrap()
                    .last()
                    .is_some_and(|part| part.kind == DefinitionKind::Method && part.name == "make")
            })
        })
        .unwrap()
        .id;
    module.module_slots.push(StaticDescriptor {
        contract,
        initializer,
    });
    let mir_rejected = verify_mir_program(records, &context, &Default::default()).is_err();
    let bytecode_rejected =
        verify_program(&bytecode, &context.snapshot(), &Default::default()).is_err();
    assert!(
        mir_rejected && bytecode_rejected,
        "ordinary callable accepted as initializer: MIR rejected={mir_rejected}, bytecode rejected={bytecode_rejected}"
    );
}

#[test]
fn native_inherent_exports_cannot_redefine_registered_signatures() {
    let context = DefinitionContext::new().unwrap();
    let original = common::adopt_bytecode(
        &context,
        common::bytecode_ok("pub fn main() -> u64 { \"abc\".len() }"),
    );
    for corruption in 0..4 {
        let mut forged = original.clone();
        let (module, table) = forged
            .modules
            .iter_mut()
            .find_map(|module| {
                let table = module.public_items.iter().position(|item| {
                    matches!(item, PublicItem::InherentTable(table)
                    if table.for_type == Ty::Builtin(BuiltinType::String))
                })?;
                Some((module, table))
            })
            .unwrap();
        let PublicItem::InherentTable(table) = &mut module.public_items[table] else {
            unreachable!()
        };
        let method = table
            .methods
            .iter_mut()
            .find(|method| method.name == "len")
            .unwrap();
        match corruption {
            0 => method.return_type = Ty::Builtin(BuiltinType::Bool),
            1 => method.params[0].name = "ordinary_parameter".into(),
            2 => method.params[0].mutable = !method.params[0].mutable,
            _ => module
                .native_declarations
                .retain(|declaration| declaration.function.name != "len"),
        }
        assert!(verify_program(&forged, &context.snapshot(), &Default::default()).is_err());
        assert!(
            KbcArtifact::from_program(forged, Default::default(), &context, &Default::default())
                .is_err()
        );
    }
}

#[test]
fn same_named_inherent_members_keep_distinct_owners_and_private_changes_stay_private() {
    let context = DefinitionContext::new().unwrap();
    let source = r#"
        pub struct Left { val value: i32 }
        pub struct Right { val value: bool }
        impl Left { pub fn get(self) -> i32 { self.value } fn private(self) -> i32 { 1 } }
        impl Right { pub fn get(self) -> bool { self.value } }
        pub fn main() -> (i32, bool) { (Left { value: 1 }.get(), Right { value: true }.get()) }
    "#;
    let original = common::adopt_bytecode(&context, common::bytecode_ok(source));
    let changed = common::adopt_bytecode(
        &context,
        common::bytecode_ok(&source.replace(
            "private(self) -> i32 { 1 }",
            "private(self) -> bool { true }",
        )),
    );
    let items = &original.modules[original.root.index()].public_items;
    assert_eq!(items, &changed.modules[changed.root.index()].public_items);
    let tables = items
        .iter()
        .filter(|item| matches!(item, PublicItem::InherentTable(_)))
        .collect::<Vec<_>>();
    assert_eq!(tables.len(), 2);
    assert_ne!(
        tables[0].fingerprint_name(&context.snapshot()),
        tables[1].fingerprint_name(&context.snapshot())
    );
    let mut forged = original.clone();
    forged.modules[forged.root.index()]
        .public_items
        .push(tables[0].clone());
    assert!(verify_program(&forged, &context.snapshot(), &Default::default()).is_err());
}
