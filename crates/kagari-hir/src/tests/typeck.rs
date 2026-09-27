use kagari_common::collection::CollectionAccess;
use kagari_common::{DiagnosticKind, SourceFile, TypePosition};
use kagari_syntax::parse_module;

use crate::builtin::surface;
use crate::hir::ExportItem;
use crate::hir::ExprKind;
use crate::hir::PatternKind;
use crate::hir::StmtKind;
use crate::resolver::resolve_names;
use crate::tests::common;
use crate::tests::common::check_module;
use crate::types::TypeId;
use kagari_abi::scalar::BuiltinType;

mod diagnostics;
mod standard;
mod traits;
