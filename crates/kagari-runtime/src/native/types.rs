//! Typed registration inputs retain scoped IDs and an immutable root-level owner.
use crate::{
    error::RuntimeError,
    native::binding::{Codec, NativeResult},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        map::DefinitionContext,
        mapping::DefinitionMapper,
        metadata::DefinitionMetadata,
        scoped::RemapDefinitions,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{TraitDef, TypeDef, TypeDefKind, module::ModuleDecl},
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

/// A semantic type input. Only this outer handle retains a table; nested nodes use IDs.
/// Identity-free scalar/composite values need no identity table allocation.
#[derive(Debug, Clone)]
pub struct Type {
    ty: Ty,
    definitions: Option<DefinitionTable>,
}

impl Type {
    /// Retain a checked identity scope, without granting semantic verification.
    pub fn from_semantic(ty: Ty, definitions: &DefinitionTable) -> NativeResult<Self> {
        DefinitionMetadata::checked(
            definitions.clone(),
            ty.clone(),
            &CancellationToken::default(),
        )
        .map_err(conflict)?;
        Ok(Self {
            ty,
            definitions: Some(definitions.clone()),
        })
    }

    pub fn scalar(kind: BuiltinType) -> Self {
        Self {
            ty: Ty::Builtin(kind),
            definitions: None,
        }
    }

    pub fn unit() -> Self {
        Self::scalar(BuiltinType::Unit)
    }

    pub fn bool() -> Self {
        Self::scalar(BuiltinType::Bool)
    }

    pub fn i32() -> Self {
        Self::scalar(BuiltinType::I32)
    }

    pub fn i64() -> Self {
        Self::scalar(BuiltinType::I64)
    }

    pub fn usize() -> Self {
        Self::scalar(BuiltinType::USize)
    }

    pub fn u64() -> Self {
        Self::scalar(BuiltinType::U64)
    }

    pub fn f64() -> Self {
        Self::scalar(BuiltinType::F64)
    }

    pub fn tuple(items: impl IntoIterator<Item = Type>) -> NativeResult<Self> {
        Self::assemble(items.into_iter().collect(), |types| Ok(Ty::Tuple(types)))
    }

    pub fn function(params: impl IntoIterator<Item = Type>, result: Type) -> NativeResult<Self> {
        let mut types = params.into_iter().collect::<Vec<_>>();
        types.push(result);
        Self::assemble(types, |mut types| {
            let result = Box::new(types.pop().expect("function result"));
            Ok(Ty::Function {
                params: types,
                result,
            })
        })
    }

    /// Construct an iterator input, retaining or adopting its element identity scope.
    pub fn iterator(item: Type) -> NativeResult<Self> {
        Self::assemble(vec![item], |mut types| {
            Ok(Ty::Iter(Box::new(types.pop().expect("iterator item"))))
        })
    }

    /// Construct a map input after adopting both key and value into one scope.
    pub fn map(key: Type, value: Type, access: CollectionAccess) -> NativeResult<Self> {
        Self::assemble(vec![key, value], |mut types| {
            let value = Box::new(types.pop().expect("map value"));
            let key = Box::new(types.pop().expect("map key"));
            Ok(Ty::Map { key, value, access })
        })
    }

    /// Construct a set input in its element scope with explicit collection access.
    pub fn set(item: Type, access: CollectionAccess) -> NativeResult<Self> {
        Self::assemble(vec![item], |mut types| {
            Ok(Ty::Set(Box::new(types.pop().expect("set item")), access))
        })
    }

    pub fn abi(&self) -> &Ty {
        &self.ty
    }

    /// None denotes an identity-free type, not an alternate identity representation.
    pub fn definitions(&self) -> Option<&DefinitionTable> {
        self.definitions.as_ref()
    }

    pub(crate) fn adopt(&self, context: &DefinitionContext) -> NativeResult<Ty> {
        match &self.definitions {
            None => Ok(self.ty.clone()),
            Some(definitions) if definitions.id() == context.snapshot().id() => Ok(self.ty.clone()),
            Some(definitions) => DefinitionMetadata::checked(
                definitions.clone(),
                self.ty.clone(),
                &CancellationToken::default(),
            )
            .and_then(|input| input.import_into(context, &CancellationToken::default()))
            .map(|input| input.into_records())
            .map_err(conflict),
        }
    }

    pub(crate) fn same_type(&self, other: &Self) -> bool {
        Self::compose(vec![self.clone(), other.clone()])
            .is_ok_and(|(_, types)| types[0] == types[1])
    }

    fn compose(inputs: Vec<Self>) -> NativeResult<(Option<DefinitionTable>, Vec<Ty>)> {
        let mut definitions: Option<DefinitionTable> = None;
        let mut foreign = false;
        for input in &inputs {
            if let Some(scope) = &input.definitions {
                match &definitions {
                    None => definitions = Some(scope.clone()),
                    Some(current) if current.id() != scope.id() => foreign = true,
                    Some(current) if current.len() < scope.len() => {
                        definitions = Some(scope.clone())
                    }
                    _ => {}
                }
            }
        }
        if foreign {
            let context = DefinitionContext::new().map_err(conflict)?;
            let types = inputs
                .iter()
                .map(|input| input.adopt(&context))
                .collect::<NativeResult<_>>()?;
            Ok((Some(context.snapshot()), types))
        } else {
            Ok((
                definitions,
                inputs.into_iter().map(|input| input.ty).collect(),
            ))
        }
    }

    fn assemble(
        inputs: Vec<Self>,
        build: impl FnOnce(Vec<Ty>) -> NativeResult<Ty>,
    ) -> NativeResult<Self> {
        let (definitions, types) = Self::compose(inputs)?;
        Ok(Self {
            ty: build(types)?,
            definitions,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ParameterRef {
    pub(crate) ty: Type,
}

impl ParameterRef {
    pub fn ty(&self) -> Type {
        self.ty.clone()
    }
}

#[derive(Debug, Clone)]
pub struct TypeRef {
    pub(crate) id: DefinitionId,
    pub(crate) definitions: DefinitionTable,
    pub(crate) declaration: Arc<TypeDef>,
    pub(crate) parameter_names: Vec<String>,
}

impl TypeRef {
    pub(crate) fn adopt(&self, context: &DefinitionContext) -> NativeResult<Self> {
        let (id, declaration, _) = adopt_reference(
            self.id,
            &self.definitions,
            self.declaration.as_ref(),
            context,
        )?;
        for variant in &declaration.variants {
            context
                .intern_child(id, DefinitionKind::Variant, &variant.name, 0)
                .map_err(conflict)?;
        }
        Ok(Self {
            id,
            definitions: context.snapshot(),
            declaration: Arc::new(declaration),
            parameter_names: self.parameter_names.clone(),
        })
    }

    pub fn id(&self) -> DefinitionId {
        self.id
    }

    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    pub fn codec(&self) -> Codec {
        match self.declaration.kind {
            TypeDefKind::Enum => Codec::Value,
            _ => Codec::Object {
                declaration: self.id,
                definitions: self.definitions.clone(),
            },
        }
    }

    pub fn apply(&self, arguments: impl IntoIterator<Item = Type>) -> NativeResult<Type> {
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        if arguments.len() != self.declaration.generic_params.len() {
            return Err(conflict("native type argument count"));
        }
        let nominal = NominalTy {
            declaration: self.id,
            arguments: vec![],
            associated_types: BTreeMap::new(),
        };
        let root = match self.declaration.kind {
            TypeDefKind::Enum => Ty::Enum(nominal),
            TypeDefKind::NativeStorage(_) => Ty::NativeObject(nominal),
            _ => return Err(conflict("unsupported authoring type kind")),
        };
        let mut inputs = vec![Type::from_semantic(root, &self.definitions)?];
        inputs.extend(arguments);
        Type::assemble(inputs, |mut inputs| {
            let mut result = inputs.remove(0);
            match &mut result {
                Ty::Enum(nominal) | Ty::NativeObject(nominal) => nominal.arguments = inputs,
                _ => unreachable!("nominal input"),
            }
            Ok(result)
        })
    }

    /// Member lookup preserves the enum declaration's retained identity scope.
    pub fn variant(&self, name: &str) -> NativeResult<VariantRef> {
        if self.declaration.kind != TypeDefKind::Enum
            || !self
                .declaration
                .variants
                .iter()
                .any(|variant| variant.name == name)
        {
            return Err(conflict("unknown enum variant"));
        }
        Ok(VariantRef {
            id: ModuleDecl::variant_id(self.id, name, &self.definitions).map_err(conflict)?,
            definitions: self.definitions.clone(),
        })
    }
}

/// Execution still validates the selected generation, owner and payload layout.
#[derive(Debug, Clone)]
pub struct VariantRef {
    pub(crate) id: DefinitionId,
    pub(crate) definitions: DefinitionTable,
}

impl VariantRef {
    pub fn id(&self) -> DefinitionId {
        self.id
    }

    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }
}

pub enum Receiver {
    Concrete(Type),
    Declaration(TypeRef),
}

impl From<Type> for Receiver {
    fn from(ty: Type) -> Self {
        Self::Concrete(ty)
    }
}

impl From<TypeRef> for Receiver {
    fn from(ty: TypeRef) -> Self {
        Self::Declaration(ty)
    }
}

#[derive(Debug, Clone)]
pub struct TraitRef {
    pub(crate) id: DefinitionId,
    pub(crate) definitions: DefinitionTable,
    pub(crate) contract: Arc<TraitDef>,
}

impl TraitRef {
    pub(crate) fn adopt(&self, context: &DefinitionContext) -> NativeResult<Self> {
        let (id, contract, _) =
            adopt_reference(self.id, &self.definitions, self.contract.as_ref(), context)?;
        for method in &contract.methods {
            context
                .intern_child(id, DefinitionKind::Method, &method.name, 0)
                .map_err(conflict)?;
        }
        Ok(Self {
            id,
            definitions: context.snapshot(),
            contract: Arc::new(contract),
        })
    }

    pub fn apply(&self, arguments: impl IntoIterator<Item = Type>) -> NativeResult<AppliedTrait> {
        let mut inputs = vec![Type::from_semantic(
            Ty::Trait(NominalTy {
                declaration: self.id,
                arguments: vec![],
                associated_types: BTreeMap::new(),
            }),
            &self.definitions,
        )?];
        inputs.extend(arguments);
        let applied = Type::assemble(inputs, |mut inputs| {
            let Ty::Trait(mut nominal) = inputs.remove(0) else {
                unreachable!("trait input")
            };
            nominal.arguments = inputs;
            Ok(Ty::Trait(nominal))
        })?;
        let Ty::Trait(ty) = applied.ty else {
            unreachable!("trait input")
        };
        Ok(AppliedTrait {
            contract: self.clone(),
            ty,
            definitions: applied.definitions.expect("nominal scope"),
        })
    }

    pub fn method(&self, name: &str) -> NativeResult<MethodRef> {
        if !self
            .contract
            .methods
            .iter()
            .any(|method| method.name == name)
        {
            return Err(conflict("unknown Kagari trait method"));
        }
        Ok(MethodRef {
            owner: self.clone(),
            id: ModuleDecl::method_id(self.id, name, &self.definitions).map_err(conflict)?,
        })
    }

    pub fn receiver(&self) -> Type {
        Type {
            ty: Ty::SelfType(self.id),
            definitions: Some(self.definitions.clone()),
        }
    }

    pub fn id(&self) -> DefinitionId {
        self.id
    }

    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }
}

#[derive(Debug, Clone)]
pub struct AppliedTrait {
    pub(crate) contract: TraitRef,
    pub(crate) ty: NominalTy,
    pub(crate) definitions: DefinitionTable,
}

impl AppliedTrait {
    pub fn associated(mut self, name: &str, ty: Type) -> NativeResult<Self> {
        let id = self
            .contract
            .definitions
            .lookup_child(self.contract.id, DefinitionKind::AssociatedType, name, 0)
            .ok_or_else(|| conflict("unknown associated type"))?;
        if !self
            .contract
            .contract
            .associated_types
            .iter()
            .any(|member| member.declaration == id)
        {
            return Err(conflict("unknown associated type"));
        }
        let member = Type::from_semantic(Ty::SelfType(id), &self.contract.definitions)?;
        let applied = Type::assemble(vec![self.ty(), ty, member], |mut inputs| {
            let Ty::SelfType(id) = inputs.pop().expect("member") else {
                unreachable!("member input")
            };
            let value = inputs.pop().expect("associated output");
            let Ty::Trait(mut nominal) = inputs.pop().expect("trait") else {
                unreachable!("trait input")
            };
            if nominal.associated_types.insert(id, value).is_some() {
                return Err(conflict("duplicate associated type"));
            }
            Ok(Ty::Trait(nominal))
        })?;
        let Ty::Trait(ty) = applied.ty else {
            unreachable!("trait input")
        };
        self.ty = ty;
        self.definitions = applied.definitions.expect("nominal scope");
        Ok(self)
    }

    pub fn ty(&self) -> Type {
        Type {
            ty: Ty::Trait(self.ty.clone()),
            definitions: Some(self.definitions.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MethodRef {
    pub(crate) owner: TraitRef,
    pub(crate) id: DefinitionId,
}

impl MethodRef {
    pub fn id(&self) -> DefinitionId {
        self.id
    }

    pub fn definitions(&self) -> &DefinitionTable {
        &self.owner.definitions
    }
}

#[derive(Debug, Clone)]
pub struct FunctionRef {
    pub(crate) id: DefinitionId,
    pub(crate) definitions: DefinitionTable,
}

impl FunctionRef {
    pub fn id(&self) -> DefinitionId {
        self.id
    }

    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }
}

pub(crate) fn same_definition(
    left: DefinitionId,
    left_scope: &DefinitionTable,
    right: DefinitionId,
    right_scope: &DefinitionTable,
) -> bool {
    match (left_scope.resolve(left), right_scope.resolve(right)) {
        (Ok(left), Ok(right)) => {
            left.module() == right.module() && left.segments().eq(right.segments())
        }
        _ => false,
    }
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}

fn adopt_reference<T: RemapDefinitions>(
    id: DefinitionId,
    definitions: &DefinitionTable,
    record: &T,
    context: &DefinitionContext,
) -> NativeResult<(DefinitionId, T, DefinitionTable)> {
    let cancel = CancellationToken::default();
    let mut ids = HashSet::from([id]);
    record
        .visit_definition_ids(
            &mut |id| {
                ids.insert(*id);
                Ok(())
            },
            &cancel,
        )
        .map_err(conflict)?;
    let remap = context.import(definitions, ids).map_err(conflict)?;
    let record = record
        .remap_definitions(&mut DefinitionMapper::new(
            &mut |id| remap.map(*id).map_err(Into::into),
            &cancel,
        ))
        .map_err(conflict)?;
    Ok((remap.map(id).map_err(conflict)?, record, context.snapshot()))
}
