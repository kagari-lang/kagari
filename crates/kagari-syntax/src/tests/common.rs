use kagari_common::SourceFile;

use crate::{
    Parse, ast,
    ast::{ConstDef, EnumDef, FnDef, Item, StaticDef, StructDef},
    parse as parse_source, parse_module,
};

pub fn source(text: &str) -> SourceFile {
    SourceFile::new("test.kg", text)
}

pub fn parse_ok(text: &str) -> ast::SourceFile {
    let source = source(text);
    parse_module(&source).expect("source should parse")
}

pub fn parse(text: &str) -> Parse {
    let source = source(text);
    parse_source(&source)
}

pub fn first_function(module: &ast::SourceFile) -> FnDef {
    match module.items().next().expect("expected one item") {
        Item::FnDef(function) => function,
        other => panic!("expected function item, got {other:?}"),
    }
}

pub fn first_struct(module: &ast::SourceFile) -> StructDef {
    match module.items().next().expect("expected one item") {
        Item::StructDef(struct_def) => struct_def,
        other => panic!("expected struct item, got {other:?}"),
    }
}

pub fn first_const(module: &ast::SourceFile) -> ConstDef {
    match module.items().next().expect("expected one item") {
        Item::ConstDef(const_def) => const_def,
        other => panic!("expected const item, got {other:?}"),
    }
}

pub fn first_static(module: &ast::SourceFile) -> StaticDef {
    match module.items().next().expect("expected one item") {
        Item::StaticDef(static_def) => static_def,
        other => panic!("expected static item, got {other:?}"),
    }
}

pub fn first_enum(module: &ast::SourceFile) -> EnumDef {
    match module.items().next().expect("expected one item") {
        Item::EnumDef(enum_def) => enum_def,
        other => panic!("expected enum item, got {other:?}"),
    }
}
