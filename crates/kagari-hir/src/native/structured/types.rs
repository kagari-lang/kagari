//! Registration type expressions become unchecked type references, never checked Ty facts.
use super::StructuredLowerer;
use crate::{
    collections::HashMap,
    hir::{
        ids::{GenericParamId, TypeRefId},
        item::behavior::{GenericParam, TraitBound, TraitRef},
        ty::{TypeBuffer, TypeKind},
    },
};
use kagari_common::{
    identity::{DefinitionKind, table::DefinitionId},
    span::Span,
};
use kagari_types::{
    declaration::{module::DeclarationError, native::NativeTypeConstructor},
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam as RegisteredParameter, NominalTy, Ty},
};
use std::{collections::BTreeMap, slice};

impl StructuredLowerer<'_> {
    pub(super) fn prepare_parameters(&mut self) -> Result<(), DeclarationError> {
        let module = self.declaration;
        let mut counts = HashMap::<DefinitionId, usize>::default();
        let parameters = module
            .types
            .iter()
            .flat_map(|item| &item.generic_params)
            .chain(module.traits.iter().flat_map(|item| &item.generic_params))
            .chain(
                module
                    .implementations
                    .iter()
                    .flat_map(|item| &item.generic_params),
            )
            .chain(
                module
                    .functions
                    .iter()
                    .flat_map(|item| &item.generic_params),
            )
            .chain(
                module
                    .traits
                    .iter()
                    .flat_map(|item| &item.methods)
                    .flat_map(|item| &item.generic_params),
            )
            .chain(
                module
                    .implementations
                    .iter()
                    .flat_map(|item| &item.methods)
                    .flat_map(|item| &item.generic_params),
            )
            .chain(
                module
                    .traits
                    .iter()
                    .flat_map(|item| &item.associated_types)
                    .flat_map(|item| &item.generic_params),
            );
        for parameter in parameters {
            let count = counts.entry(parameter.owner).or_default();
            *count = (*count).max(parameter.position + 1);
        }
        for (owner, count) in &counts {
            let view = self
                .definitions
                .resolve(*owner)
                .map_err(|error| DeclarationError(error.to_string()))?;
            let parent = self
                .definitions
                .parent(*owner)
                .map_err(|error| DeclarationError(error.to_string()))?;
            let method = view
                .last()
                .is_some_and(|part| part.kind == DefinitionKind::Method);
            let prefix = if method && parent.is_some_and(|id| counts.contains_key(&id)) {
                "M"
            } else {
                "T"
            };
            for position in 0..*count {
                let name = if *count == 1 {
                    prefix.into()
                } else {
                    format!("{prefix}{}", position + 1)
                };
                self.parameter_names.insert((*owner, position), name);
            }
        }
        Ok(())
    }

    pub(super) fn name(&self, id: DefinitionId) -> Result<String, DeclarationError> {
        self.definitions
            .resolve(id)
            .map_err(|error| DeclarationError(error.to_string()))?
            .last()
            .map(|part| part.name.to_owned())
            .ok_or_else(|| DeclarationError("missing declaration name".into()))
    }

    pub(super) fn path(&self, id: DefinitionId) -> Result<String, DeclarationError> {
        let view = self
            .definitions
            .resolve(id)
            .map_err(|error| DeclarationError(error.to_string()))?;
        Ok(if view.module() == &self.declaration.identity {
            view.segments()
                .map(|part| part.name)
                .collect::<Vec<_>>()
                .join("::")
        } else {
            self.paths.definition_path(view)
        })
    }

    pub(super) fn alloc_type(&mut self, kind: TypeKind) -> TypeRefId {
        self.lowerer.alloc_type(Span::default(), kind)
    }

    pub(super) fn generics(
        &mut self,
        parameters: &[RegisteredParameter],
    ) -> Result<Vec<GenericParamId>, DeclarationError> {
        parameters
            .iter()
            .map(|parameter| {
                let name = self
                    .parameter_names
                    .get(&(parameter.owner, parameter.position))
                    .cloned()
                    .ok_or_else(|| {
                        DeclarationError("unbound registered generic parameter".into())
                    })?;
                Ok(self.lowerer.alloc_generic_param(GenericParam {
                    origin: self.lowerer.origin(Span::default()),
                    name_span: None,
                    name,
                    bounds: Vec::new(),
                }))
            })
            .collect()
    }

    /// Struct/enum/trait headers own inline binder bounds in the common HIR.
    /// Reject unrepresentable predicates rather than silently dropping constraints.
    pub(super) fn bounded_generics(
        &mut self,
        parameters: &[RegisteredParameter],
        bounds: &[GenericBound],
    ) -> Result<Vec<GenericParamId>, DeclarationError> {
        let generic_params = self.generics(parameters)?;
        for bound in bounds {
            let Ty::Parameter { owner, position } = bound.ty else {
                return Err(DeclarationError(
                    "structured header bound requires a generic parameter target".into(),
                ));
            };
            let index = parameters
                .iter()
                .position(|parameter| parameter.owner == owner && parameter.position == position)
                .ok_or_else(|| {
                    DeclarationError("header bound target is not a declared binder".into())
                })?;
            let constraints = self.constraints(&bound.constraints)?;
            self.lowerer.module.generic_params[generic_params[index].index()]
                .bounds
                .extend(constraints);
        }
        Ok(generic_params)
    }

    pub(super) fn bounds(
        &mut self,
        bounds: &[GenericBound],
    ) -> Result<Vec<TraitBound>, DeclarationError> {
        bounds
            .iter()
            .map(|bound| {
                let target_ref = self.ty(&bound.ty)?;
                Ok(TraitBound {
                    target: self.type_label(target_ref),
                    target_ref,
                    traits: self.constraints(&bound.constraints)?,
                })
            })
            .collect()
    }

    fn type_label(&self, id: TypeRefId) -> String {
        match &self.lowerer.module.type_ref(id).kind {
            TypeKind::Named(name) | TypeKind::Definition { name, .. } => name.clone(),
            _ => "<structured type>".into(),
        }
    }

    pub(super) fn constraints(
        &mut self,
        constraints: &[NominalTy],
    ) -> Result<Vec<TraitRef>, DeclarationError> {
        constraints
            .iter()
            .map(|constraint| {
                Ok(TraitRef {
                    ty: self.nominal(constraint)?,
                })
            })
            .collect()
    }

    fn constructor(
        &self,
        constructor: NativeTypeConstructor,
    ) -> Result<DefinitionId, DeclarationError> {
        self.definitions
            .lookup(&constructor.declaration())
            .ok_or_else(|| DeclarationError("missing scoped language constructor reference".into()))
    }

    fn application(
        &mut self,
        declaration: DefinitionId,
        arguments: &[Ty],
        outputs: &BTreeMap<DefinitionId, Ty>,
    ) -> Result<TypeRefId, DeclarationError> {
        let name = self.path(declaration)?;
        let args = arguments
            .iter()
            .map(|ty| self.ty(ty))
            .collect::<Result<TypeBuffer, _>>()?;
        let bindings = outputs
            .iter()
            .map(|(id, ty)| Ok((self.name(*id)?, self.ty(ty)?)))
            .collect::<Result<_, DeclarationError>>()?;
        Ok(self.alloc_type(TypeKind::Definition {
            declaration,
            name,
            // ModuleDecl stores identity, not an authored exposure. The actual
            // analysis catalog selects and retains the access-checked route.
            exposure: None,
            args,
            bindings,
        }))
    }

    pub(super) fn nominal(&mut self, nominal: &NominalTy) -> Result<TypeRefId, DeclarationError> {
        self.application(
            nominal.declaration,
            &nominal.arguments,
            &nominal.associated_types,
        )
    }

    pub(super) fn ty(&mut self, ty: &Ty) -> Result<TypeRefId, DeclarationError> {
        self.lowerer
            .cancel
            .check()
            .map_err(|_| DeclarationError("native type lowering cancelled".into()))?;
        let kind = match ty {
            Ty::Builtin(BuiltinType::String) => {
                let declaration = self.constructor(NativeTypeConstructor::String)?;
                return self.application(declaration, &[], &BTreeMap::new());
            }
            Ty::Builtin(kind) => TypeKind::Named(
                match kind {
                    BuiltinType::Never => "!",
                    BuiltinType::Unit => "()",
                    BuiltinType::Bool => "bool",
                    BuiltinType::Char => "char",
                    BuiltinType::I8 => "i8",
                    BuiltinType::I16 => "i16",
                    BuiltinType::I32 => "i32",
                    BuiltinType::I64 => "i64",
                    BuiltinType::U8 => "u8",
                    BuiltinType::U16 => "u16",
                    BuiltinType::U32 => "u32",
                    BuiltinType::U64 => "u64",
                    BuiltinType::F32 => "f32",
                    BuiltinType::F64 => "f64",
                    BuiltinType::String => unreachable!(),
                }
                .into(),
            ),
            Ty::Parameter { owner, position } => TypeKind::Named(
                self.parameter_names
                    .get(&(*owner, *position))
                    .cloned()
                    .ok_or_else(|| {
                        DeclarationError("unbound registered generic parameter".into())
                    })?,
            ),
            Ty::SelfType(_) => TypeKind::Named("Self".into()),
            Ty::Host(declaration) => return self.application(*declaration, &[], &BTreeMap::new()),
            Ty::Struct(nominal)
            | Ty::Enum(nominal)
            | Ty::NativeObject(nominal)
            | Ty::Trait(nominal) => return self.nominal(nominal),
            Ty::Range(element, range) => {
                let constructor = NativeTypeConstructor::Range(*range);
                let declaration = self.constructor(constructor)?;
                let args = if constructor.arity() == 0 {
                    &[][..]
                } else {
                    slice::from_ref(element.as_ref())
                };
                return self.application(declaration, args, &BTreeMap::new());
            }
            Ty::Tuple(elements) => TypeKind::Tuple(
                elements
                    .iter()
                    .map(|ty| self.ty(ty))
                    .collect::<Result<_, _>>()?,
            ),
            Ty::Array(element) => TypeKind::Array(self.ty(element)?),
            Ty::Function { params, result } => TypeKind::Function {
                params: params
                    .iter()
                    .map(|ty| self.ty(ty))
                    .collect::<Result<_, _>>()?,
                result: self.ty(result)?,
            },
            Ty::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => TypeKind::Projection {
                receiver: self.ty(receiver)?,
                trait_ref: self.nominal(interface)?,
                member: self.name(*member)?,
                arguments: arguments
                    .iter()
                    .map(|ty| self.ty(ty))
                    .collect::<Result<_, _>>()?,
            },
        };
        Ok(self.alloc_type(kind))
    }
}
