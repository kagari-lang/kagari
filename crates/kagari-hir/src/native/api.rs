//! Direct declaration import. Generated CST is presentation only, not semantic input.
use crate::{
    hir::{
        ids::{BodyOwner, FunctionId, HirOwner, OpaqueTypeId, TypeRefId},
        item::{
            Item,
            adt::OpaqueType,
            behavior::{
                GenericParam, Impl, ImplMethod, ReceiverKind, TraitBound, TraitDef, TraitMethod,
                TraitRef,
            },
            function::{Function, FunctionKind, Param},
            storage::{Export, ExportItem, Visibility},
        },
        ty::{TypeData, TypeKind},
        writeability::Writeability,
    },
    lower::{LoweredModule, context::Lowerer},
    native::NativeTypeKind,
};
use kagari_abi::{
    callable::CallableImplementation,
    native_api::{NativeApiError, NativeModule, render::NativeApiSource},
    standard::surface::builtin_type_spec,
    types::{AbiType, ConstraintAbi, FunctionAbi, GenericParameterAbi, NominalAbiType},
};
use kagari_common::{
    cancellation::CancellationToken,
    collection::CollectionAccess,
    identity::{DefinitionId, DefinitionKind},
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
        generated: &generated,
        lowerer: Lowerer::new(cancel.clone()),
        native_types: HashMap::new(),
        native_functions: HashMap::new(),
    };
    importer.import_types()?;
    importer.import_traits()?;
    importer.import_implementations()?;
    importer.import_functions()?;
    let (module, source_map) = importer.lowerer.finish();
    Ok((
        parsed,
        Arc::new(LoweredModule {
            source,
            module,
            source_map,
            attributes: vec![],
            registered_native_api: true,
            native_types: importer.native_types,
            native_enums: HashMap::new(),
            native_functions: importer.native_functions,
            method_policies: HashMap::new(),
            native_attributes: HashSet::new(),
            installed_stdlib: None,
        }),
    ))
}

struct Importer<'a> {
    definition: &'a NativeModule,
    generated: &'a NativeApiSource,
    lowerer: Lowerer,
    native_types: HashMap<OpaqueTypeId, NativeTypeKind>,
    native_functions: HashMap<FunctionId, DefinitionId>,
}

impl Importer<'_> {
    fn import_types(&mut self) -> Result<(), NativeApiError> {
        let definition = self.definition;
        let generated = self.generated;
        for ty in &definition.types {
            let owner = definition.definition(DefinitionKind::AssociatedType, &ty.name);
            let site = &generated.sites[&owner];
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
            self.native_types.insert(id, NativeTypeKind::ArrayList);
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
                    has_default: false,
                });
            }
            self.lowerer.module.traits.push(TraitDef {
                id,
                visibility: Visibility::Public,
                name: item.name.clone(),
                generic_params,
                supertraits,
                methods,
                associated_types: vec![],
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
                    self.nominal_type(ty, site.name_span)
                        .map(|ty| TraitRef { ty })
                })
                .transpose()?;
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
            self.lowerer.module.impls.push(Impl {
                id,
                generic_params,
                trait_ref,
                for_type: Some(for_type),
                bounds: vec![],
                methods,
                associated_types: vec![],
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
            self.lowerer.module.exports.push(Export {
                name: function.name.clone(),
                item: ExportItem::Function(id),
            });
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
        let mut bounds = vec![];
        for (bound, site) in function.bounds.iter().zip(&site.bounds) {
            let target_ref = self.ty(&bound.ty, site.target)?;
            let traits = bound
                .constraints
                .iter()
                .zip(&site.constraints)
                .map(|(constraint, span)| {
                    let ConstraintAbi::Trait(trait_type) = constraint else {
                        return Err(NativeApiError("native bound requires a named trait".into()));
                    };
                    self.nominal_type(trait_type, *span)
                        .map(|ty| TraitRef { ty })
                })
                .collect::<Result<_, _>>()?;
            bounds.push(TraitBound {
                target: match &bound.ty {
                    AbiType::Parameter { position, .. } => format!("T{position}"),
                    _ => String::new(),
                },
                target_ref,
                traits,
            });
        }
        self.lowerer.source_map.set_owner(old);
        if let CallableImplementation::Native(binding) = &function.implementation {
            self.native_functions.insert(id, binding.clone());
        }
        self.lowerer.module.functions.push(Function {
            id,
            kind,
            visibility: if kind == FunctionKind::TraitMethod {
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
            let package = if module.package.0 == "kagari-std" {
                "std"
            } else {
                &module.package.0
            };
            format!("{}::{}::{}", package, module.path.join("::"), name.name)
        };
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
            AbiType::Builtin(kind) => TypeKind::Named(
                builtin_type_spec(*kind)
                    .ok_or_else(|| NativeApiError("unknown native scalar".into()))?
                    .name
                    .into(),
            ),
            AbiType::Parameter { position, .. } => TypeKind::Named(format!("T{position}")),
            AbiType::SelfType(_) => TypeKind::Named("Self".into()),
            AbiType::Trait(ty) => return self.nominal_type(ty, span),
            AbiType::Array(item, access) => {
                let arg = self.ty(item, span)?;
                if *access == CollectionAccess::ReadOnly {
                    TypeKind::Array(arg)
                } else {
                    let name = self
                        .definition
                        .types
                        .first()
                        .map(|ty| ty.name.clone())
                        .unwrap_or("ArrayList".into());
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
            AbiType::Tuple(items) => TypeKind::Tuple(
                items
                    .iter()
                    .map(|ty| self.ty(ty, span))
                    .collect::<Result<_, _>>()?,
            ),
            AbiType::StandardEnum { kind, args } if args.is_empty() => {
                TypeKind::Named(format!("{kind:?}"))
            }
            AbiType::StandardEnum { kind, args } => TypeKind::Generic {
                name: format!("{kind:?}"),
                args: args
                    .iter()
                    .map(|ty| self.ty(ty, span))
                    .collect::<Result<_, _>>()?,
                bindings: vec![],
                positional_after_binding: false,
                callable_syntax: false,
            },
            _ => return Err(NativeApiError("unsupported native HIR type".into())),
        };
        Ok(self.lowerer.alloc_type(span, TypeData { kind }))
    }
}
