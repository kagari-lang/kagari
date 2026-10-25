use crate::{
    bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir, tests::common,
};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::types::{PublicItem, named::NamedContract};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    declaration::{
        TypeDef, TypeDefKind,
        host::{
            HostCallPolicy, HostFunctionEffects, HostPassingStyle, HostReflectionPolicy,
            HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::{ImplDecl, ModuleDecl},
    },
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::sync::Arc;

/// Author the native provider against the checked source trait, then lower the
/// complete closure through the ordinary compiler. No legacy host records are
/// synthesized after lowering.
pub(super) fn readable_program(source: &str, context: &DefinitionContext) -> BytecodeProgram {
    readable_sources(&[("root", source)], false, context)
}

pub(super) fn readable_sources(
    inputs: &[(&str, &str)],
    host_argument: bool,
    context: &DefinitionContext,
) -> BytecodeProgram {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, source) in inputs {
        let uri = format!("mem://{name}");
        sources
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![(*name).into()],
                },
            )
            .unwrap();
        root = Some(
            sources
                .set(&uri, (*source).into(), SourceLayer::Base)
                .unwrap(),
        );
    }
    let root = root.unwrap();
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(kagari_stdlib::catalog::shared().unwrap());
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let original = common::adopt_bytecode(
        context,
        lower_program_to_bytecode(&ir, &Default::default())
            .map(|program| program.into_unverified())
            .unwrap(),
    );
    let source_module = &original.modules[original.root.index()];
    let contract = source_module
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::Trait(contract) if contract.name == "Readable" => Some(contract.clone()),
            _ => None,
        })
        .or_else(|| {
            source_module
                .trait_contracts
                .iter()
                .find(|contract| contract.abi.name == "Readable")
                .map(|contract| contract.abi.clone())
        })
        .unwrap();
    let source_owner = context.intern_root(&source_module.identity).unwrap();
    let trait_id = context
        .intern_child(source_owner, DefinitionKind::Trait, "Readable", 0)
        .unwrap();
    let mut provider = ModuleDecl::new(ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec![],
    });
    provider.types.push(TypeDef {
        name: "Counter".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::Opaque,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::None,
        }),
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    let definitions = provider.collect_definitions(context).unwrap();
    let counter = provider
        .definition(DefinitionKind::Struct, "Counter", &definitions)
        .unwrap();
    let owner = definitions.lookup_root(&provider.identity).unwrap();
    let argument = if host_argument {
        Ty::Host(counter)
    } else {
        Ty::Builtin(BuiltinType::I32)
    };
    let mut contracts = vec![(trait_id, contract, vec![argument], "get")];
    if host_argument {
        let marker = source_module
            .trait_contracts
            .iter()
            .find(|contract| contract.abi.name == "Marker")
            .unwrap();
        contracts.push((marker.declaration, marker.abi.clone(), vec![], "mark"));
    }
    for (trait_id, contract, arguments, method_name) in contracts {
        let binding = context
            .intern_child(
                owner,
                DefinitionKind::Function,
                &format!("{method_name}_binding"),
                0,
            )
            .unwrap();
        provider.trait_references.insert(trait_id, contract.clone());
        provider
            .implement_trait(
                &contract,
                ImplDecl {
                    generic_params: vec![],
                    bounds: vec![],
                    trait_type: Some(NominalTy {
                        declaration: trait_id,
                        arguments,
                        associated_types: Default::default(),
                    }),
                    for_type: Ty::Host(counter),
                    methods: vec![],
                },
                &[(method_name, binding)],
                context,
            )
            .unwrap();
        provider.implementations.last_mut().unwrap().methods[0].host_policy =
            Some(HostCallPolicy {
                passing: vec![HostPassingStyle::SharedBorrow],
                effects: HostFunctionEffects::default(),
            });
    }
    provider.collect_definitions(context).unwrap();
    let provider = Arc::new(
        DefinitionMetadata::checked(context.snapshot(), provider, &Default::default()).unwrap(),
    );
    let mut catalog = kagari_stdlib::catalog::shared().unwrap();
    catalog.push(provider);
    database.set_native_modules(catalog);
    let (name, source) = inputs.last().unwrap();
    sources
        .set(
            &format!("mem://{name}"),
            format!("use demo::Counter;\n{source}"),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    common::adopt_bytecode(
        context,
        lower_program_to_bytecode(&ir, &Default::default())
            .map(|program| program.into_unverified())
            .unwrap(),
    )
}

pub(super) fn replace_readable_result(program: &mut BytecodeProgram, result: Ty) {
    let provider = program
        .modules
        .iter_mut()
        .find(|module| module.identity.package.0 == "demo")
        .unwrap();
    let table = provider
        .public_items
        .iter_mut()
        .find_map(|item| match item {
            PublicItem::InterfaceTable(table)
                if table.methods.iter().any(|method| method.name == "get") =>
            {
                Some(table)
            }
            _ => None,
        })
        .unwrap();
    let Ty::Trait(trait_type) = &mut table.trait_type else {
        panic!("trait instance")
    };
    trait_type.arguments = vec![result.clone()];
    table.methods[0].return_type = result.clone();
    for record in &mut provider.named_declarations {
        if record.declaration == table.declaration {
            let NamedContract::Implementation(header) = &mut record.contract else {
                panic!("implementation header")
            };
            header.trait_type = Some(table.trait_type.clone());
        }
    }

    for declaration in &mut provider.native_declarations {
        if declaration.function.name == "get" {
            declaration.function.return_type = result.clone();
        }
    }
}
