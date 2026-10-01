use crate::{
    ast::item::{ConstDef, EnumDef, FnDef, Item, SourceFile as AstSourceFile, StructDef},
    parser::{Parse, parse as parse_source, parse_module},
};
use kagari_common::source::SourceFile;

pub fn source(text: &str) -> SourceFile {
    SourceFile::new("test.kg", text)
}

pub fn parse_ok(text: &str) -> AstSourceFile {
    let source = source(text);
    parse_module(&source).expect("source should parse")
}

pub fn parse(text: &str) -> Parse {
    let source = source(text);
    parse_source(&source)
}

pub fn first_function(module: &AstSourceFile) -> FnDef {
    match module.items().next().expect("expected one item") {
        Item::FnDef(function) => function,
        other => panic!("expected function item, got {other:?}"),
    }
}

pub fn first_struct(module: &AstSourceFile) -> StructDef {
    match module.items().next().expect("expected one item") {
        Item::StructDef(struct_def) => struct_def,
        other => panic!("expected struct item, got {other:?}"),
    }
}

pub fn first_const(module: &AstSourceFile) -> ConstDef {
    match module.items().next().expect("expected one item") {
        Item::ConstDef(const_def) => const_def,
        other => panic!("expected const item, got {other:?}"),
    }
}

pub fn first_enum(module: &AstSourceFile) -> EnumDef {
    match module.items().next().expect("expected one item") {
        Item::EnumDef(enum_def) => enum_def,
        other => panic!("expected enum item, got {other:?}"),
    }
}
