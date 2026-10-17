//! Reference target and boundary-position rules shared by declaration consumers.
use crate::ty::Ty;

impl Ty {
    /// Reports references in the entire type expression, including callable signatures.
    pub fn contains_reference(&self) -> bool {
        self.contains_reference_in(true)
    }

    /// Callable signatures describe future inputs, not captured locations. Owning
    /// values cannot contain an actual view; closure captures are checked separately.
    pub fn contains_stored_reference(&self) -> bool {
        self.contains_reference_in(false)
    }

    fn contains_reference_in(&self, signatures: bool) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Reference { .. } => return true,
                Self::Struct(n) | Self::NativeObject(n) | Self::Enum(n) | Self::Trait(n) => {
                    pending.extend(&n.arguments);
                    pending.extend(n.associated_types.values());
                }
                Self::Projection {
                    receiver,
                    interface,
                    arguments,
                    ..
                } => {
                    pending.push(receiver);
                    pending.extend(&interface.arguments);
                    pending.extend(interface.associated_types.values());
                    pending.extend(arguments);
                }
                Self::Tuple(items) => pending.extend(items),
                Self::Function { params, result } => {
                    if signatures {
                        pending.extend(params);
                        pending.push(result);
                    }
                }
                Self::Array(item)
                | Self::Set(item, _)
                | Self::Iter(item)
                | Self::Range(item, _) => pending.push(item),
                Self::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Builtin(_) | Self::Host(_) | Self::Parameter { .. } | Self::SelfType(_) => {}
            }
        }
        false
    }

    /// Only a direct parameter/local view is admitted. Target binders are checked
    /// again after application; concrete targets must be registered nominal types.
    pub fn reference_position_valid(&self) -> bool {
        self.within_wire_limits() && self.reference_position_valid_inner()
    }

    fn reference_position_valid_inner(&self) -> bool {
        match self {
            Self::Reference { target, .. } => target.reference_target_valid(),
            Self::Function { params, result } => {
                params.iter().all(Self::reference_position_valid_inner)
                    && !result.contains_stored_reference()
                    && result.reference_position_valid_inner()
            }
            Self::Tuple(items) => items
                .iter()
                .all(|ty| !ty.contains_stored_reference() && ty.reference_position_valid_inner()),
            Self::Array(item) | Self::Set(item, _) | Self::Iter(item) | Self::Range(item, _) => {
                !item.contains_stored_reference() && item.reference_position_valid_inner()
            }
            Self::Map { key, value, .. } => [key, value]
                .iter()
                .all(|ty| !ty.contains_stored_reference() && ty.reference_position_valid_inner()),
            Self::Struct(n) | Self::NativeObject(n) | Self::Enum(n) | Self::Trait(n) => n
                .arguments
                .iter()
                .chain(n.associated_types.values())
                .all(|ty| !ty.contains_stored_reference() && ty.reference_position_valid_inner()),
            _ => !self.contains_reference(),
        }
    }

    pub fn reference_target_valid(&self) -> bool {
        matches!(
            self,
            Self::Struct(_) | Self::NativeObject(_) | Self::Parameter { .. } | Self::SelfType(_)
        ) && !self.contains_reference()
    }
}
