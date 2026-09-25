use kagari_common::identity::DefinitionId;

pub type TypeSubstitution = std::collections::HashMap<GenericParameterType, TypeId>;

#[cfg(test)]
mod nominal_tests;

/// Names are diagnostic metadata; owner and position determine equality.
#[derive(Debug, Clone)]
pub struct GenericParameterType {
    pub owner: DefinitionId,
    pub position: usize,
    pub name: String,
}

impl PartialEq for GenericParameterType {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner && self.position == other.position
    }
}
impl Eq for GenericParameterType {}
impl std::hash::Hash for GenericParameterType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.owner.hash(state);
        self.position.hash(state);
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum BuiltinType {
    Unit,
    Bool,
    I8,
    I16,
    I32,
    I64,
    ISize,
    U8,
    U16,
    U32,
    U64,
    USize,
    F32,
    F64,
    String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NominalType {
    pub declaration: DefinitionId,
    pub arguments: Vec<TypeId>,
}

impl NominalType {
    pub fn instantiate(&self, substitution: &TypeSubstitution) -> Self {
        Self {
            declaration: self.declaration.clone(),
            arguments: self
                .arguments
                .iter()
                .map(|argument| argument.instantiate(substitution))
                .collect(),
        }
    }

    fn map_arguments(&self, mut map: impl FnMut(&TypeId) -> TypeId) -> Self {
        Self {
            declaration: self.declaration.clone(),
            arguments: self.arguments.iter().map(&mut map).collect(),
        }
    }

    fn display_name(&self) -> String {
        let name = &self
            .declaration
            .path
            .last()
            .expect("type declaration path")
            .name;
        if self.arguments.is_empty() {
            name.clone()
        } else {
            format!(
                "{name}<{}>",
                self.arguments
                    .iter()
                    .map(TypeId::display_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeId {
    Unknown,
    Error,
    Builtin(BuiltinType),
    Tuple(Vec<TypeId>),
    Array(Box<TypeId>),
    Map {
        key: Box<TypeId>,
        value: Box<TypeId>,
    },
    Set(Box<TypeId>),
    Struct(NominalType),
    Enum(NominalType),
    Trait(NominalType),
    Host(DefinitionId),
    Generic(GenericParameterType),
    SelfType(DefinitionId),
    StandardEnum {
        kind: crate::builtin::surface::StandardEnum,
        args: Vec<TypeId>,
    },
}

impl TypeId {
    pub fn contains_self_type(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::SelfType(_) => return true,
                Self::Tuple(items) | Self::StandardEnum { args: items, .. } => {
                    pending.extend(items)
                }
                Self::Array(item) | Self::Set(item) => pending.push(item),
                Self::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Struct(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    pending.extend(&ty.arguments)
                }
                Self::Unknown
                | Self::Error
                | Self::Builtin(_)
                | Self::Host(_)
                | Self::Generic(_) => {}
            }
        }
        false
    }

    /// Substitute one binder layer; replacements can contain the caller's parameters.
    pub fn instantiate(&self, substitution: &TypeSubstitution) -> TypeId {
        self.substitute_once(|ty| match ty {
            Self::Generic(parameter) => substitution.get(parameter),
            _ => None,
        })
    }

    /// Preserve known argument context without exposing uninferred callee binders.
    pub(crate) fn argument_context(
        &self,
        substitution: &TypeSubstitution,
        parameters: &[GenericParameterType],
    ) -> Self {
        self.substitute_once(|ty| match ty {
            Self::Generic(parameter) => substitution
                .get(parameter)
                .or_else(|| parameters.contains(parameter).then_some(&Self::Unknown)),
            _ => None,
        })
    }

    /// Rebuild one binding layer, copying inserted types without revisiting them
    /// as substitution targets. Both generic binders and trait Self use this walk.
    fn substitute_once<'a>(&'a self, replacement: impl Fn(&Self) -> Option<&'a Self>) -> Self {
        let mut result = Self::Unknown;
        let mut pending = vec![(self, &mut result, true)];
        while let Some((source, target, substitute)) = pending.pop() {
            if substitute && let Some(inserted) = replacement(source) {
                pending.push((inserted, target, false));
                continue;
            }
            *target = match source {
                Self::Struct(ty) => Self::Struct(ty.map_arguments(|_| Self::Unknown)),
                Self::Enum(ty) => Self::Enum(ty.map_arguments(|_| Self::Unknown)),
                Self::Trait(ty) => Self::Trait(ty.map_arguments(|_| Self::Unknown)),
                Self::Tuple(items) => Self::Tuple(vec![Self::Unknown; items.len()]),
                Self::StandardEnum { kind, args } => Self::StandardEnum {
                    kind: *kind,
                    args: vec![Self::Unknown; args.len()],
                },
                Self::Array(_) => Self::Array(Box::new(Self::Unknown)),
                Self::Set(_) => Self::Set(Box::new(Self::Unknown)),
                Self::Map { .. } => Self::Map {
                    key: Box::new(Self::Unknown),
                    value: Box::new(Self::Unknown),
                },
                _ => source.clone(),
            };
            match (source, target) {
                (Self::Struct(source), Self::Struct(target))
                | (Self::Enum(source), Self::Enum(target))
                | (Self::Trait(source), Self::Trait(target)) => {
                    pending.extend(
                        source
                            .arguments
                            .iter()
                            .zip(&mut target.arguments)
                            .rev()
                            .map(|(source, target)| (source, target, substitute)),
                    );
                }
                (Self::Tuple(source), Self::Tuple(target))
                | (
                    Self::StandardEnum { args: source, .. },
                    Self::StandardEnum { args: target, .. },
                ) => {
                    pending.extend(
                        source
                            .iter()
                            .zip(target)
                            .rev()
                            .map(|(source, target)| (source, target, substitute)),
                    );
                }
                (Self::Array(source), Self::Array(target))
                | (Self::Set(source), Self::Set(target)) => {
                    pending.push((source, target, substitute));
                }
                (
                    Self::Map {
                        key: source_key,
                        value: source_value,
                    },
                    Self::Map {
                        key: target_key,
                        value: target_value,
                    },
                ) => {
                    pending.push((source_value, target_value, substitute));
                    pending.push((source_key, target_key, substitute));
                }
                _ => {}
            }
        }
        result
    }

    pub fn is_concrete(&self) -> bool {
        self.is_resolved_in(&[])
    }

    /// A caller-owned binder is known context even before monomorphization.
    pub(crate) fn is_resolved_in(&self, parameters: &[GenericParameterType]) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Struct(nominal) | Self::Enum(nominal) | Self::Trait(nominal) => {
                    pending.extend(&nominal.arguments);
                }
                Self::Tuple(items) | Self::StandardEnum { args: items, .. } => {
                    pending.extend(items);
                }
                Self::Array(item) | Self::Set(item) => pending.push(item),
                Self::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Generic(parameter) if parameters.contains(parameter) => {}
                Self::Generic(_) | Self::Unknown | Self::Error | Self::SelfType(_) => return false,
                Self::Builtin(_) | Self::Host(_) => {}
            }
        }
        true
    }
    pub fn with_self(&self, owner: &DefinitionId, replacement: &TypeId) -> TypeId {
        self.substitute_once(|ty| match ty {
            Self::SelfType(id) if id == owner => Some(replacement),
            _ => None,
        })
    }
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Self::Builtin(
                BuiltinType::I8
                    | BuiltinType::I16
                    | BuiltinType::I32
                    | BuiltinType::I64
                    | BuiltinType::ISize
                    | BuiltinType::U8
                    | BuiltinType::U16
                    | BuiltinType::U32
                    | BuiltinType::U64
                    | BuiltinType::USize
            )
        )
    }
    pub fn supports_equality(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Tuple(members) | Self::StandardEnum { args: members, .. } => {
                    pending.extend(members);
                }
                Self::Unknown
                | Self::Error
                | Self::Trait(_)
                | Self::Host(_)
                | Self::Generic(_)
                | Self::SelfType(_) => return false,
                // The elements of mutable containers do not participate in identity equality.
                Self::Builtin(_)
                | Self::Struct(_)
                | Self::Enum(_)
                | Self::Array(_)
                | Self::Map { .. }
                | Self::Set(_) => {}
            }
        }
        true
    }

    /// Recovery types suppress dependent diagnostics but never authorize codegen.
    pub fn is_unresolved(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Struct(nominal) | Self::Enum(nominal) | Self::Trait(nominal) => {
                    pending.extend(&nominal.arguments);
                }
                Self::Tuple(items) | Self::StandardEnum { args: items, .. } => {
                    pending.extend(items);
                }
                Self::Array(item) | Self::Set(item) => pending.push(item),
                Self::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Unknown | Self::Error => return true,
                Self::Builtin(_) | Self::Host(_) | Self::Generic(_) | Self::SelfType(_) => {}
            }
        }
        false
    }

    /// Seal failed inference without discarding independently known members.
    pub(crate) fn diagnose_unknowns(&self) -> Self {
        self.substitute_once(|ty| matches!(ty, Self::Unknown).then_some(&Self::Error))
    }

    /// Unknown inference holes still need a diagnostic; Error already has one.
    pub(crate) fn contains_unknown(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Unknown => return true,
                Self::Tuple(items) | Self::StandardEnum { args: items, .. } => {
                    pending.extend(items)
                }
                Self::Struct(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    pending.extend(&ty.arguments)
                }
                Self::Array(element) | Self::Set(element) => pending.push(element),
                Self::Map { key, value } => {
                    pending.push(key);
                    pending.push(value);
                }
                _ => {}
            }
        }
        false
    }

    /// Fill recovery holes from another checked expression, preserving known facts.
    pub(crate) fn recover_from(&mut self, other: &Self) {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            if matches!(right, Self::Unknown | Self::Error) {
                continue;
            }
            match (left, right) {
                (left @ (Self::Unknown | Self::Error), right) => *left = right.clone(),
                (Self::Tuple(left), Self::Tuple(right)) if left.len() == right.len() => {
                    pending.extend(left.iter_mut().zip(right).rev());
                }
                (Self::Array(left), Self::Array(right)) | (Self::Set(left), Self::Set(right)) => {
                    pending.push((left, right));
                }
                (Self::Map { key: lk, value: lv }, Self::Map { key: rk, value: rv }) => {
                    pending.push((lv, rv));
                    pending.push((lk, rk));
                }
                (Self::Struct(left), Self::Struct(right))
                | (Self::Enum(left), Self::Enum(right))
                | (Self::Trait(left), Self::Trait(right))
                    if left.declaration == right.declaration
                        && left.arguments.len() == right.arguments.len() =>
                {
                    pending.extend(left.arguments.iter_mut().zip(&right.arguments).rev());
                }
                (
                    Self::StandardEnum {
                        kind: lk,
                        args: left,
                    },
                    Self::StandardEnum {
                        kind: rk,
                        args: right,
                    },
                ) if lk == rk && left.len() == right.len() => {
                    pending.extend(left.iter_mut().zip(right).rev());
                }
                _ => {}
            }
        }
    }

    pub fn conflicts_with(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (Self::Unknown | Self::Error, _) | (_, Self::Unknown | Self::Error) => {}
                (Self::Tuple(left), Self::Tuple(right)) => {
                    if left.len() != right.len() {
                        return true;
                    }
                    pending.extend(left.iter().zip(right).rev());
                }
                (Self::Array(left), Self::Array(right)) | (Self::Set(left), Self::Set(right)) => {
                    pending.push((left, right));
                }
                (Self::Map { key: lk, value: lv }, Self::Map { key: rk, value: rv }) => {
                    pending.push((lv, rv));
                    pending.push((lk, rk));
                }
                (Self::Struct(left), Self::Struct(right))
                | (Self::Enum(left), Self::Enum(right))
                | (Self::Trait(left), Self::Trait(right)) => {
                    if left.declaration != right.declaration
                        || left.arguments.len() != right.arguments.len()
                    {
                        return true;
                    }
                    pending.extend(left.arguments.iter().zip(&right.arguments).rev());
                }
                (
                    Self::StandardEnum { kind: lk, args: la },
                    Self::StandardEnum { kind: rk, args: ra },
                ) => {
                    if lk != rk || la.len() != ra.len() {
                        return true;
                    }
                    pending.extend(la.iter().zip(ra).rev());
                }
                _ if left != right => return true,
                _ => {}
            }
        }
        false
    }

    pub fn from_name(name: &str) -> Option<Self> {
        crate::builtin::surface::builtin_type(name).map(Self::Builtin)
    }

    pub fn display_name(&self) -> String {
        match self {
            Self::Unknown => "<unknown>".to_owned(),
            Self::Error => "<error>".to_owned(),
            Self::Host(id) => id.path.last().expect("host type identity").name.clone(),
            Self::Builtin(ty) => crate::builtin::surface::builtin_type_spec(*ty)
                .map(|spec| spec.name.to_owned())
                .unwrap_or("<builtin>".to_owned()),
            Self::Tuple(elements) => {
                let inner = elements
                    .iter()
                    .map(TypeId::display_name)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({inner})")
            }
            Self::Array(element) => format!("[{}]", element.display_name()),
            Self::Map { key, value } => {
                format!("Map<{}, {}>", key.display_name(), value.display_name())
            }
            Self::Set(element) => format!("Set<{}>", element.display_name()),
            Self::Struct(ty) | Self::Enum(ty) | Self::Trait(ty) => ty.display_name(),
            Self::Generic(parameter) => parameter.name.clone(),
            Self::SelfType(_) => "Self".to_owned(),
            Self::StandardEnum { kind, args } => {
                let inner = args
                    .iter()
                    .map(TypeId::display_name)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}<{inner}>", kind.spec().name)
            }
        }
    }

    pub fn is_heap_backed(&self) -> bool {
        match self {
            Self::Unknown | Self::Error => false,
            Self::Builtin(ty) => {
                crate::builtin::surface::builtin_type_spec(*ty).is_some_and(|spec| spec.heap_backed)
            }
            Self::Tuple(_)
            | Self::Array(_)
            | Self::Map { .. }
            | Self::Set(_)
            | Self::Struct(_)
            | Self::Enum(_)
            | Self::Trait(_)
            | Self::Host(_)
            | Self::Generic(_)
            | Self::SelfType(_)
            | Self::StandardEnum { .. } => true,
        }
    }
}
