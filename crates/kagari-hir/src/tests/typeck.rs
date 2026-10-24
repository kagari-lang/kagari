use crate::{
    builtin::surface,
    hir::{expr::ExprKind, pattern::PatternKind, stmt::StmtKind},
    resolver::collect::resolve_names,
    tests::{common, common::check_module},
    types::TypeId,
};
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};

use kagari_types::scalar::BuiltinType;

mod recovery;
mod standard;
mod traits;
mod typed_facts;
