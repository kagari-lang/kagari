use kagari_syntax::parser::parse_module;
use {
    kagari_common::collection::CollectionAccess,
    kagari_source::{
        diagnostic::{DiagnosticKind, TypePosition},
        source::SourceFile,
    },
};

use crate::{
    builtin::surface,
    hir::{expr::ExprKind, item::storage::ExportItem, pattern::PatternKind, stmt::StmtKind},
    resolver::collect::resolve_names,
    tests::common::{self, check_module},
    types::TypeId,
};
use kagari_contract::scalar::BuiltinType;

mod diagnostics;
mod standard;
mod traits;
