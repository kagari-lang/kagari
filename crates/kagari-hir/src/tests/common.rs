use kagari_common::SourceFile;
use kagari_syntax::parse_module;

use crate::lower::{LoweredModule, lower_module};

pub fn lower_ok(text: &str) -> LoweredModule {
    let source = SourceFile::new("test.kg", text);
    parse_module(&source).expect("source should parse");
    lower_module(&source)
}

pub fn definition(
    lowered: &LoweredModule,
    kind: kagari_common::identity::DefinitionKind,
    name: &str,
) -> kagari_common::identity::DefinitionId {
    kagari_common::identity::DefinitionId {
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
    names: &crate::resolver::ResolvedNames,
    reuse: Option<&crate::typeck::BodyReuse<'_>>,
) -> crate::AnalysisResult<crate::typeck::TypedModule> {
    let declarations = crate::declarations::Declarations::collect(
        &lowered.source,
        lowered,
        names,
        &Default::default(),
    );
    crate::typeck::check_module_controlled(
        lowered,
        names,
        &declarations,
        reuse,
        &Default::default(),
    )
}
