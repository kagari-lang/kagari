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
    pub associated_types: std::collections::BTreeMap<DefinitionId, TypeId>,
}

pub fn associated_type_id(owner: &DefinitionId, name: &str) -> DefinitionId {
    let mut id = owner.clone();
    id.path
        .push(kagari_common::identity::DefinitionPathSegment {
            kind: kagari_common::identity::DefinitionKind::AssociatedType,
            name: name.to_owned(),
            occurrence: 0,
        });
    id
}

impl NominalType {
    pub fn satisfies(&self, required: &Self) -> bool {
        self.declaration == required.declaration
            && self.arguments == required.arguments
            && required
                .associated_types
                .iter()
                .all(|(member, ty)| self.associated_types.get(member) == Some(ty))
    }
    pub fn instantiate(&self, substitution: &TypeSubstitution) -> Self {
        Self {
            declaration: self.declaration.clone(),
            associated_types: self
                .associated_types
                .iter()
                .map(|(id, ty)| (id.clone(), ty.instantiate(substitution)))
                .collect(),
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
            associated_types: self
                .associated_types
                .iter()
                .map(|(id, ty)| (id.clone(), map(ty)))
                .collect(),
            arguments: self.arguments.iter().map(&mut map).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeId {
    Unknown,
    Error,
    Builtin(BuiltinType),
    Tuple(Vec<TypeId>),
    Function {
        params: Vec<TypeId>,
        result: Box<TypeId>,
    },
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
    Projection {
        receiver: Box<TypeId>,
        interface: Box<NominalType>,
        member: DefinitionId,
    },
    SelfType(DefinitionId),
    StandardEnum {
        kind: crate::builtin::surface::StandardEnum,
        args: Vec<TypeId>,
    },
}

impl TypeId {
    pub fn contains_projection(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Projection { .. } => return true,
                Self::Struct(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    pending.extend(&ty.arguments);
                    pending.extend(ty.associated_types.values());
                }
                Self::Tuple(types) | Self::StandardEnum { args: types, .. } => {
                    pending.extend(types)
                }
                Self::Array(ty) | Self::Set(ty) => pending.push(ty),
                Self::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                _ => {}
            }
        }
        false
    }
    pub fn with_associated_types(&self, interface: &NominalType) -> Self {
        crate::typeck::associated::normalize(self, &|projected, _, member| {
            (projected.declaration == interface.declaration)
                .then(|| interface.associated_types.get(member).cloned())
                .flatten()
        })
    }
    pub fn contains_host_value(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Host(_) => return true,
                Self::Tuple(items) | Self::StandardEnum { args: items, .. } => {
                    pending.extend(items)
                }
                Self::Function { .. } => {}
                Self::Array(item) | Self::Set(item) => pending.push(item),
                Self::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Struct(nominal) | Self::Enum(nominal) | Self::Trait(nominal) => {
                    pending.extend(&nominal.arguments);
                    pending.extend(nominal.associated_types.values())
                }
                _ => {}
            }
        }
        false
    }

    pub fn contains_self_type(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Projection {
                    receiver,
                    interface,
                    ..
                } => {
                    pending.push(receiver);
                    pending.extend(&interface.arguments);
                    pending.extend(interface.associated_types.values());
                }
                Self::SelfType(_) => return true,
                Self::Tuple(items) | Self::StandardEnum { args: items, .. } => {
                    pending.extend(items)
                }
                Self::Array(item) | Self::Set(item) => pending.push(item),
                Self::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Self::Struct(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    pending.extend(&ty.arguments);
                    pending.extend(ty.associated_types.values())
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

    /// Visit direct type children. Projection normalization uses a separately
    /// bounded walk; ordinary substitution remains iterative.
    pub fn map_children(&self, mut map: impl FnMut(&TypeId) -> TypeId) -> Self {
        match self {
            Self::Tuple(items) => Self::Tuple(items.iter().map(map).collect()),
            Self::Array(ty) => Self::Array(Box::new(map(ty))),
            Self::Set(ty) => Self::Set(Box::new(map(ty))),
            Self::Map { key, value } => Self::Map {
                key: Box::new(map(key)),
                value: Box::new(map(value)),
            },
            Self::Function { params, result } => Self::Function {
                params: params.iter().map(&mut map).collect(),
                result: Box::new(map(result)),
            },
            Self::Struct(ty) => Self::Struct(ty.map_arguments(map)),
            Self::Enum(ty) => Self::Enum(ty.map_arguments(map)),
            Self::Trait(ty) => Self::Trait(ty.map_arguments(map)),
            Self::StandardEnum { kind, args } => Self::StandardEnum {
                kind: *kind,
                args: args.iter().map(map).collect(),
            },
            Self::Projection {
                receiver,
                interface,
                member,
            } => Self::Projection {
                receiver: Box::new(map(receiver)),
                interface: Box::new(interface.map_arguments(map)),
                member: member.clone(),
            },
            _ => self.clone(),
        }
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
                Self::Function { params, .. } => Self::Function {
                    params: vec![Self::Unknown; params.len()],
                    result: Box::new(Self::Unknown),
                },
                Self::Projection {
                    interface, member, ..
                } => Self::Projection {
                    receiver: Box::new(Self::Unknown),
                    interface: Box::new(interface.map_arguments(|_| Self::Unknown)),
                    member: member.clone(),
                },
                _ => source.clone(),
            };
            match (source, target) {
                (Self::Struct(source), Self::Struct(target))
                | (Self::Enum(source), Self::Enum(target))
                | (Self::Trait(source), Self::Trait(target)) => {
                    pending.extend(
                        source
                            .associated_types
                            .values()
                            .zip(target.associated_types.values_mut())
                            .map(|(source, target)| (source, target, substitute)),
                    );
                    pending.extend(
                        source
                            .arguments
                            .iter()
                            .zip(&mut target.arguments)
                            .rev()
                            .map(|(source, target)| (source, target, substitute)),
                    );
                }
                (
                    Self::Projection {
                        receiver: sr,
                        interface: si,
                        ..
                    },
                    Self::Projection {
                        receiver: tr,
                        interface: ti,
                        ..
                    },
                ) => {
                    pending.push((sr, tr, substitute));
                    pending.extend(
                        si.arguments
                            .iter()
                            .zip(&mut ti.arguments)
                            .map(|(source, target)| (source, target, substitute)),
                    );
                    pending.extend(
                        si.associated_types
                            .values()
                            .zip(ti.associated_types.values_mut())
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
                (
                    Self::Function {
                        params: source_params,
                        result: source_result,
                    },
                    Self::Function {
                        params: target_params,
                        result: target_result,
                    },
                ) => {
                    pending.push((source_result, target_result, substitute));
                    pending.extend(
                        source_params
                            .iter()
                            .zip(target_params)
                            .rev()
                            .map(|(source, target)| (source, target, substitute)),
                    );
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
                    pending.extend(nominal.associated_types.values());
                }
                Self::Tuple(items) | Self::StandardEnum { args: items, .. } => {
                    pending.extend(items);
                }
                Self::Array(item) | Self::Set(item) => pending.push(item),
                Self::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Self::Generic(parameter) if parameters.contains(parameter) => {}
                Self::Projection { receiver, .. }
                    if receiver.is_resolved_in(parameters) && !parameters.is_empty() => {}
                Self::Projection { .. }
                | Self::Generic(_)
                | Self::Unknown
                | Self::Error
                | Self::SelfType(_) => return false,
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
                | Self::SelfType(_)
                | Self::Projection { .. } => return false,
                Self::Function { .. } => return false,
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
                    pending.extend(nominal.associated_types.values());
                }
                Self::Tuple(items) | Self::StandardEnum { args: items, .. } => {
                    pending.extend(items);
                }
                Self::Array(item) | Self::Set(item) => pending.push(item),
                Self::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Self::Projection {
                    receiver,
                    interface,
                    ..
                } => {
                    pending.push(receiver);
                    pending.extend(&interface.arguments);
                    pending.extend(interface.associated_types.values());
                }
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
                    pending.extend(&ty.arguments);
                    pending.extend(ty.associated_types.values())
                }
                Self::Array(element) | Self::Set(element) => pending.push(element),
                Self::Map { key, value } => {
                    pending.push(key);
                    pending.push(value);
                }
                Self::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
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
                (
                    Self::Function {
                        params: left_params,
                        result: left_result,
                    },
                    Self::Function {
                        params: right_params,
                        result: right_result,
                    },
                ) if left_params.len() == right_params.len() => {
                    pending.push((left_result, right_result));
                    pending.extend(left_params.iter_mut().zip(right_params).rev());
                }
                (Self::Struct(left), Self::Struct(right))
                | (Self::Enum(left), Self::Enum(right))
                | (Self::Trait(left), Self::Trait(right))
                    if left.declaration == right.declaration
                        && left.arguments.len() == right.arguments.len() =>
                {
                    if left
                        .associated_types
                        .keys()
                        .eq(right.associated_types.keys())
                    {
                        pending.extend(
                            left.associated_types
                                .values_mut()
                                .zip(right.associated_types.values()),
                        );
                    }
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
                (
                    Self::Function {
                        params: left_params,
                        result: left_result,
                    },
                    Self::Function {
                        params: right_params,
                        result: right_result,
                    },
                ) => {
                    if left_params.len() != right_params.len() {
                        return true;
                    }
                    pending.push((left_result, right_result));
                    pending.extend(left_params.iter().zip(right_params).rev());
                }
                (Self::Struct(left), Self::Struct(right))
                | (Self::Enum(left), Self::Enum(right))
                | (Self::Trait(left), Self::Trait(right)) => {
                    if left.declaration != right.declaration
                        || left.arguments.len() != right.arguments.len()
                        || !left
                            .associated_types
                            .keys()
                            .eq(right.associated_types.keys())
                    {
                        return true;
                    }
                    pending.extend(
                        left.associated_types
                            .values()
                            .zip(right.associated_types.values()),
                    );
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
        enum Part<'a> {
            Type(&'a TypeId),
            Text(&'a str),
        }
        fn sequence<'a>(
            pending: &mut Vec<Part<'a>>,
            args: &'a [TypeId],
            open: &'a str,
            close: &'a str,
        ) {
            pending.push(Part::Text(close));
            for (index, ty) in args.iter().enumerate().rev() {
                pending.push(Part::Type(ty));
                if index > 0 {
                    pending.push(Part::Text(", "));
                }
            }
            pending.push(Part::Text(open));
        }

        let mut output = String::new();
        let mut pending = vec![Part::Type(self)];
        while let Some(part) = pending.pop() {
            match part {
                Part::Text(text) => output.push_str(text),
                Part::Type(ty) => match ty {
                    Self::Unknown => output.push_str("<unknown>"),
                    Self::Error => output.push_str("<error>"),
                    Self::Host(id) => {
                        output.push_str(&id.path.last().expect("host type identity").name)
                    }
                    Self::Builtin(ty) => output.push_str(
                        crate::builtin::surface::builtin_type_spec(*ty)
                            .map_or("<builtin>", |spec| spec.name),
                    ),
                    Self::Tuple(items) => sequence(&mut pending, items, "(", ")"),
                    Self::Function { params, result } => {
                        pending.push(Part::Type(result));
                        pending.push(Part::Text(" -> "));
                        sequence(&mut pending, params, "fn(", ")");
                    }
                    Self::Array(item) => {
                        pending.push(Part::Text("]"));
                        pending.push(Part::Type(item));
                        pending.push(Part::Text("["));
                    }
                    Self::Map { key, value } => {
                        pending.push(Part::Text(">"));
                        pending.push(Part::Type(value));
                        pending.push(Part::Text(", "));
                        pending.push(Part::Type(key));
                        pending.push(Part::Text("Map<"));
                    }
                    Self::Set(item) => {
                        pending.push(Part::Text(">"));
                        pending.push(Part::Type(item));
                        pending.push(Part::Text("Set<"));
                    }
                    Self::Struct(nominal) | Self::Enum(nominal) | Self::Trait(nominal) => {
                        if !nominal.arguments.is_empty() || !nominal.associated_types.is_empty() {
                            pending.push(Part::Text(">"));
                            for (index, (member, ty)) in
                                nominal.associated_types.iter().enumerate().rev()
                            {
                                pending.push(Part::Type(ty));
                                pending.push(Part::Text(" = "));
                                pending.push(Part::Text(
                                    &member.path.last().expect("associated member").name,
                                ));
                                if index > 0 || !nominal.arguments.is_empty() {
                                    pending.push(Part::Text(", "));
                                }
                            }
                            for (index, ty) in nominal.arguments.iter().enumerate().rev() {
                                pending.push(Part::Type(ty));
                                if index > 0 {
                                    pending.push(Part::Text(", "));
                                }
                            }
                            pending.push(Part::Text("<"));
                        }
                        pending.push(Part::Text(
                            &nominal
                                .declaration
                                .path
                                .last()
                                .expect("type declaration path")
                                .name,
                        ));
                    }
                    Self::Generic(parameter) => output.push_str(&parameter.name),
                    Self::SelfType(_) => output.push_str("Self"),
                    Self::Projection {
                        receiver,
                        interface,
                        member,
                    } => {
                        output.push_str(&format!(
                            "<{} as {}>::{}",
                            receiver.display_name(),
                            interface
                                .declaration
                                .path
                                .last()
                                .map_or("", |p| p.name.as_str()),
                            member.path.last().map_or("", |p| p.name.as_str())
                        ));
                    }
                    Self::StandardEnum { kind, args } => {
                        sequence(&mut pending, args, "<", ">");
                        pending.push(Part::Text(kind.spec().name));
                    }
                },
            }
        }
        output
    }

    pub fn is_heap_backed(&self) -> bool {
        match self {
            Self::Unknown | Self::Error => false,
            Self::Builtin(ty) => {
                crate::builtin::surface::builtin_type_spec(*ty).is_some_and(|spec| spec.heap_backed)
            }
            Self::Tuple(_)
            | Self::Function { .. }
            | Self::Array(_)
            | Self::Map { .. }
            | Self::Set(_)
            | Self::Struct(_)
            | Self::Enum(_)
            | Self::Trait(_)
            | Self::Host(_)
            | Self::Generic(_)
            | Self::SelfType(_)
            | Self::Projection { .. }
            | Self::StandardEnum { .. } => true,
        }
    }
}
