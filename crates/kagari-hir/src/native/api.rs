//! Direct declaration import. Generated CST is presentation only, not semantic input.
use crate::{
    hir::{
        ids::{BodyOwner, EnumId, FunctionId, HirOwner, OpaqueTypeId, TypeRefId, VariantId},
        item::{
            Item,
            adt::{Enum, OpaqueType, Variant},
            behavior::{
                AssociatedType, GenericParam, Impl, ImplMethod, ReceiverKind, TraitBound, TraitDef,
                TraitMethod, TraitRef,
            },
            function::{Function, FunctionKind, Param},
            module::Import,
            storage::{Export, ExportItem, Visibility},
        },
        ty::{TypeData, TypeKind},
        writeability::Writeability,
    },
    lower::{LoweredModule, context::Lowerer},
    native::{NativeBinding, NativeTypeKind},
};
use kagari_abi::{
    callable::{CallableImplementation, MethodPolicy},
    native_api::{
        NativeApiError, NativeModule,
        render::{NativeApiSource, NativeBoundSite},
    },
    scalar::BuiltinType,
    standard::surface::builtin_type_spec,
    types::{
        AbiType, ConstraintAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi, NominalAbiType,
        TypeAbiKind, native::NativeTypeConstructor,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    collection::CollectionAccess,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, associated_type_id},
    source_database::{SourceDatabase, SourceLayer},
    span::Span,
};
use kagari_syntax::parser::{Parse, ParseLimits, parse_declarations};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub(crate) fn import(
    definition: &NativeModule,
    providers: &[Arc<NativeModule>],
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<(Parse, Arc<LoweredModule>), NativeApiError> {
    definition.validate()?;
    let generated = definition.declaration_source()?;
    let mut sources = SourceDatabase::default();
    sources
        .bind_module(&generated.uri, definition.identity.clone())
        .map_err(NativeApiError)?;
    let id = sources
        .set(&generated.uri, generated.text.clone(), SourceLayer::Base)
        .map_err(NativeApiError)?;
    let source = sources
        .snapshot()
        .file(id)
        .expect("generated native source")
        .clone();
    // Tool queries expose lossless syntax and docs; declaration checking below consumes records directly.
    let parsed = parse_declarations(&source, limits, cancel)
        .map_err(|_| NativeApiError("native declaration presentation cancelled".into()))?;
    if !parsed.diagnostics().is_empty() {
        return Err(NativeApiError(format!(
            "invalid generated declaration syntax: {:?}",
            parsed.diagnostics().first()
        )));
    }
    let mut importer = Importer {
        definition,
        providers,
        generated: &generated,
        lowerer: Lowerer::new(cancel.clone()),
        native_types: HashMap::new(),
        native_enums: HashMap::new(),
        native_functions: HashMap::new(),
        method_policies: HashMap::new(),
        external_imports: HashSet::new(),
    };
    importer.import_types()?;
    importer.import_traits()?;
    importer.import_implementations()?;
    importer.import_functions()?;
    let mut occupied: HashSet<String> = importer
        .lowerer
        .module
        .exports
        .iter()
        .map(|export| export.name.clone())
        .chain(
            definition
                .functions
                .iter()
                .map(|function| function.name.clone()),
        )
        .chain(
            importer
                .lowerer
                .module
                .imports
                .iter()
                .map(|import| import.alias.clone()),
        )
        .collect();
    for (index, identity) in definition.dependencies.iter().enumerate() {
        let mut alias = format!("__native_dependency_{index}");
        while !occupied.insert(alias.clone()) {
            alias.push('_');
        }
        importer.lowerer.module.imports.push(Import {
            visibility: Visibility::Private,
            alias,
            path: format!("{}::{}", identity.package.0, identity.path.join("::")),
            span: Span::default(),
            glob: false,
        });
    }
    let (module, source_map) = importer.lowerer.finish();
    Ok((
        parsed,
        Arc::new(LoweredModule {
            source,
            module,
            source_map,
            attributes: vec![],
            registered_native_api: true,
            native_package_alias: definition.package_alias.clone(),
            registered_declarations: definition.native_declarations(),
            native_types: importer.native_types,
            native_enums: importer.native_enums,
            native_functions: importer.native_functions,
            method_policies: importer.method_policies,
            native_attributes: HashSet::new(),
        }),
    ))
}

struct Importer<'a> {
    definition: &'a NativeModule,
    providers: &'a [Arc<NativeModule>],
    generated: &'a NativeApiSource,
    lowerer: Lowerer,
    native_types: HashMap<OpaqueTypeId, NativeTypeKind>,
    native_enums: HashMap<EnumId, NativeTypeKind>,
    native_functions: HashMap<FunctionId, NativeBinding>,
    method_policies: HashMap<FunctionId, MethodPolicy>,
    external_imports: HashSet<String>,
}

impl Importer<'_> {
    fn import_types(&mut self) -> Result<(), NativeApiError> {
        let definition = self.definition;
        let generated = self.generated;
        for ty in &definition.types {
            let TypeAbiKind::Native(constructor) = ty.kind else {
                return Err(NativeApiError("missing native representation".into()));
            };
            let owner = definition.definition(constructor.declaration_kind(), &ty.name);
            let site = &generated.sites[&owner];
            if let NativeTypeConstructor::Enum(kind) = constructor {
                let id = self.lowerer.source_map.push_enum(site.span);
                self.lowerer
                    .source_map
                    .insert_item_name(Item::Enum(id), site.name_span);
                let generic_params = self.generics(&owner, &ty.generic_params);
                let mut variants = vec![];
                for (index, variant) in ty.variants.iter().enumerate() {
                    let mut declaration = owner.clone();
                    declaration.path.push(DefinitionPathSegment {
                        kind: DefinitionKind::Variant,
                        name: variant.name.clone(),
                        occurrence: 0,
                    });
                    let site = &generated.sites[&declaration];
                    let variant_id = VariantId::new(self.lowerer.source_map.arena(), id, index);
                    self.lowerer
                        .source_map
                        .insert_variant(variant_id, site.name_span);
                    if definition.variant_exports.contains(&ty.name) {
                        self.lowerer.module.exports.push(Export {
                            name: variant.name.clone(),
                            item: ExportItem::Variant(variant_id),
                        });
                    }
                    let payload = variant
                        .payload
                        .iter()
                        .zip(&site.parameters)
                        .map(|(ty, span)| self.ty(ty, *span))
                        .collect::<Result<_, _>>()?;
                    variants.push(Variant {
                        id: variant_id,
                        name: variant.name.clone(),
                        payload,
                    });
                }
                self.lowerer.module.enums.push(Enum {
                    id,
                    visibility: Visibility::Public,
                    name: ty.name.clone(),
                    generic_params,
                    variants,
                    methods: vec![],
                    impls: vec![],
                });
                self.lowerer.module.items.push(Item::Enum(id));
                self.lowerer.module.exports.push(Export {
                    name: ty.name.clone(),
                    item: ExportItem::Enum(id),
                });
                self.native_enums.insert(id, NativeTypeKind::Enum(kind));
                continue;
            }
            let id = self.lowerer.source_map.push_opaque_type(site.span);
            self.lowerer
                .source_map
                .insert_item_name(Item::OpaqueType(id), site.name_span);
            let generic_params = self.generics(&owner, &ty.generic_params);
            self.lowerer.module.opaque_types.push(OpaqueType {
                id,
                visibility: Visibility::Public,
                name: ty.name.clone(),
                generic_params,
                bounds: vec![],
                trait_bounds: vec![],
                definition: None,
            });
            self.lowerer.module.items.push(Item::OpaqueType(id));
            self.lowerer.module.exports.push(Export {
                name: ty.name.clone(),
                item: ExportItem::OpaqueType(id),
            });
            self.native_types.insert(
                id,
                match constructor {
                    NativeTypeConstructor::Array => NativeTypeKind::ArrayList,
                    NativeTypeConstructor::String => NativeTypeKind::String,
                    NativeTypeConstructor::Map => NativeTypeKind::LinkedHashMap,
                    NativeTypeConstructor::Set => NativeTypeKind::LinkedHashSet,
                    NativeTypeConstructor::Iter => NativeTypeKind::Iter,
                    NativeTypeConstructor::Range(kind) => NativeTypeKind::Range(kind),
                    NativeTypeConstructor::Enum(_) => unreachable!("enum imported separately"),
                },
            );
        }
        Ok(())
    }
    fn import_traits(&mut self) -> Result<(), NativeApiError> {
        let definition = self.definition;
        let generated = self.generated;
        for item in &definition.traits {
            let owner = definition.definition(DefinitionKind::Trait, &item.name);
            let site = &generated.sites[&owner];
            let id = self.lowerer.source_map.push_trait(site.span);
            self.lowerer
                .source_map
                .insert_item_name(Item::Trait(id), site.name_span);
            let generic_params = self.generics(&owner, &item.generic_params);
            let supertraits = item
                .supertraits
                .iter()
                .map(|ty| {
                    self.nominal_type(ty, site.name_span)
                        .map(|ty| TraitRef { ty })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut associated_types = vec![];
            for member in &item.associated_types {
                let site = &generated.sites[&member.declaration];
                let name = member
                    .declaration
                    .path
                    .last()
                    .ok_or_else(|| NativeApiError("missing associated name".into()))?
                    .name
                    .clone();
                let name_ref = self.lowerer.alloc_type(
                    site.name_span,
                    TypeData {
                        kind: TypeKind::Named(name.clone()),
                    },
                );
                let bounds = member
                    .bounds
                    .iter()
                    .zip(&site.bounds[0].constraints)
                    .map(|(constraint, span)| self.constraint_ref(constraint, *span))
                    .collect::<Result<_, _>>()?;
                associated_types.push(AssociatedType {
                    name,
                    name_ref,
                    ty: None,
                    bounds,
                    generic_params: vec![],
                    parameter_bounds: vec![],
                });
            }
            let mut methods = vec![];
            for method in &item.methods {
                let method_owner = NativeModule::method_id(&owner, &method.name);
                let function = self.function(
                    &method_owner,
                    method,
                    FunctionKind::TraitMethod,
                    generic_params.clone(),
                    None,
                )?;
                let method_id = self
                    .lowerer
                    .source_map
                    .push_trait_method(generated.sites[&method_owner].span);
                methods.push(TraitMethod {
                    id: method_id,
                    name: method.name.clone(),
                    receiver: ReceiverKind::Value,
                    function,
                    has_default: matches!(
                        method.implementation,
                        CallableImplementation::NativeDefault(_)
                    ),
                });
            }
            self.lowerer.module.traits.push(TraitDef {
                id,
                visibility: Visibility::Public,
                name: item.name.clone(),
                generic_params,
                supertraits,
                methods,
                associated_types,
                associated_consts: vec![],
            });
            self.lowerer.module.items.push(Item::Trait(id));
            self.lowerer.module.exports.push(Export {
                name: item.name.clone(),
                item: ExportItem::Trait(id),
            });
        }
        Ok(())
    }
    fn import_implementations(&mut self) -> Result<(), NativeApiError> {
        let definition = self.definition;
        let generated = self.generated;
        for (index, implementation) in definition.implementations.iter().enumerate() {
            let owner = definition.implementation_id(index);
            let site = &generated.sites[&owner];
            let id = self.lowerer.source_map.push_impl(site.span);
            let generic_params = self.generics(&owner, &implementation.generic_params);
            let for_type = self.ty(&implementation.for_type, site.name_span)?;
            let trait_ref = implementation
                .trait_type
                .as_ref()
                .map(|ty| {
                    let mut header = ty.clone();
                    header.associated_types.clear();
                    self.nominal_type(&header, site.name_span)
                        .map(|ty| TraitRef { ty })
                })
                .transpose()?;
            let mut associated_types = vec![];
            if let Some(trait_type) = &implementation.trait_type {
                for (member, value) in &trait_type.associated_types {
                    let name = member
                        .path
                        .last()
                        .ok_or_else(|| NativeApiError("missing associated name".into()))?
                        .name
                        .clone();
                    let site = &generated.sites[&associated_type_id(&owner, &name)];
                    let name_ref = self.lowerer.alloc_type(
                        site.name_span,
                        TypeData {
                            kind: TypeKind::Named(name.clone()),
                        },
                    );
                    associated_types.push(AssociatedType {
                        name,
                        name_ref,
                        ty: Some(self.ty(value, site.parameters[0])?),
                        bounds: vec![],
                        generic_params: vec![],
                        parameter_bounds: vec![],
                    });
                }
            }
            let mut methods = vec![];
            for method in &implementation.methods {
                let method_owner = NativeModule::method_id(&owner, &method.name);
                let function = self.function(
                    &method_owner,
                    method,
                    FunctionKind::ImplMethod,
                    generic_params.clone(),
                    Some(for_type),
                )?;
                methods.push(ImplMethod {
                    name: method.name.clone(),
                    function,
                });
            }
            let bounds = self.bounds(&implementation.bounds, &site.bounds)?;
            self.lowerer.module.impls.push(Impl {
                id,
                generic_params,
                trait_ref,
                for_type: Some(for_type),
                bounds,
                methods,
                associated_types,
                associated_consts: vec![],
            });
            self.lowerer.module.items.push(Item::Impl(id));
        }
        Ok(())
    }
    fn import_functions(&mut self) -> Result<(), NativeApiError> {
        let definition = self.definition;
        for function in &definition.functions {
            let owner = definition.definition(DefinitionKind::Function, &function.name);
            let generic_params = self.generics(&owner, &function.generic_params);
            let id = self.function(&owner, function, FunctionKind::User, generic_params, None)?;
            self.lowerer.module.items.push(Item::Function(id));
            if !definition.private_functions.contains(&owner) {
                self.lowerer.module.exports.push(Export {
                    name: function.name.clone(),
                    item: ExportItem::Function(id),
                });
            }
        }
        Ok(())
    }

    fn generics(
        &mut self,
        owner: &DefinitionId,
        params: &[GenericParameterAbi],
    ) -> Vec<GenericParam> {
        params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                let span = self.generated.sites[owner].generics[index];
                GenericParam {
                    id: self.lowerer.source_map.push_generic_param(span),
                    name: format!("T{}", param.position),
                    bounds: vec![],
                }
            })
            .collect()
    }
    fn function(
        &mut self,
        owner: &DefinitionId,
        function: &FunctionAbi,
        kind: FunctionKind,
        generic_params: Vec<GenericParam>,
        receiver: Option<TypeRefId>,
    ) -> Result<FunctionId, NativeApiError> {
        let site = &self.generated.sites[owner];
        let id = self.lowerer.source_map.push_function(site.span);
        self.lowerer
            .source_map
            .insert_item_name(Item::Function(id), site.name_span);
        let old = self
            .lowerer
            .source_map
            .set_owner(HirOwner::Body(BodyOwner::Function(id)));
        let mut params = vec![];
        for (index, param) in function.params.iter().enumerate() {
            let span = site.parameters[index];
            let ty = if let Some(receiver) = receiver.filter(|_| param.name == "self") {
                receiver
            } else {
                self.ty(&param.ty, span)?
            };
            params.push(Param {
                id: self.lowerer.source_map.push_param(span),
                name: param.name.clone(),
                ty,
                writeability: if param.mutable {
                    Writeability::Var
                } else {
                    Writeability::Val
                },
            });
        }
        let return_type = Some(self.ty(&function.return_type, site.name_span)?);
        let bounds = self.bounds(&function.bounds, &site.bounds)?;
        self.lowerer.source_map.set_owner(old);
        match &function.implementation {
            CallableImplementation::Native(binding) => {
                self.native_functions
                    .insert(id, NativeBinding::Entry(binding.clone()));
            }
            CallableImplementation::NativeDefault(application) => {
                self.native_functions
                    .insert(id, NativeBinding::Default(application.clone()));
            }
            _ => {}
        }
        self.method_policies.insert(id, function.method_policy);
        self.lowerer.module.functions.push(Function {
            id,
            kind,
            visibility: if kind == FunctionKind::TraitMethod
                || self.definition.private_functions.contains(owner)
            {
                Visibility::Private
            } else {
                Visibility::Public
            },
            name: function.name.clone(),
            generic_params,
            bounds,
            params,
            return_type,
            body: None,
        });
        Ok(id)
    }
    fn bounds(
        &mut self,
        bounds: &[GenericBoundAbi],
        sites: &[NativeBoundSite],
    ) -> Result<Vec<TraitBound>, NativeApiError> {
        let mut result = vec![];
        for (bound, site) in bounds.iter().zip(sites) {
            let target_ref = self.ty(&bound.ty, site.target)?;
            let traits = bound
                .constraints
                .iter()
                .zip(&site.constraints)
                .map(|(constraint, span)| self.constraint_ref(constraint, *span))
                .collect::<Result<_, _>>()?;
            result.push(TraitBound {
                target: match &bound.ty {
                    AbiType::Parameter { position, .. } => format!("T{position}"),
                    _ => String::new(),
                },
                target_ref,
                traits,
            });
        }
        Ok(result)
    }

    fn constraint_ref(
        &mut self,
        constraint: &ConstraintAbi,
        span: Span,
    ) -> Result<TraitRef, NativeApiError> {
        let ty = match constraint {
            ConstraintAbi::Trait(trait_type) => self.nominal_type(trait_type, span)?,
            ConstraintAbi::Standard(kind) => {
                let name = kind
                    .source_bound_name()
                    .ok_or_else(|| NativeApiError("native bound has no source name".into()))?;
                self.lowerer.alloc_type(
                    span,
                    TypeData {
                        kind: TypeKind::Named(name.into()),
                    },
                )
            }
        };
        Ok(TraitRef { ty })
    }

    fn nominal_type(
        &mut self,
        nominal: &NominalAbiType,
        span: Span,
    ) -> Result<TypeRefId, NativeApiError> {
        let name = nominal
            .declaration
            .path
            .last()
            .ok_or_else(|| NativeApiError("missing nominal name".into()))?;
        let name = if nominal.declaration.module == self.definition.identity {
            name.name.clone()
        } else {
            let module = &nominal.declaration.module;
            format!(
                "{}::{}::{}",
                module.package.0,
                module.path.join("::"),
                name.name
            )
        };
        if nominal.declaration.module != self.definition.identity
            && self.external_imports.insert(name.clone())
        {
            // Registered references establish normal checked dependencies. The
            // complete path stays the name of the direct record; generated text
            // is not parsed to infer an import or authorize a declaration.
            self.lowerer.module.imports.push(Import {
                visibility: Visibility::Private,
                alias: name.clone(),
                path: name.clone(),
                span,
                glob: false,
            });
        }
        let args = nominal
            .arguments
            .iter()
            .map(|ty| self.ty(ty, span))
            .collect::<Result<_, _>>()?;
        let bindings = nominal
            .associated_types
            .iter()
            .map(|(id, ty)| {
                let name = id
                    .path
                    .last()
                    .ok_or_else(|| NativeApiError("missing associated name".into()))?
                    .name
                    .clone();
                Ok((name, self.ty(ty, span)?))
            })
            .collect::<Result<_, NativeApiError>>()?;
        if nominal.arguments.is_empty() && nominal.associated_types.is_empty() {
            return Ok(self.lowerer.alloc_type(
                span,
                TypeData {
                    kind: TypeKind::Named(name),
                },
            ));
        }
        Ok(self.lowerer.alloc_type(
            span,
            TypeData {
                kind: TypeKind::Generic {
                    name,
                    args,
                    bindings,
                    positional_after_binding: false,
                    callable_syntax: false,
                },
            },
        ))
    }
    fn ty(&mut self, ty: &AbiType, span: Span) -> Result<TypeRefId, NativeApiError> {
        let kind = match ty {
            AbiType::Builtin(BuiltinType::String) => TypeKind::Named(self.representation_name(
                NativeTypeConstructor::String,
                "String",
                span,
            )?),
            AbiType::Builtin(kind) => TypeKind::Named(
                builtin_type_spec(*kind)
                    .ok_or_else(|| NativeApiError("unknown native scalar".into()))?
                    .name
                    .into(),
            ),
            AbiType::Parameter { position, .. } => TypeKind::Named(format!("T{position}")),
            AbiType::SelfType(_) => TypeKind::Named("Self".into()),
            AbiType::Trait(ty) => return self.nominal_type(ty, span),
            AbiType::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => TypeKind::Projection {
                arguments: arguments
                    .iter()
                    .map(|ty| self.ty(ty, span))
                    .collect::<Result<_, _>>()?,
                receiver: self.ty(receiver, span)?,
                trait_ref: self.nominal_type(interface, span)?,
                member: member
                    .path
                    .last()
                    .ok_or_else(|| NativeApiError("missing associated name".into()))?
                    .name
                    .clone(),
            },
            AbiType::Array(item, access) => {
                let arg = self.ty(item, span)?;
                if *access == CollectionAccess::ReadOnly {
                    TypeKind::Array(arg)
                } else {
                    let name =
                        self.representation_name(NativeTypeConstructor::Array, "ArrayList", span)?;
                    TypeKind::Generic {
                        name,
                        args: [arg].into_iter().collect(),
                        bindings: vec![],
                        positional_after_binding: false,
                        callable_syntax: false,
                    }
                }
            }
            AbiType::Function { params, result } => TypeKind::Function {
                params: params
                    .iter()
                    .map(|ty| self.ty(ty, span))
                    .collect::<Result<_, _>>()?,
                result: self.ty(result, span)?,
            },
            AbiType::Iter(item) => TypeKind::Generic {
                name: self.representation_name(NativeTypeConstructor::Iter, "Iter", span)?,
                args: [self.ty(item, span)?].into_iter().collect(),
                bindings: vec![],
                positional_after_binding: false,
                callable_syntax: false,
            },
            AbiType::Tuple(items) => TypeKind::Tuple(
                items
                    .iter()
                    .map(|ty| self.ty(ty, span))
                    .collect::<Result<_, _>>()?,
            ),
            AbiType::StandardEnum { kind, args } if args.is_empty() => {
                TypeKind::Named(self.representation_name(
                    NativeTypeConstructor::Enum(*kind),
                    &format!("{kind:?}"),
                    span,
                )?)
            }
            AbiType::StandardEnum { kind, args } => TypeKind::Generic {
                name: self.representation_name(
                    NativeTypeConstructor::Enum(*kind),
                    &format!("{kind:?}"),
                    span,
                )?,
                args: args
                    .iter()
                    .map(|ty| self.ty(ty, span))
                    .collect::<Result<_, _>>()?,
                bindings: vec![],
                positional_after_binding: false,
                callable_syntax: false,
            },
            AbiType::Range(item, kind) => {
                let name = self.representation_name(
                    NativeTypeConstructor::Range(*kind),
                    kind.name(),
                    span,
                )?;
                if NativeTypeConstructor::Range(*kind).arity() == 0 {
                    TypeKind::Named(name)
                } else {
                    TypeKind::Generic {
                        name,
                        args: [self.ty(item, span)?].into_iter().collect(),
                        bindings: vec![],
                        positional_after_binding: false,
                        callable_syntax: false,
                    }
                }
            }
            _ => return Err(NativeApiError("unsupported native HIR type".into())),
        };
        Ok(self.lowerer.alloc_type(span, TypeData { kind }))
    }

    fn representation_name(
        &mut self,
        constructor: NativeTypeConstructor,
        fallback: &str,
        span: Span,
    ) -> Result<String, NativeApiError> {
        if let Some(owned) = self
            .definition
            .types
            .iter()
            .find(|ty| ty.kind == TypeAbiKind::Native(constructor))
        {
            return Ok(owned.name.clone());
        }
        let candidates: Vec<_> = self
            .providers
            .iter()
            .flat_map(|module| {
                module
                    .types
                    .iter()
                    .filter(move |ty| ty.kind == TypeAbiKind::Native(constructor))
                    .map(move |ty| (module, ty))
            })
            .collect();
        let preferred: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|(_, ty)| ty.name == fallback)
            .collect();
        let candidates = if preferred.is_empty() {
            &candidates
        } else {
            &preferred
        };
        let [(module, ty)] = candidates.as_slice() else {
            if candidates.is_empty() {
                // Carried source-owned declarations still resolve through their
                // actual installed prelude until their NR04 provider migrates.
                return Ok(fallback.into());
            }
            return Err(NativeApiError(
                "ambiguous installed native representation".into(),
            ));
        };
        let name = format!(
            "{}::{}::{}",
            module.identity.package.0,
            module.identity.path.join("::"),
            ty.name
        );
        if self.external_imports.insert(name.clone()) {
            self.lowerer.module.imports.push(Import {
                visibility: Visibility::Private,
                alias: name.clone(),
                path: name.clone(),
                span,
                glob: false,
            });
        }
        Ok(name)
    }
}
