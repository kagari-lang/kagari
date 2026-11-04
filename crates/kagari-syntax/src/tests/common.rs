use crate::{
    ast::{
        borrow::Ast,
        item::{ConstDef, EnumDef, FnDef, StructDef},
        module::ModuleSyntax,
        ty::TypeSyntax,
        views::{ItemView, TypeView},
    },
    parser::{Parse, parse as parse_source, parse_module},
};
use kagari_source::source::SourceFile;

pub fn source(text: &str) -> SourceFile {
    SourceFile::new("test.kg", text)
}

pub fn parse_ok(text: &str) -> ModuleSyntax {
    parse_module(&source(text)).expect("source should parse")
}

pub fn parse(text: &str) -> Parse {
    parse_source(&source(text))
}

pub fn first_function(module: &ModuleSyntax) -> Ast<'_, FnDef> {
    match module.items().next().expect("expected one item") {
        ItemView::FnDef(function) => function,
        other => panic!("expected function item, got {other:?}"),
    }
}

pub fn first_struct(module: &ModuleSyntax) -> Ast<'_, StructDef> {
    match module.items().next().expect("expected one item") {
        ItemView::StructDef(structure) => structure,
        other => panic!("expected struct item, got {other:?}"),
    }
}

pub fn first_const(module: &ModuleSyntax) -> Ast<'_, ConstDef> {
    match module.items().next().expect("expected one item") {
        ItemView::ConstDef(constant) => constant,
        other => panic!("expected const item, got {other:?}"),
    }
}

pub fn first_enum(module: &ModuleSyntax) -> Ast<'_, EnumDef> {
    match module.items().next().expect("expected one item") {
        ItemView::EnumDef(enumeration) => enumeration,
        other => panic!("expected enum item, got {other:?}"),
    }
}

pub fn type_name(ty: Ast<'_, TypeSyntax>) -> Option<String> {
    match ty.view() {
        TypeView::Named(path) => path.text(),
        TypeView::Applied(applied) => applied.path()?.text(),
        TypeView::Never(_) => Some("!".into()),
        _ => None,
    }
}
