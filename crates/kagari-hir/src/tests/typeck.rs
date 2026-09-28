use kagari_common::{DiagnosticKind, SourceFile, TypePosition, collection::CollectionAccess};
use kagari_syntax::parse_module;

use crate::{
    builtin::surface,
    hir::{ExportItem, ExprKind, PatternKind, StmtKind},
    resolver::resolve_names,
    tests::common::{self, check_module},
    types::TypeId,
};
use kagari_abi::scalar::BuiltinType;

mod diagnostics;
mod standard;
mod traits;
