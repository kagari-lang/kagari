use crate::{
    hir::ids::BodySelection,
    lower::{LoweredModule, lower_module},
    resolver::resolved::{DeclarationNames, ResolvedNames},
    typeck::{
        applications::validate_signatures,
        check::{check_bodies_controlled, check_signatures},
        reuse::BodyReuse,
    },
};
use {kagari_common::identity::map::DefinitionContext, kagari_source::source::SourceFile};

use kagari_syntax::parser::parse_module;

pub fn lower_ok(text: &str) -> LoweredModule {
    let source = SourceFile::new("test.kg", text);
    parse_module(&source).expect("source should parse");
    lower_module(&source)
}

pub fn definition(
    lowered: &LoweredModule,
    kind: kagari_common::identity::DefinitionKind,
    name: &str,
) -> kagari_common::identity::DefinitionPath {
    kagari_common::identity::DefinitionPath {
        module: lowered.source.module_identity().clone(),
        path: vec![kagari_common::identity::DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

pub fn check_module(
    lowered: &LoweredModule,
    names: &ResolvedNames,
    reuse: Option<&BodyReuse<'_>>,
) -> crate::AnalysisResult<crate::typeck::TypedModule> {
    let declarations = crate::declarations::Declarations::collect_named(
        &lowered.source,
        lowered,
        &DeclarationNames {
            catalog: names.catalog.clone(),
            items: names.items.clone(),
            hosts: names.hosts.clone(),
            imports: names.imports.clone(),
        },
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    )
    .with_bindings(lowered, names, &Default::default());
    let mut signatures = check_signatures(lowered, &declarations, &Default::default());
    let mut aggregates = crate::aggregates::AggregateCatalog::default();
    aggregates
        .add_module(
            lowered,
            &declarations,
            signatures.facts(),
            &Default::default(),
        )
        .unwrap();
    let mut diagnostics = crate::DiagnosticBuffer::new();
    validate_signatures(
        lowered,
        &declarations,
        signatures.facts(),
        &aggregates,
        &mut diagnostics,
        &Default::default(),
    );
    signatures.diagnostics.extend(diagnostics);
    check_bodies_controlled(
        lowered,
        names,
        &declarations,
        crate::typeck::BodyInputs {
            const_limits: Default::default(),
            selection: BodySelection::All,
            signatures: &signatures,
            imported_functions: &Default::default(),
            aggregates: &aggregates,
        },
        reuse,
        &Default::default(),
    )
}
