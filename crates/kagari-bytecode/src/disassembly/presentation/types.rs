//! Complete semantic type spelling without source-level name resolution.

use crate::disassembly::presentation::DisplayContext;
use kagari_types::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::fmt::Write;

impl DisplayContext<'_> {
    /// Displays one retained semantic type, including scoped generic information.
    ///
    /// Nominal categories and access modes remain explicit. Associated bindings
    /// are ordered by portable identity, never by process-local handle order.
    /// The input must have verified depth bounds and use this context's table;
    /// a foreign or missing definition handle panics instead of guessing a name.
    pub(crate) fn ty(&self, ty: &Ty) -> String {
        let mut output = String::new();
        self.push_ty(&mut output, ty);
        output
    }

    fn push_ty(&self, output: &mut String, ty: &Ty) {
        match ty {
            Ty::Projection {
                arguments,
                receiver,
                interface,
                member,
            } => {
                output.push('<');
                self.push_ty(output, receiver);
                output.push_str(" as ");
                self.push_nominal(output, interface);
                output.push_str(">::");
                output.push_str(&self.definition(*member));
                self.push_arguments(output, arguments);
            }
            Ty::Host(id) => {
                output.push_str("host ");
                output.push_str(&self.definition(*id));
            }
            Ty::SelfType(owner) => {
                write!(output, "Self<{}>", self.definition(*owner)).expect("writing to a String");
            }
            Ty::Parameter { owner, position } => {
                write!(output, "parameter<{}, {position}>", self.definition(*owner))
                    .expect("writing to a String");
            }
            Ty::Builtin(kind) => output.push_str(builtin(*kind)),
            Ty::Tuple(items) => {
                output.push('(');
                self.push_sequence(output, items);
                if items.len() == 1 {
                    output.push(',');
                }
                output.push(')');
            }
            Ty::Function { params, result } => {
                output.push_str("fn(");
                self.push_sequence(output, params);
                output.push_str(") -> ");
                self.push_ty(output, result);
            }
            Ty::Iter(item) => self.push_unary(output, "Iter", item),
            Ty::Range(item, kind) => self.push_unary(output, kind.name(), item),
            Ty::Array(item) => {
                output.push('[');
                self.push_ty(output, item);
                output.push(']');
            }

            Ty::Struct(nominal) => {
                output.push_str("struct ");
                self.push_nominal(output, nominal);
            }
            Ty::NativeObject(nominal) => {
                output.push_str("native_object ");
                self.push_nominal(output, nominal);
            }
            Ty::Enum(nominal) => {
                output.push_str("enum ");
                self.push_nominal(output, nominal);
            }
            Ty::Trait(nominal) => {
                output.push_str("trait ");
                self.push_nominal(output, nominal);
            }
            Ty::Reference { target, access } => {
                self.push_unary(output, access.name(), target);
            }
        }
    }

    fn push_sequence(&self, output: &mut String, items: &[Ty]) {
        for (index, item) in items.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            self.push_ty(output, item);
        }
    }

    fn push_arguments(&self, output: &mut String, arguments: &[Ty]) {
        if !arguments.is_empty() {
            output.push('<');
            self.push_sequence(output, arguments);
            output.push('>');
        }
    }

    fn push_unary(&self, output: &mut String, name: &str, item: &Ty) {
        output.push_str(name);
        output.push('<');
        self.push_ty(output, item);
        output.push('>');
    }

    fn push_nominal(&self, output: &mut String, nominal: &NominalTy) {
        let NominalTy {
            declaration,
            arguments,
            associated_types,
        } = nominal;
        output.push_str(&self.definition(*declaration));
        if arguments.is_empty() && associated_types.is_empty() {
            return;
        }
        output.push('<');
        self.push_sequence(output, arguments);
        // BTreeMap's handle order follows import allocation, not portable identity.
        let mut bindings: Vec<_> = associated_types.iter().collect();
        bindings.sort_by(|(left, _), (right, _)| {
            self.definitions
                .compare(**left, **right)
                .expect("disassembly type belongs to the retained verified scope")
        });
        for (index, (member, ty)) in bindings.into_iter().enumerate() {
            if !arguments.is_empty() || index != 0 {
                output.push_str(", ");
            }
            output.push_str(&self.definition(*member));
            output.push_str(" = ");
            self.push_ty(output, ty);
        }
        output.push('>');
    }
}

fn builtin(ty: BuiltinType) -> &'static str {
    match ty {
        BuiltinType::Never => "!",
        BuiltinType::Unit => "()",
        BuiltinType::Bool => "bool",
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
        BuiltinType::String => "String",
        BuiltinType::Char => "char",
    }
}
