use kagari_common::collection::CollectionAccess;
use kagari_common::{DiagnosticKind, SourceFile, TypePosition};
use kagari_syntax::parse_module;

use crate::{
    builtin::surface::{self, StandardIntrinsic},
    hir::{ExportItem, ExprKind, PatternKind, StmtKind},
    resolver::resolve_names,
    tests::common,
    tests::common::check_module,
    types::{BuiltinType, TypeId},
};

mod diagnostics;
mod standard;
mod traits;
