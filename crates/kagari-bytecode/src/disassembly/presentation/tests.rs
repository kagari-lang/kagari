//! Contract tests for source-free presentation vocabulary and retained scopes.

use crate::disassembly::presentation::{DisplayContext, effects, physical, quoted};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId,
    table::{DefinitionId, DefinitionTableBuilder},
};
use kagari_contract::effects::EffectSet;
use kagari_types::{
    collection::CollectionAccess,
    range::RangeKind,
    reference::ReferenceAccess,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::collections::BTreeMap;

fn module() -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("game".into()),
        path: vec!["model".into()],
    }
}

fn child(
    definitions: &mut DefinitionTableBuilder,
    owner: DefinitionId,
    kind: DefinitionKind,
    name: &str,
) -> DefinitionId {
    definitions.intern_child(owner, kind, name, 0).unwrap()
}

fn i32_ty() -> Ty {
    Ty::Builtin(BuiltinType::I32)
}

#[test]
fn identity_and_associated_binding_order_are_independent_of_interning_order() {
    let render = |reverse: bool| {
        let mut definitions = DefinitionTableBuilder::new().unwrap();
        let root = definitions.intern_root(&module()).unwrap();
        let owner = child(&mut definitions, root, DefinitionKind::Trait, "Convert");
        let names = if reverse { ["Z", "A"] } else { ["A", "Z"] };
        let mut bindings = BTreeMap::new();
        for name in names {
            let id = child(
                &mut definitions,
                owner,
                DefinitionKind::AssociatedType,
                name,
            );
            bindings.insert(id, i32_ty());
        }
        let duplicate = definitions
            .intern_child(root, DefinitionKind::Function, "generated", 2)
            .unwrap();
        let table = definitions.freeze();
        let display = DisplayContext {
            definitions: &table,
        };
        (
            display.module(&module()),
            display.definition(duplicate),
            display.ty(&Ty::Trait(NominalTy {
                declaration: owner,
                arguments: vec![i32_ty()],
                associated_types: bindings,
            })),
        )
    };
    let expected = (
        "game::model".to_owned(),
        "game::model::generated#2".to_owned(),
        "trait game::model::Convert<i32, game::model::Convert::A = i32, game::model::Convert::Z = i32>"
            .to_owned(),
    );
    assert_eq!(render(false), expected);
    assert_eq!(render(true), expected);
}

#[test]
fn generic_projection_preserves_owner_receiver_interface_member_and_arguments() {
    let mut definitions = DefinitionTableBuilder::new().unwrap();
    let root = definitions.intern_root(&module()).unwrap();
    let owner = child(&mut definitions, root, DefinitionKind::Function, "convert");
    let interface = child(&mut definitions, root, DefinitionKind::Trait, "Convert");
    let member = child(
        &mut definitions,
        interface,
        DefinitionKind::AssociatedType,
        "Output",
    );
    let table = definitions.freeze();
    let display = DisplayContext {
        definitions: &table,
    };
    let ty = Ty::Projection {
        arguments: vec![Ty::SelfType(interface), Ty::Tuple(vec![i32_ty()])],
        receiver: Box::new(Ty::Parameter { owner, position: 3 }),
        interface: Box::new(NominalTy {
            declaration: interface,
            arguments: vec![Ty::Builtin(BuiltinType::String)],
            associated_types: BTreeMap::from([(member, i32_ty())]),
        }),
        member,
    };
    assert_eq!(
        display.ty(&ty),
        "<parameter<game::model::convert, 3> as game::model::Convert<String, game::model::Convert::Output = i32>>::game::model::Convert::Output<Self<game::model::Convert>, (i32,)>"
    );
}

#[test]
fn nominal_categories_and_host_identity_remain_explicit() {
    let mut definitions = DefinitionTableBuilder::new().unwrap();
    let root = definitions.intern_root(&module()).unwrap();
    let declaration = child(&mut definitions, root, DefinitionKind::Struct, "Item");
    let nominal = NominalTy {
        declaration,
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let table = definitions.freeze();
    let display = DisplayContext {
        definitions: &table,
    };
    for (ty, expected) in [
        (Ty::Struct(nominal.clone()), "struct game::model::Item"),
        (
            Ty::NativeObject(nominal.clone()),
            "native_object game::model::Item",
        ),
        (Ty::Enum(nominal.clone()), "enum game::model::Item"),
        (Ty::Trait(nominal), "trait game::model::Item"),
        (Ty::Host(declaration), "host game::model::Item"),
    ] {
        assert_eq!(display.ty(&ty), expected);
    }
}

#[test]
fn containers_functions_and_access_modes_keep_all_type_facts() {
    let table = DefinitionTableBuilder::new().unwrap().freeze();
    let display = DisplayContext {
        definitions: &table,
    };
    let ty = Ty::Function {
        params: vec![
            Ty::Array(Box::new(i32_ty())),
            Ty::Iter(Box::new(Ty::Tuple(vec![]))),
            Ty::Tuple(vec![i32_ty(), Ty::Builtin(BuiltinType::Bool)]),
        ],
        result: Box::new(Ty::Builtin(BuiltinType::Never)),
    };
    assert_eq!(display.ty(&ty), "fn([i32], Iter<()>, (i32, bool)) -> !");
    for (access, expected) in [
        (CollectionAccess::ReadOnly, "readonly"),
        (CollectionAccess::Mutable, "mutable"),
    ] {
        assert_eq!(
            display.ty(&Ty::Map {
                key: Box::new(i32_ty()),
                value: Box::new(Ty::Builtin(BuiltinType::String)),
                access,
            }),
            format!("Map<i32, String, access={expected}>")
        );
        assert_eq!(
            display.ty(&Ty::Set(Box::new(i32_ty()), access)),
            format!("Set<i32, access={expected}>")
        );
    }
    for (access, expected) in [
        (ReferenceAccess::Shared, "Ref<i32>"),
        (ReferenceAccess::Mutable, "RefMut<i32>"),
    ] {
        assert_eq!(
            display.ty(&Ty::Reference {
                target: Box::new(i32_ty()),
                access,
            }),
            expected
        );
    }
    for (kind, expected) in [
        (RangeKind::Exclusive, "Range<i32>"),
        (RangeKind::Inclusive, "RangeInclusive<i32>"),
        (RangeKind::From, "RangeFrom<i32>"),
        (RangeKind::To, "RangeTo<i32>"),
        (RangeKind::ToInclusive, "RangeToInclusive<i32>"),
        (RangeKind::Full, "RangeFull<i32>"),
    ] {
        assert_eq!(display.ty(&Ty::Range(Box::new(i32_ty()), kind)), expected);
    }
}

#[test]
fn semantic_builtins_and_physical_representations_use_separate_vocabularies() {
    let table = DefinitionTableBuilder::new().unwrap().freeze();
    let display = DisplayContext {
        definitions: &table,
    };
    for (builtin, expected) in [
        (BuiltinType::Never, "!"),
        (BuiltinType::Unit, "()"),
        (BuiltinType::Bool, "bool"),
        (BuiltinType::I8, "i8"),
        (BuiltinType::I16, "i16"),
        (BuiltinType::I32, "i32"),
        (BuiltinType::I64, "i64"),
        (BuiltinType::U8, "u8"),
        (BuiltinType::U16, "u16"),
        (BuiltinType::U32, "u32"),
        (BuiltinType::U64, "u64"),
        (BuiltinType::F32, "f32"),
        (BuiltinType::F64, "f64"),
        (BuiltinType::String, "String"),
        (BuiltinType::Char, "char"),
    ] {
        assert_eq!(display.ty(&Ty::Builtin(builtin)), expected);
    }
    for (ty, expected) in [
        (ValueType::Never, "never"),
        (ValueType::Unit, "unit"),
        (ValueType::Bool, "bool"),
        (ValueType::I32, "i32"),
        (ValueType::I64, "i64"),
        (ValueType::U64, "u64"),
        (ValueType::F32, "f32"),
        (ValueType::F64, "f64"),
        (ValueType::Str, "str"),
        (ValueType::HeapObject, "heap_object"),
        (ValueType::HostHandle, "host_handle"),
        (ValueType::Generic, "generic"),
        (ValueType::Char, "char"),
    ] {
        assert_eq!(physical(ty), expected);
    }
}

#[test]
fn recorded_effects_have_fixed_order_and_an_explicit_empty_set() {
    assert_eq!(effects(EffectSet::default()), "[]");
    assert_eq!(
        effects(EffectSet {
            reads_local: true,
            writes_local: true,
            reads_module: true,
            writes_module: true,
            reads_aggregate: true,
            writes_aggregate: true,
            reads_path: true,
            writes_path: true,
            allocates: true,
            calls: true,
            touches_runtime: true,
            may_trap: true,
            may_suspend: true,
        }),
        "[reads_local, writes_local, reads_module, writes_module, reads_aggregate, writes_aggregate, reads_path, writes_path, allocates, calls, touches_runtime, may_trap, may_suspend]"
    );
    assert_eq!(
        effects(EffectSet {
            writes_local: true,
            may_suspend: true,
            ..EffectSet::default()
        }),
        "[writes_local, may_suspend]"
    );
}

#[test]
fn quoted_text_preserves_unicode_but_cannot_inject_lines_or_terminal_controls() {
    assert_eq!(quoted("你好 café e\u{301} 🦀"), "\"你好 café e\u{301} 🦀\"");
    assert_eq!(
        quoted("\"\\\n\r\t\0\u{1b}[31m\u{85}"),
        "\"\\\"\\\\\\n\\r\\t\\0\\u{1b}[31m\\u{85}\""
    );
    for ch in [
        '\u{ad}',
        '\u{600}',
        '\u{61c}',
        '\u{6dd}',
        '\u{70f}',
        '\u{890}',
        '\u{8e2}',
        '\u{180e}',
        '\u{200b}',
        '\u{200e}',
        '\u{2028}',
        '\u{2029}',
        '\u{202e}',
        '\u{2060}',
        '\u{2066}',
        '\u{2069}',
        '\u{feff}',
        '\u{fff9}',
        '\u{110bd}',
        '\u{110cd}',
        '\u{13430}',
        '\u{1343f}',
        '\u{1bca0}',
        '\u{1d173}',
        '\u{e0001}',
        '\u{e0020}',
        '\u{e007f}',
    ] {
        assert_eq!(
            quoted(&ch.to_string()),
            format!("\"\\u{{{:x}}}\"", u32::from(ch))
        );
    }
}
