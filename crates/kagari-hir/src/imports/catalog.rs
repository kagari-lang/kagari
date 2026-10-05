//! Immutable visible declarations and package spellings for import resolution.
use crate::hir::ty::TypeKind;
use crate::{
    hir::{
        ids::EnumId,
        item::{function::FunctionKind, storage::ExportItem},
    },
    imports::{ImportTarget, ModuleImports, SourceImport},
    lower::LoweredModule,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{DefinitionPath, ModuleIdentity},
};
use kagari_source::source::SourceFile;
use kagari_types::{collection::CollectionAccess, visibility::Visibility};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::Arc,
};

pub(super) struct SourceCatalog<'a> {
    pub(super) paths: BTreeMap<String, Vec<SourceCatalogEntry<'a>>>,
    pub(super) package_aliases: BTreeMap<String, BTreeSet<String>>,
    pub(super) array_interfaces: BTreeMap<CollectionAccess, DefinitionPath>,
}

pub(super) struct SourceCatalogEntry<'a> {
    pub(super) source: &'a SourceFile,
    pub(super) installed: bool,
    pub(super) prelude: bool,
    pub(super) glob_enums: HashSet<EnumId>,
    pub(super) members: Arc<BTreeMap<String, Vec<CatalogMember>>>,
    pub(super) reexports: BTreeMap<usize, ImportTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CatalogMember {
    pub(super) item: ExportItem,
    pub(super) visibility: Visibility,
}

impl<'a> SourceCatalog<'a> {
    pub(super) fn module_path(&self, identity: &ModuleIdentity) -> String {
        let aliases = self
            .package_aliases
            .iter()
            .filter(|(_, packages)| packages.len() == 1 && packages.contains(&identity.package.0))
            .collect::<Vec<_>>();
        let package = if let [(alias, _)] = aliases.as_slice() {
            alias.as_str()
        } else {
            &identity.package.0
        };
        format!("{}::{}", package, identity.path.join("::"))
    }

    pub(super) fn source_path<'p>(&self, path: &'p str) -> Cow<'p, str> {
        if let Some((alias, member)) = path.split_once("::")
            && let Some(packages) = self.package_aliases.get(alias)
            && packages.len() == 1
        {
            let package = packages.first().expect("one installed package alias");
            return Cow::Owned(format!("{package}::{member}"));
        }
        Cow::Borrowed(path)
    }

    pub(super) fn new(
        sources: impl IntoIterator<Item = &'a LoweredModule>,
        imports: Option<&BTreeMap<ModuleIdentity, ModuleImports>>,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let mut paths = BTreeMap::<_, Vec<_>>::new();
        let mut package_aliases = BTreeMap::<String, BTreeSet<String>>::new();
        let mut array_interfaces = BTreeMap::new();
        for module in sources {
            cancel.check()?;
            array_interfaces.extend(module.native_array_interfaces.clone());
            if let Some(alias) = &module.native_package_alias {
                package_aliases
                    .entry(alias.clone())
                    .or_default()
                    .insert(module.source.module_identity().package.0.clone());
            }
            let mut members = BTreeMap::<_, Vec<_>>::new();
            let mut add = |name: &str, item, visibility| {
                members
                    .entry(name.to_owned())
                    .or_insert_with(Vec::new)
                    .push(CatalogMember { item, visibility });
            };
            for item in module
                .module
                .functions
                .iter()
                .filter(|item| item.kind == FunctionKind::User)
            {
                cancel.check()?;
                add(&item.name, ExportItem::Function(item.id), item.visibility);
            }
            for item in &module.module.consts {
                cancel.check()?;
                add(&item.name, ExportItem::Const(item.id), item.visibility);
            }
            for item in &module.module.modules {
                cancel.check()?;
                add(&item.name, ExportItem::Module(item.id), item.visibility);
            }
            for item in &module.module.opaque_types {
                cancel.check()?;
                add(&item.name, ExportItem::OpaqueType(item.id), item.visibility);
            }
            for item in &module.module.structs {
                cancel.check()?;
                add(&item.name, ExportItem::Struct(item.id), item.visibility);
            }
            for item in &module.module.enums {
                cancel.check()?;
                add(&item.name, ExportItem::Enum(item.id), item.visibility);
                for variant in &item.variants {
                    cancel.check()?;
                    add(
                        &format!("{}::{}", item.name, variant.name),
                        ExportItem::Variant(variant.id),
                        item.visibility,
                    );
                }
            }
            for item in &module.module.traits {
                cancel.check()?;
                add(&item.name, ExportItem::Trait(item.id), item.visibility);
            }
            // Direct registration exports have no source import node. Respect
            // their explicit module aliases alongside qualified enum members.
            for export in &module.module.exports {
                if let ExportItem::Variant(_) = export.item {
                    cancel.check()?;
                    add(&export.name, export.item, Visibility::Public);
                }
            }
            for implementation in &module.module.impls {
                cancel.check()?;
                if implementation.trait_ref.is_some() {
                    continue;
                }
                let Some(reference) = implementation.for_type else {
                    continue;
                };
                let owner = match &module.module.type_ref(reference).kind {
                    TypeKind::Named(name) | TypeKind::Generic { name, .. } => name,
                    _ => continue,
                };
                let owner_visibility = module
                    .module
                    .structs
                    .iter()
                    .find(|item| &item.name == owner)
                    .map(|item| item.visibility)
                    .or_else(|| {
                        module
                            .module
                            .enums
                            .iter()
                            .find(|item| &item.name == owner)
                            .map(|item| item.visibility)
                    })
                    .or_else(|| {
                        module
                            .module
                            .opaque_types
                            .iter()
                            .find(|item| &item.name == owner)
                            .map(|item| item.visibility)
                    });
                for method in &implementation.methods {
                    let Some(function) = module
                        .module
                        .functions
                        .iter()
                        .find(|item| item.id == method.function)
                    else {
                        continue;
                    };
                    add(
                        &format!("{owner}::{}", method.name),
                        ExportItem::Function(method.function),
                        match (owner_visibility, function.visibility) {
                            (Some(Visibility::Private), _) | (_, Visibility::Private) => {
                                Visibility::Private
                            }
                            (Some(Visibility::PublicSuper), _) | (_, Visibility::PublicSuper) => {
                                Visibility::PublicSuper
                            }
                            _ => Visibility::Public,
                        },
                    );
                }
            }
            for (index, item) in module.module.imports.iter().enumerate() {
                cancel.check()?;
                if !item.glob {
                    add(&item.alias, ExportItem::Import(index), item.visibility);
                }
            }
            let resolved_imports = imports.and_then(|all| all.get(module.source.module_identity()));
            if let Some(imports) = resolved_imports {
                for (index, import) in imports.entries.iter().enumerate() {
                    cancel.check()?;
                    if index >= module.module.imports.len()
                        && !import.internal_namespace
                        && import.target.is_some()
                        && !members.contains_key(&import.alias)
                    {
                        members.insert(
                            import.alias.clone(),
                            vec![CatalogMember {
                                item: ExportItem::Import(index),
                                visibility: import.visibility,
                            }],
                        );
                    }
                }
            }
            paths
                .entry(module.source.module_identity().to_string())
                .or_default()
                .push(SourceCatalogEntry {
                    source: &module.source,
                    installed: module.registered_native_api,
                    prelude: module.native_prelude,
                    glob_enums: module.module.enums.iter().map(|item| item.id).collect(),
                    members: Arc::new(members),
                    reexports: resolved_imports.map_or_else(BTreeMap::new, |imports| {
                        imports
                            .entries
                            .iter()
                            .enumerate()
                            .filter_map(|(index, entry)| {
                                entry.target.clone().map(|target| (index, target))
                            })
                            .collect()
                    }),
                });
        }
        Ok(Self {
            paths,
            package_aliases,
            array_interfaces,
        })
    }

    pub(super) fn same_members(&self, other: &Self) -> bool {
        self.package_aliases == other.package_aliases
            && self.paths.len() == other.paths.len()
            && self.paths.iter().all(|(path, entries)| {
                other.paths.get(path).is_some_and(|old| {
                    entries.len() == old.len()
                        && entries.iter().zip(old).all(|(a, b)| {
                            a.members == b.members
                                && a.reexports == b.reexports
                                && a.glob_enums == b.glob_enums
                        })
                })
            })
    }
}

impl SourceCatalogEntry<'_> {
    pub(super) fn target(
        &self,
        item: Option<ExportItem>,
        importer: &ModuleIdentity,
    ) -> SourceImport {
        SourceImport {
            module: self.source.module_identity().clone(),
            file: self.source.id(),
            revision: self.source.revision(),
            item,
            members: Arc::new(
                self.members
                    .iter()
                    .filter_map(|(name, members)| {
                        let visible = members
                            .iter()
                            .filter(|member| {
                                member
                                    .visibility
                                    .allows(self.source.module_identity(), importer)
                            })
                            .map(|member| member.item)
                            .collect::<Vec<_>>();
                        (!visible.is_empty()).then(|| (name.clone(), visible))
                    })
                    .collect(),
            ),
        }
    }
}
