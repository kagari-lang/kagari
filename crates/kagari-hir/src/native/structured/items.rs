//! Declaration and forwarding-body construction using the ordinary Lowerer allocator.
use super::StructuredLowerer;
use crate::hir::{
    attribute::{AttributeFact, AttributeForm, AttributeTarget, AttributeValue},
    expr::{ExprData, ExprKind},
    ids::{
        BodyOwner, ExprId, FieldId, FieldOwner, FunctionId, HirOwner, ParamId, TypeRefId, VariantId,
    },
    item::{
        Item,
        adt::{Enum, Field, NativeTypeDeclaration, Struct, Variant},
        behavior::{
            AssociatedType, GenericParam, Impl, ImplMethod, TraitDef, TraitMethod, TraitRef,
        },
        function::{Function, FunctionImplementation, FunctionKind, Param},
        module::{Import, ImportLeaf},
    },
    stmt::BlockData,
    ty::TypeKind,
    writeability::Writeability,
};
use kagari_common::{
    identity::{DefinitionKind, table::DefinitionId},
    span::Span,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, TypeDef, TypeDefKind,
        module::{DeclarationError, ModuleDecl},
        names::NameNamespace,
        script::ScriptExpr,
    },
    language::{Protocol, role::LangRole},
    visibility::Visibility,
};

impl StructuredLowerer<'_> {
    pub(super) fn module(&mut self) -> Result<(), DeclarationError> {
        for (name, target) in &self.declaration.exports {
            self.import(*target, Some(name.name.clone()), Some(name.namespace))?;
        }
        for ty in &self.declaration.types {
            self.type_definition(ty)?;
        }
        for contract in &self.declaration.traits {
            let declaration = self.declaration.definition(
                DefinitionKind::Trait,
                &contract.name,
                self.definitions,
            )?;
            self.lowerer.source_map.set_structured_origin(declaration);
            let id = self.lowerer.source_map.push_trait(Span::default());
            let generic_params =
                self.bounded_generics(&contract.generic_params, &contract.bounds)?;
            let mut methods = Vec::new();
            for method in &contract.methods {
                let owner = ModuleDecl::method_id(declaration, &method.name, self.definitions)?;
                let function = self.function(
                    owner,
                    method,
                    FunctionKind::TraitMethod,
                    &generic_params,
                    None,
                )?;
                self.lowerer.source_map.set_structured_origin(owner);
                methods.push(TraitMethod {
                    override_allowed: method.method_policy.override_allowed,
                    id: self.lowerer.source_map.push_trait_method(Span::default()),
                    name: method.name.clone(),
                    function,
                    has_default: matches!(
                        method.implementation,
                        CallableImplementation::Native(_) | CallableImplementation::Script
                    ),
                });
            }
            let mut associated_types = Vec::new();
            for member in &contract.associated_types {
                self.lowerer
                    .source_map
                    .set_structured_origin(member.declaration);
                let name = self.name(member.declaration)?;
                let name_ref = self.alloc_type(TypeKind::Named(name.clone()));
                associated_types.push(AssociatedType {
                    name,
                    name_ref,
                    generic_params: self.generics(&member.generic_params)?,
                    parameter_bounds: self.bounds(&member.parameter_bounds)?,
                    ty: None,
                    bounds: self.constraints(&member.bounds)?,
                });
            }
            if !contract.associated_consts.is_empty() {
                return Err(DeclarationError(
                    "structured associated constants require complete supplied Script source"
                        .into(),
                ));
            }
            self.lowerer.source_map.set_structured_origin(declaration);
            let supertraits = self.constraints(&contract.supertraits)?;
            self.lowerer.module.traits.push(TraitDef {
                conversion_adapter: contract.conversion_adapter.clone(),
                id,
                name: contract.name.clone(),
                visibility: Visibility::Public,
                generic_params,
                methods,
                associated_types,
                supertraits,
                associated_consts: vec![],
            });
            self.item(declaration, Item::Trait(id));
            if let Some(role) = Protocol::from_definition(&declaration, self.definitions)
                .and_then(LangRole::from_protocol)
            {
                self.lowerer.module.attributes.push(AttributeFact {
                    name: "lang".into(),
                    form: AttributeForm::Value(AttributeValue::Literal(format!(
                        "\"{}\"",
                        role.name()
                    ))),
                    span: Span::default(),
                    target: AttributeTarget::Item(Item::Trait(id)),
                });
            }
        }
        for (index, implementation) in self.declaration.implementations.iter().enumerate() {
            let declaration = self
                .declaration
                .implementation_id(index, self.definitions)?;
            self.lowerer.source_map.set_structured_origin(declaration);
            let id = self.lowerer.source_map.push_impl(Span::default());
            let generic_params = self.generics(&implementation.generic_params)?;
            let for_type = Some(self.ty(&implementation.for_type)?);
            let trait_ref = implementation
                .trait_type
                .as_ref()
                .map(|interface| {
                    let mut header = interface.clone();
                    header.associated_types.clear();
                    Ok(TraitRef {
                        ty: self.nominal(&header)?,
                    })
                })
                .transpose()?;
            let bounds = self.bounds(&implementation.bounds)?;
            let mut associated_types = Vec::new();
            if let Some(interface) = &implementation.trait_type {
                for (member, value) in &interface.associated_types {
                    let name = self.name(*member)?;
                    let owner = self
                        .definitions
                        .lookup_child(declaration, DefinitionKind::AssociatedType, &name, 0)
                        .ok_or_else(|| {
                            DeclarationError("missing implementation associated identity".into())
                        })?;
                    self.lowerer.source_map.set_structured_origin(owner);
                    let name_ref = self.alloc_type(TypeKind::Named(name.clone()));
                    associated_types.push(AssociatedType {
                        name,
                        name_ref,
                        ty: Some(self.ty(value)?),
                        generic_params: vec![],
                        parameter_bounds: vec![],
                        bounds: vec![],
                    });
                }
            }
            let mut methods = Vec::new();
            for method in &implementation.methods {
                let owner = ModuleDecl::method_id(declaration, &method.name, self.definitions)?;
                methods.push(ImplMethod {
                    name: method.name.clone(),
                    function: self.function(
                        owner,
                        method,
                        FunctionKind::ImplMethod,
                        &generic_params,
                        for_type,
                    )?,
                });
            }
            self.lowerer.module.impls.push(Impl {
                id,
                generic_params,
                trait_ref,
                for_type,
                bounds,
                methods,
                associated_types,
                associated_consts: vec![],
            });
            self.item(declaration, Item::Impl(id));
        }
        for function in &self.declaration.functions {
            let declaration = self.declaration.definition(
                DefinitionKind::Function,
                &function.name,
                self.definitions,
            )?;
            let id = self.function(declaration, function, FunctionKind::User, &[], None)?;
            self.lowerer.module.items.push(Item::Function(id));
        }
        Ok(())
    }

    fn item(&mut self, declaration: DefinitionId, item: Item) {
        assert!(
            self.correspondence
                .items
                .insert(declaration, item)
                .is_none()
        );
        self.lowerer.module.items.push(item);
    }

    fn import(
        &mut self,
        declaration: DefinitionId,
        alias: Option<String>,
        namespace: Option<NameNamespace>,
    ) -> Result<(), DeclarationError> {
        let view = self
            .definitions
            .resolve(declaration)
            .map_err(|error| DeclarationError(error.to_string()))?;
        let path = if view.module() == &self.declaration.identity {
            format!("self::{}", self.path(declaration)?)
        } else {
            self.path(declaration)?
        };
        self.lowerer.module.imports.push(Import {
            namespace,
            kind: ImportLeaf::Named { alias },
            visibility: Visibility::Public,
            segments: path.split("::").map(str::to_owned).collect(),
            root_span: Span::default(),
            span: Span::default(),
        });
        Ok(())
    }

    fn type_definition(&mut self, ty: &TypeDef) -> Result<(), DeclarationError> {
        let declaration =
            self.declaration
                .definition(ty.kind.definition_kind(), &ty.name, self.definitions)?;
        self.lowerer.source_map.set_structured_origin(declaration);
        let generic_params = if matches!(ty.kind, TypeDefKind::Struct | TypeDefKind::Enum) {
            self.bounded_generics(&ty.generic_params, &ty.bounds)?
        } else {
            self.generics(&ty.generic_params)?
        };
        let item = match ty.kind {
            TypeDefKind::Struct => {
                let id = self.lowerer.source_map.push_struct(Span::default());
                let fields = self.fields(declaration, FieldOwner::Struct(id), ty)?;
                self.lowerer.module.structs.push(Struct {
                    id,
                    visibility: Visibility::Public,
                    name: ty.name.clone(),
                    generic_params,
                    fields,
                    impls: vec![],
                });
                Item::Struct(id)
            }
            TypeDefKind::Enum => {
                let id = self.lowerer.source_map.push_enum(Span::default());
                let mut variants = Vec::new();
                for (slot, variant) in ty.variants.iter().enumerate() {
                    let owner =
                        ModuleDecl::variant_id(declaration, &variant.name, self.definitions)?;
                    self.lowerer.source_map.set_structured_origin(owner);
                    let member = VariantId::new(self.lowerer.source_map.arena(), id, slot);
                    self.lowerer
                        .source_map
                        .insert_variant(member, Span::default());
                    let payload = variant
                        .payload
                        .iter()
                        .map(|ty| self.ty(ty))
                        .collect::<Result<_, _>>()?;
                    variants.push(Variant {
                        reports_failure: variant.reports_failure,
                        id: member,
                        name: variant.name.clone(),
                        payload,
                    });
                    if self.declaration.variant_exports.contains(&ty.name) {
                        self.import(owner, None, None)?;
                    }
                }
                self.lowerer.module.enums.push(Enum {
                    id,
                    visibility: Visibility::Public,
                    name: ty.name.clone(),
                    generic_params,
                    variants,
                    impls: vec![],
                });
                Item::Enum(id)
            }
            _ => {
                let id = self.lowerer.source_map.push_native_type(Span::default());
                let fields = self.fields(declaration, FieldOwner::NativeType(id), ty)?;
                self.lowerer.source_map.set_structured_origin(declaration);
                let bounds = self.bounds(&ty.bounds)?;
                self.lowerer
                    .module
                    .native_types
                    .push(NativeTypeDeclaration {
                        host: None,
                        id,
                        visibility: Visibility::Public,
                        name: ty.name.clone(),
                        generic_params,
                        fields,
                        bounds,
                        trait_bounds: vec![],
                    });
                Item::NativeType(id)
            }
        };
        self.item(declaration, item);
        Ok(())
    }

    fn fields(
        &mut self,
        declaration: DefinitionId,
        owner: FieldOwner,
        ty: &TypeDef,
    ) -> Result<Vec<Field>, DeclarationError> {
        ty.fields
            .iter()
            .enumerate()
            .map(|(slot, field)| {
                let declaration = self
                    .definitions
                    .lookup_child(declaration, DefinitionKind::Field, &field.name, 0)
                    .ok_or_else(|| DeclarationError("missing registered field identity".into()))?;
                self.lowerer.source_map.set_structured_origin(declaration);
                let id = FieldId::new(self.lowerer.source_map.arena(), owner, slot);
                self.lowerer.source_map.insert_field(id, Span::default());
                Ok(Field {
                    id,
                    visibility: field.visibility,
                    name: field.name.clone(),
                    writeability: if field.mutable {
                        Writeability::Var
                    } else {
                        Writeability::Val
                    },
                    ty: self.ty(&field.ty)?,
                })
            })
            .collect()
    }

    fn function(
        &mut self,
        declaration: DefinitionId,
        function: &FnDecl,
        kind: FunctionKind,
        inherited: &[GenericParam],
        receiver: Option<TypeRefId>,
    ) -> Result<FunctionId, DeclarationError> {
        self.lowerer.source_map.set_structured_origin(declaration);
        let id = self.lowerer.reserve_function(Span::default());
        assert!(
            self.correspondence
                .items
                .insert(declaration, Item::Function(id))
                .is_none()
        );
        let owner = HirOwner::Body(BodyOwner::Function(id));
        let previous = self.lowerer.source_map.set_owner(owner);
        let own = function
            .generic_params
            .iter()
            .filter(|parameter| inherited.is_empty() || parameter.owner == declaration)
            .cloned()
            .collect::<Vec<_>>();
        let mut generic_params = inherited.to_vec();
        generic_params.extend(self.generics(&own)?);
        let params = function
            .params
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                let id = ParamId::new(self.lowerer.source_map.arena(), owner, index);
                self.lowerer.source_map.insert_param(id, Span::default());
                // Shared signature checking resolves the implicit impl receiver once
                // at its header; retain that exact reference, including its binder owner.
                let ty = if parameter.name == "self" {
                    match receiver {
                        Some(receiver) => receiver,
                        None => self.ty(&parameter.ty)?,
                    }
                } else {
                    self.ty(&parameter.ty)?
                };
                Ok(Param {
                    id,
                    name: parameter.name.clone(),
                    ty,
                })
            })
            .collect::<Result<_, DeclarationError>>()?;
        let return_type = Some(self.ty(&function.return_type)?);
        let bounds = self.bounds(&function.bounds)?;
        let implementation = match &function.implementation {
            CallableImplementation::Script => {
                let expression = self
                    .declaration
                    .script_bodies
                    .get(&declaration)
                    .ok_or_else(|| {
                        DeclarationError("Script registration requires a complete body".into())
                    })?;
                let tail = self.script_expr(expression)?;
                let block = self.lowerer.alloc_block(
                    Span::default(),
                    BlockData {
                        statements: Default::default(),
                        tail_expr: Some(tail),
                    },
                );
                FunctionImplementation::Script(block)
            }
            // The installed contract is attached by exact allocation correspondence.
            CallableImplementation::Native(_) | CallableImplementation::Required => {
                FunctionImplementation::Required
            }
        };
        self.lowerer.finish_function(Function {
            id,
            kind,
            is_async: false,
            visibility: if kind == FunctionKind::TraitMethod
                || self.declaration.private_functions.contains(&declaration)
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
            implementation,
        });
        self.lowerer.source_map.set_owner(previous);
        Ok(id)
    }

    fn script_expr(&mut self, expression: &ScriptExpr) -> Result<ExprId, DeclarationError> {
        let kind = match expression {
            ScriptExpr::Name(name) => ExprKind::Name {
                name: name.clone(),
                explicit_type: None,
            },
            ScriptExpr::Definition(id) => ExprKind::Name {
                name: {
                    let path = self.path(*id)?;
                    if self
                        .definitions
                        .resolve(*id)
                        .map_err(|error| DeclarationError(error.to_string()))?
                        .module()
                        == &self.declaration.identity
                    {
                        format!("self::{path}")
                    } else {
                        path
                    }
                },
                explicit_type: None,
            },
            ScriptExpr::Call {
                callee,
                type_arguments,
                arguments,
            } => ExprKind::Call {
                callee: self.script_expr(callee)?,
                args: arguments
                    .iter()
                    .map(|argument| self.script_expr(argument))
                    .collect::<Result<_, _>>()?,
                type_args: if type_arguments.is_empty() {
                    None
                } else {
                    Some(
                        type_arguments
                            .iter()
                            .map(|ty| self.ty(ty))
                            .collect::<Result<_, _>>()?,
                    )
                },
            },
        };
        Ok(self.lowerer.alloc_expr(Span::default(), ExprData { kind }))
    }
}
