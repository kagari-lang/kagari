use crate::{bytecode::lower_program_to_bytecode, tests::common};
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};
use kagari_contract::types::PublicItem;
use kagari_mir::{
    codec::{decode_program, encode_program},
    program::verify_program as verify_mir_program,
};
use kagari_types::{scalar::BuiltinType, ty::Ty, visibility::Visibility};

const SOURCE: &str = r#"
pub struct Player {
    val secret: i32,
    pub(super) val internal: i32,
    pub var hp: i32,
}
pub fn make() -> Player { Player { secret: 1, internal: 2, hp: 100 } }
"#;

#[test]
fn field_visibility_survives_source_free_artifacts() {
    let mir = common::mir_ok(SOURCE);
    let bytes = encode_program(&mir, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&decoded).unwrap();
    let artifact = KbcArtifact::from_program(bytecode, Default::default()).unwrap();
    let bytes = artifact.to_bytes().unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .into_verified(&Default::default())
        .unwrap();
    let program = decoded.bytecode().program();
    let module = &program.modules[program.root.index()];
    let layout = module
        .structures
        .iter()
        .find(|s| s.fields.len() == 3)
        .unwrap();
    let expected = [
        Visibility::Private,
        Visibility::PublicSuper,
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
    let original = common::bytecode_ok(SOURCE);
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
                .find(|s| s.fields.len() == 3)
                .unwrap();
            layout.fields[0].visibility = Visibility::Public;
        }
        assert!(verify_program(&forged).is_err());
        assert!(KbcArtifact::from_program(forged, Default::default()).is_err());
    }
}

const METHODS: &str = r#"
pub struct Player { pub var hp: i32 }
impl Player {
    pub fn damage(self, amount: i32) -> i32 { self.hp = self.hp - amount; self.hp }
    fn secret(self) -> i32 { self.hp }
    pub(super) fn internal(self) -> i32 { self.secret() }
    pub fn create(hp: i32) -> Player { Player { hp: hp } }
}
pub fn main() -> i32 { val player = Player::create(100); player.damage(20) + player.internal() }
"#;

#[test]
fn inherent_visibility_and_receiver_survive_source_free_artifacts() {
    let mir = common::mir_ok(METHODS);
    let decoded = decode_program(
        &encode_program(&mir, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let bytecode = lower_program_to_bytecode(&decoded).unwrap();
    let artifact = KbcArtifact::from_program(bytecode, Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
        .unwrap()
        .into_verified(&Default::default())
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
    let original = common::bytecode_ok(METHODS);
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
        assert!(verify_program(&forged).is_err());
        assert!(KbcArtifact::from_program(forged, Default::default()).is_err());
    }
    let mir = common::mir_ok(METHODS);
    let root = mir.root().clone();
    let mut modules = mir.into_unverified();
    let module = modules
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
    assert!(verify_mir_program(root, modules, &Default::default()).is_err());
}

#[test]
fn native_inherent_exports_cannot_redefine_registered_signatures() {
    let original = common::bytecode_ok("pub fn main() -> usize { \"abc\".len() }");
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
        assert!(verify_program(&forged).is_err());
        assert!(KbcArtifact::from_program(forged, Default::default()).is_err());
    }
}

#[test]
fn same_named_inherent_members_keep_distinct_owners_and_private_changes_stay_private() {
    let source = r#"
        pub struct Left { val value: i32 }
        pub struct Right { val value: bool }
        impl Left { pub fn get(self) -> i32 { self.value } fn private(self) -> i32 { 1 } }
        impl Right { pub fn get(self) -> bool { self.value } }
        pub fn main() -> (i32, bool) { (Left { value: 1 }.get(), Right { value: true }.get()) }
    "#;
    let original = common::bytecode_ok(source);
    let changed = common::bytecode_ok(&source.replace(
        "private(self) -> i32 { 1 }",
        "private(self) -> bool { true }",
    ));
    let items = &original.modules[original.root.index()].public_items;
    assert_eq!(items, &changed.modules[changed.root.index()].public_items);
    let tables = items
        .iter()
        .filter(|item| matches!(item, PublicItem::InherentTable(_)))
        .collect::<Vec<_>>();
    assert_eq!(tables.len(), 2);
    assert_ne!(tables[0].fingerprint_name(), tables[1].fingerprint_name());
    let mut forged = original.clone();
    forged.modules[forged.root.index()]
        .public_items
        .push(tables[0].clone());
    assert!(verify_program(&forged).is_err());
}
