use crate::query_tests::{native_fixture::module, test_analysis};
use kagari_hir::analysis::AnalysisDatabase;

pub(crate) fn database() -> AnalysisDatabase {
    let mut database = test_analysis();
    database.set_native_modules(
        kagari_stdlib::catalog::shared()
            .unwrap()
            .into_iter()
            .chain([module()])
            .collect(),
    );
    database
}
