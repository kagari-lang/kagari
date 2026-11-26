use crate::{
    bytecode::lower_program_to_bytecode,
    source::program::{
        InstallationTraitRequirements, SourceProgramError, lower_program_with_installation,
    },
    tests::{
        common::{self, compilation},
        standard::standard_modules,
    },
};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::types::{PublicItem, named::NamedContract};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, Param, TypeDef, TypeDefKind,
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

pub(super) fn root_owner() -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["root".into()],
    }
}

pub(super) fn provider_owner() -> ModuleIdentity {
    // A child module has ordinary lexical access to the private-trait cases.
    let mut owner = root_owner();
    owner.path.push("provider".into());
    owner
}

pub(super) fn readable_program(source: &str, context: &DefinitionContext) -> BytecodeProgram {
    readable_sources(&[("root", source)], false, context)
}

pub(super) fn readable_sources(
    inputs: &[(&str, &str)],
    host_argument: bool,
    context: &DefinitionContext,
) -> BytecodeProgram {
    readable_sources_with(inputs, host_argument, context, |_| Default::default()).unwrap()
}

pub(super) fn readable_sources_with(
    inputs: &[(&str, &str)],
    host_argument: bool,
    context: &DefinitionContext,
    configure: impl FnOnce(&mut ModuleDecl) -> InstallationTraitRequirements,
) -> Result<BytecodeProgram, SourceProgramError> {
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
        let text = if *name == "root" {
            format!("use pkg::root::provider::Counter;\n{source}")
        } else {
            (*source).to_owned()
        };
        root = Some(sources.set(&uri, text, SourceLayer::Base).unwrap());
    }
    let source_owner = context.intern_root(&root_owner()).unwrap();
    let mut provider = ModuleDecl::new(provider_owner());
    provider.dependencies.insert(root_owner());
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
    let mut implementations = vec![("Readable", vec![argument.clone()], "get", argument)];
    if host_argument {
        implementations.push(("Marker", vec![], "mark", Ty::Builtin(BuiltinType::I32)));
    }
    for (index, (trait_name, arguments, method, result)) in implementations.into_iter().enumerate()
    {
        let declaration = context
            .intern_child(source_owner, DefinitionKind::Trait, trait_name, 0)
            .unwrap();
        let implementation = context
            .intern_child(owner, DefinitionKind::Impl, "", index as u32)
            .unwrap();
        let binding = context
            .intern_child(implementation, DefinitionKind::Method, method, 0)
            .unwrap();
        provider.implementations.push(ImplDecl {
            generic_params: vec![],
            bounds: vec![],
            trait_type: Some(NominalTy {
                declaration,
                arguments,
                associated_types: Default::default(),
            }),
            for_type: Ty::Host(counter),
            methods: vec![FnDecl {
                name: method.into(),
                implementation: CallableImplementation::Native(binding),
                generic_params: vec![],
                bounds: vec![],
                method_policy: Default::default(),
                host_policy: Some(HostCallPolicy {
                    passing: vec![HostPassingStyle::SharedBorrow],
                    effects: HostFunctionEffects::default(),
                }),
                params: vec![Param {
                    name: "self".into(),
                    mutable: false,
                    ty: Ty::Host(counter),
                }],
                return_type: result,
            }],
        });
    }
    let installation = configure(&mut provider);
    let definitions = provider.collect_definitions(context).unwrap();
    let provider =
        Arc::new(DefinitionMetadata::checked(definitions, provider, &Default::default()).unwrap());
    let mut catalog = standard_modules();
    catalog.push(provider);
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(catalog);
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot
        .check_program(root.unwrap(), &Default::default())
        .unwrap();
    let input = compilation(&checked);
    let ir = lower_program_with_installation(&input, &Default::default(), &installation, None)?;
    Ok(common::adopt_bytecode(
        context,
        lower_program_to_bytecode(&ir, &Default::default())
            .unwrap()
            .into_unverified(),
    ))
}

pub(super) fn replace_readable_result(program: &mut BytecodeProgram, result: Ty) {
    let provider = program
        .modules
        .iter_mut()
        .find(|module| module.identity == provider_owner())
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
