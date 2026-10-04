use crate::{
    builtin::surface,
    hir::{expr::ExprKind, item::storage::ExportItem, pattern::PatternKind, stmt::StmtKind},
    resolver::collect::resolve_names,
    tests::{common, common::check_module},
    types::TypeId,
};
use kagari_source::{
    diagnostic::{DiagnosticKind, TypePosition},
    source::SourceFile,
};
use kagari_syntax::parser::parse_module;
use kagari_types::{collection::CollectionAccess, scalar::BuiltinType};

mod diagnostics;
mod standard;
mod traits;
