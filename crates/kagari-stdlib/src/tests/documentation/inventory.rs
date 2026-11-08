//! Source-owned examples and correspondence to exact native declaration identities.
use crate::{catalog, sources as standard_sources};
use kagari_common::identity::{DefinitionKind, DefinitionPath, ModuleIdentity};
use kagari_hir::{analysis::AnalysisDatabase, declarations::DeclarationId, native::NativeSource};
use kagari_source::source_database::SourceSnapshot;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Owner {
    Module(ModuleIdentity),
    Item(DefinitionPath),
}

#[derive(Debug, Clone)]
pub(super) struct Origin {
    pub file: &'static str,
    pub line: usize,
    pub owner: Owner,
}

#[derive(Debug)]
pub(super) struct Example {
    pub code: String,
    pub origins: Vec<Origin>,
}

impl Example {
    pub fn group(&self) -> &'static str {
        self.origins
            .iter()
            .map(|origin| origin.file)
            .min()
            .expect("example owner")
    }

    pub fn label(&self) -> String {
        self.origins
            .iter()
            .map(|origin| format!("{}:{} {:?}", origin.file, origin.line, origin.owner))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(super) struct Inventory {
    pub baseline: AnalysisDatabase,
    pub examples: Vec<Example>,
    pub items: usize,
}

pub(super) fn collect() -> Inventory {
    let providers = catalog::shared().expect("standard catalog");
    assert_eq!(providers.len(), standard_sources::DECLARATIONS.len());
    let mut baseline = AnalysisDatabase::default();
    let sources = providers
        .iter()
        .map(|provider| {
            let source = standard_sources::declaration(&provider.records().identity)
                .expect("bundled module");
            NativeSource {
                uri: format!("bundled-stdlib:///{}", source.path),
                text: source.text.into(),
            }
        })
        .collect();
    baseline
        .set_native_sources(providers.clone(), sources)
        .unwrap();
    let native = baseline
        .snapshot(SourceSnapshot::default(), &Default::default())
        .unwrap();
    let declarations = native.declaration_snapshot();
    let mut examples = BTreeMap::<String, Vec<Origin>>::new();
    let mut items = 0;
    for file in declarations.files() {
        let module = file.source().module_identity();
        let resource = standard_sources::declaration(module).unwrap();
        let provider = providers
            .iter()
            .find(|provider| &provider.records().identity == module)
            .unwrap();
        let checked = native.file(file.source().id()).unwrap();
        assert!(
            checked.result().diagnostics().is_empty(),
            "{}: {:?}",
            resource.path,
            checked.result().diagnostics()
        );
        let module_doc = declarations.module_documentation(module).unwrap();
        assert_eq!(
            module_doc.documentation.trim(),
            provider.records().module_documentation.trim(),
            "{} module Markdown drift",
            resource.path
        );
        add(
            &mut examples,
            &module_doc.documentation,
            Origin {
                file: resource.path,
                line: 1,
                owner: Owner::Module(module.clone()),
            },
        );
        let expected = provider
            .records()
            .documentation
            .iter()
            .map(|(id, text)| (provider.definitions().resolve(*id).unwrap().to_path(), text))
            .collect::<BTreeMap<_, _>>();
        for declaration in file.declarations().iter() {
            let DeclarationId::Definition(id) = declaration.id else {
                continue;
            };
            let path = file
                .declarations()
                .definitions()
                .resolve(id)
                .unwrap()
                .to_path();
            let last = path.path.last().unwrap();
            if last.name.starts_with("__") {
                continue;
            }
            let Some(document) = declarations.documentation(&declaration.id) else {
                assert_eq!(
                    last.kind,
                    DefinitionKind::Impl,
                    "{} {:?}: named item without a documentation query",
                    resource.path,
                    path
                );
                continue;
            };
            let expected = expected.get(&path).map_or("", |text| text.as_str());
            assert_eq!(
                document.documentation.trim(),
                expected.trim(),
                "{} {:?}: source/export Markdown drift",
                resource.path,
                path
            );
            // Implementation associated-type bindings inherit the trait's contract.
            let binding = matches!(last.kind, DefinitionKind::AssociatedType)
                && path
                    .path
                    .first()
                    .is_some_and(|part| part.kind == DefinitionKind::Impl);
            if binding {
                continue;
            }
            assert!(
                !document.documentation.trim().is_empty(),
                "{} {:?}: undocumented public item",
                resource.path,
                path
            );
            if matches!(last.kind, DefinitionKind::Method | DefinitionKind::Function) {
                assert!(
                    document.documentation.contains("```kgr\n"),
                    "{} {:?}: operation without an example",
                    resource.path,
                    path
                );
            }
            let line = file.source().text()[..declaration.location.range.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            add(
                &mut examples,
                &document.documentation,
                Origin {
                    file: resource.path,
                    line,
                    owner: Owner::Item(path),
                },
            );
            items += 1;
        }
    }
    drop(native);
    let mut schemas = 0;
    for (code, origins) in &examples {
        if code.contains("fn main(") {
            continue;
        }
        assert!(
            origins
                .iter()
                .all(|origin| matches!(origin.file, "core/host.kgr" | "std/host.kgr"))
                && (code.contains("Ref<Player>") || code.contains("RefMut<Player>")),
            "unclassified non-executable example: {origins:?}\n{code}"
        );
        schemas += 1;
    }
    assert_eq!(
        schemas, 2,
        "only the Ref and RefMut visitor declarations are compile-only"
    );
    Inventory {
        baseline,
        examples: examples
            .into_iter()
            .map(|(code, origins)| Example { code, origins })
            .collect(),
        items,
    }
}

fn add(examples: &mut BTreeMap<String, Vec<Origin>>, markdown: &str, origin: Origin) {
    for block in markdown.split("```kgr\n").skip(1) {
        let (code, _) = block.split_once("```").expect("closed Kagari example");
        examples
            .entry(code.into())
            .or_default()
            .push(origin.clone());
    }
}
