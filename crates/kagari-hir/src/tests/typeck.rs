use kagari_common::{
    collection::CollectionAccess,
    diagnostic::{DiagnosticKind, TypePosition},
    source::SourceFile,
};
use kagari_syntax::parser::parse_module;

use crate::{
    builtin::surface,
    hir::{expr::ExprKind, item::storage::ExportItem, pattern::PatternKind, stmt::StmtKind},
    resolver::collect::resolve_names,
    tests::common::{self, check_module},
    types::TypeId,
};
use kagari_abi::scalar::BuiltinType;

mod diagnostics;
mod standard;
mod traits;
