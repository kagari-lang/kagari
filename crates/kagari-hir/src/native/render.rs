//! Complete native declaration views derived from authoritative registrations.
use crate::native::paths::module_path;
use kagari_common::{
    identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment, associated_type_id},
    span::Span,
};
use kagari_types::{
    callable::{CallableImplementation, NativeDefaultApplication},
    collection::CollectionAccess,
    declaration::{
        FnDecl, TypeDefKind,
        module::{DeclarationError, ModuleDecl},
        native::NativeTypeConstructor,
    },
    language::{Protocol, role::LangRole},
    scalar::BuiltinType,
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty},
};
use std::{collections::BTreeMap, ops::Deref, sync::Arc};

/// Byte sites recorded while rendering one canonical declaration into generated Kagari text.
#[derive(Debug, Clone)]
pub struct NativeDeclarationSite {
    /// Full declaration byte range in the generated text.
    pub span: Span,
    /// Declaration-name byte range.
    pub name_span: Span,
    /// Generic binder sites in declaration order.
    pub generics: Vec<Span>,
    /// Parameter sites in declaration order.
    pub parameters: Vec<Span>,
    /// Where/bound sites with their target and constraint ranges.
    pub bounds: Vec<NativeBoundSite>,
}

/// Generated source locations for a bound target and its requirements.
#[derive(Debug, Clone)]
pub struct NativeBoundSite {
    /// Bound-subject byte range.
    pub target: Span,
    /// Required trait/type constraint ranges in rendered order.
    pub constraints: Vec<Span>,
}

/// Generated declaration text and identity-to-site metadata from authoritative registration.
///
/// ```text
/// registered ModuleDecl + providers -> declaration_source
///   -> DeclarationSource { uri, text, sites }
///   -> native::api parse_declarations + correspondence validation
///   -> ordinary LoweredModule + attached installed metadata
/// ```
///
/// The generated view is presentation/analysis input, not an independently authored
/// source of native authority. All sites refer to this exact text, in bytes.
#[derive(Debug, Clone)]
pub struct DeclarationSource {
    /// Source URI assigned to the generated module view.
    pub uri: String,
    /// Complete rendered Kagari declarations and documentation.
    pub text: String,
    /// Canonical definition paths to their generated source locations.
    pub sites: BTreeMap<DefinitionPath, NativeDeclarationSite>,
}

/// Renders complete source declarations using the module and its explicit provider set.
///
/// # Errors
///
/// Returns a declaration error if registered data cannot be rendered consistently
/// with the supported source declaration model.
pub fn declaration_source(
    module: &ModuleDecl,
    providers: &[Arc<ModuleDecl>],
) -> Result<DeclarationSource, DeclarationError> {
    DeclarationView { module, providers }.render()
}

/// Borrowed authoritative module plus provider context used to resolve rendered identities.
struct DeclarationView<'a> {
    module: &'a ModuleDecl,
    providers: &'a [Arc<ModuleDecl>],
}

impl Deref for DeclarationView<'_> {
    type Target = ModuleDecl;
    fn deref(&self) -> &ModuleDecl {
        self.module
    }
}

impl DeclarationView<'_> {
    fn render(&self) -> Result<DeclarationSource, DeclarationError> {
        let mut output = Renderer {
            module: self,
            text: "// Generated from native registration definitions. Do not edit.\n\n".into(),
            sites: BTreeMap::new(),
        };
        for line in self.module_documentation.lines() {
            output.text.push_str("//! ");
            output.text.push_str(line);
            output.text.push('\n');
        }
        output.text.push('\n');
        for (name, target) in &self.exports {
            let path = target
                .path
                .iter()
                .map(|part| part.name.as_str())
                .collect::<Vec<_>>()
                .join("::");
            output.text.push_str(&format!(
                "pub use {}::{} as {};\n",
                module_path(&target.module, self.providers),
                path,
                name
            ));
        }
        for ty in &self.types {
            let id = self.definition(ty.kind.definition_kind(), &ty.name);
            let is_enum = ty.kind == TypeDefKind::Enum;
            output.doc(&id);
            let start = output.text.len();
            output
                .text
                .push_str(if is_enum { "pub enum " } else { "pub type " });
            let name_span = output.name(&ty.name);
            let generics = output.generics(&ty.generic_params);
            let bounds = output.bounds(&ty.bounds)?;
            if is_enum {
                output.text.push_str(" {\n");
                for variant in &ty.variants {
                    let mut owner = id.clone();
                    owner.path.push(DefinitionPathSegment {
                        kind: DefinitionKind::Variant,
                        name: variant.name.clone(),
                        occurrence: 0,
                    });
                    output.doc(&owner);
                    let start = output.text.len();
                    output.text.push_str("    ");
                    let name_span = output.name(&variant.name);
                    let mut payload = vec![];
                    if !variant.payload.is_empty() {
                        output.text.push('(');
                        for (index, ty) in variant.payload.iter().enumerate() {
                            if index > 0 {
                                output.text.push_str(", ");
                            }
                            payload.push(output.name(&self.type_spelling(ty)?));
                        }
                        output.text.push(')');
                    }
                    output.text.push_str(",\n");
                    output.site(owner, start, name_span, vec![], payload);
                }
                output.text.push_str("}\n\n");
            } else {
                output.text.push_str(";\n\n");
            }
            output.site(id.clone(), start, name_span, generics, vec![]);
            output
                .sites
                .get_mut(&id)
                .expect("type declaration site")
                .bounds = bounds;
            if self.variant_exports.contains(&ty.name) {
                output.text.push_str(&format!(
                    "pub use self::{}::{{{}}};\n\n",
                    ty.name,
                    ty.variants
                        .iter()
                        .map(|variant| variant.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        for item in &self.traits {
            let id = self.definition(DefinitionKind::Trait, &item.name);
            output.doc(&id);
            let start = output.text.len();
            if let Some(role) = Protocol::from_id(&id).and_then(LangRole::from_protocol) {
                output
                    .text
                    .push_str(&format!("#[lang = \"{}\"]\n", role.name()));
            }
            output.text.push_str("pub trait ");
            let name_span = output.name(&item.name);
            let generics = output.generics(&item.generic_params);
            if !item.supertraits.is_empty() {
                output.text.push_str(": ");
                let names = item
                    .supertraits
                    .iter()
                    .map(|ty| self.nominal_spelling(ty))
                    .collect::<Result<Vec<_>, _>>()?;
                output.text.push_str(&names.join(" + "));
            }
            output.text.push_str(" {\n");
            for member in &item.associated_types {
                let name = &member
                    .declaration
                    .path
                    .last()
                    .ok_or_else(|| DeclarationError("missing associated name".into()))?
                    .name;
                output.doc(&member.declaration);
                let start = output.text.len();
                output.text.push_str("    type ");
                let name_span = output.name(name);
                let constraints = output.constraints(&member.bounds)?;
                output.text.push_str(";\n");
                output.site(member.declaration.clone(), start, name_span, vec![], vec![]);
                output
                    .sites
                    .get_mut(&member.declaration)
                    .expect("associated declaration")
                    .bounds = vec![NativeBoundSite {
                    target: name_span,
                    constraints,
                }];
            }
            for method in &item.methods {
                output.function(ModuleDecl::method_id(&id, &method.name), method, true)?;
            }
            output.text.push_str("}\n\n");
            output.site(id, start, name_span, generics, vec![]);
        }
        for (index, implementation) in self.implementations.iter().enumerate() {
            let id = self.implementation_id(index);
            let start = output.text.len();
            output.text.push_str("impl");
            let generics = output.generics(&implementation.generic_params);
            output.text.push(' ');
            if let Some(trait_type) = &implementation.trait_type {
                let mut header = trait_type.clone();
                header.associated_types.clear();
                output.text.push_str(&self.nominal_spelling(&header)?);
                output.text.push_str(" for ");
            }
            let name_span = output.name(&self.type_spelling(&implementation.for_type)?);
            let bounds = output.bounds(&implementation.bounds)?;
            output.text.push_str(" {\n");
            if let Some(trait_type) = &implementation.trait_type {
                for (member, ty) in &trait_type.associated_types {
                    let name = &member
                        .path
                        .last()
                        .ok_or_else(|| DeclarationError("missing associated name".into()))?
                        .name;
                    let member_id = associated_type_id(&id, name);
                    let start = output.text.len();
                    output.text.push_str("    type ");
                    let name_span = output.name(name);
                    output.text.push_str(" = ");
                    let value_span = output.name(&self.type_spelling(ty)?);
                    output.text.push_str(";\n");
                    output.site(member_id, start, name_span, vec![], vec![value_span]);
                }
            }
            for method in &implementation.methods {
                output.function(ModuleDecl::method_id(&id, &method.name), method, true)?;
            }
            output.text.push_str("}\n\n");
            output.site(id.clone(), start, name_span, generics, vec![]);
            output
                .sites
                .get_mut(&id)
                .expect("implementation declaration")
                .bounds = bounds;
        }
        for function in &self.functions {
            output.function(
                self.definition(DefinitionKind::Function, &function.name),
                function,
                false,
            )?;
        }
        while output.text.ends_with("\n\n") {
            output.text.pop();
        }
        let source = DeclarationSource {
            uri: format!(
                "kagari://native/{}/{}.kgr",
                self.identity.package.0,
                self.identity.path.join("/")
            ),
            text: output.text,
            sites: output.sites,
        };
        Ok(source)
    }

    fn type_spelling(&self, ty: &Ty) -> Result<String, DeclarationError> {
        if !ty.within_wire_limits() {
            return Err(DeclarationError("type exceeds native API limits".into()));
        }
        self.spell(ty)
    }

    fn spell(&self, ty: &Ty) -> Result<String, DeclarationError> {
        let join = |items: &[Ty]| -> Result<String, DeclarationError> {
            Ok(items
                .iter()
                .map(|item| self.spell(item))
                .collect::<Result<Vec<_>, _>>()?
                .join(", "))
        };
        Ok(match ty {
            Ty::Builtin(kind) => match kind {
                BuiltinType::Never => "!",
                BuiltinType::Unit => "()",
                BuiltinType::Bool => "bool",
                BuiltinType::I8 => "i8",
                BuiltinType::I16 => "i16",
                BuiltinType::I32 => "i32",
                BuiltinType::I64 => "i64",
                BuiltinType::ISize => "isize",
                BuiltinType::U8 => "u8",
                BuiltinType::U16 => "u16",
                BuiltinType::U32 => "u32",
                BuiltinType::U64 => "u64",
                BuiltinType::USize => "usize",
                BuiltinType::F32 => "f32",
                BuiltinType::F64 => "f64",
                BuiltinType::String => {
                    return self.representation_name(NativeTypeConstructor::String, "String");
                }
            }
            .into(),
            Ty::Parameter { owner, position } => parameter_spelling(owner, *position),
            Ty::SelfType(_) => "Self".into(),
            Ty::Array(item, CollectionAccess::ReadOnly) => format!("[{}]", self.spell(item)?),
            Ty::Array(item, CollectionAccess::Mutable) => {
                let name = self.representation_name(NativeTypeConstructor::Array, "Vec")?;
                format!("{name}<{}>", self.spell(item)?)
            }
            Ty::Map {
                key,
                value,
                access: CollectionAccess::Mutable,
            } => {
                let name = self.representation_name(NativeTypeConstructor::Map, "HashMap")?;
                format!("{name}<{}, {}>", self.spell(key)?, self.spell(value)?)
            }
            Ty::Set(item, CollectionAccess::Mutable) => {
                let name = self.representation_name(NativeTypeConstructor::Set, "HashSet")?;
                format!("{name}<{}>", self.spell(item)?)
            }
            Ty::Tuple(items) => format!(
                "({}{})",
                join(items)?,
                if items.len() == 1 { "," } else { "" }
            ),
            Ty::Function { params, result } => {
                format!("fn({}) -> {}", join(params)?, self.spell(result)?)
            }
            Ty::Trait(ty) | Ty::NativeObject(ty) | Ty::Enum(ty) => self.nominal_spelling(ty)?,
            Ty::Projection {
                receiver,
                interface,
                member,
                arguments,
            } if arguments.is_empty() => {
                let name = &member
                    .path
                    .last()
                    .ok_or_else(|| DeclarationError("missing associated name".into()))?
                    .name;
                format!(
                    "<{} as {}>::{name}",
                    self.spell(receiver)?,
                    self.nominal_spelling(interface)?
                )
            }
            Ty::Range(item, kind) => {
                let name =
                    self.representation_name(NativeTypeConstructor::Range(*kind), kind.name())?;
                if NativeTypeConstructor::Range(*kind).arity() == 0 {
                    name
                } else {
                    format!("{name}<{}>", self.spell(item)?)
                }
            }
            Ty::Iter(item) => {
                let name = self.representation_name(NativeTypeConstructor::Iter, "Iter")?;
                format!("{name}<{}>", self.spell(item)?)
            }

            _ => {
                return Err(DeclarationError(
                    "unsupported type in initial native API importer".into(),
                ));
            }
        })
    }

    fn representation_name(
        &self,
        constructor: NativeTypeConstructor,
        fallback: &str,
    ) -> Result<String, DeclarationError> {
        if let Some(ty) = self
            .types
            .iter()
            .find(|ty| ty.kind == TypeDefKind::Native(constructor))
        {
            return Ok(ty.name.clone());
        }
        let candidates: Vec<_> = self
            .providers
            .iter()
            .filter(|provider| provider.identity != self.identity)
            .flat_map(|provider| {
                provider
                    .types
                    .iter()
                    .filter(move |ty| ty.kind == TypeDefKind::Native(constructor))
                    .map(move |ty| (provider, ty))
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
        let [(provider, ty)] = candidates.as_slice() else {
            return Err(DeclarationError(
                "missing or ambiguous native representation declaration".into(),
            ));
        };
        Ok(format!(
            "{}::{}",
            module_path(&provider.identity, self.providers),
            ty.name
        ))
    }

    fn nominal_spelling(&self, ty: &NominalTy) -> Result<String, DeclarationError> {
        let name = ty
            .declaration
            .path
            .last()
            .ok_or_else(|| DeclarationError("missing declaration name".into()))?;
        let mut text = if ty.declaration.module == self.identity {
            name.name.clone()
        } else {
            format!(
                "{}::{}",
                module_path(&ty.declaration.module, self.providers),
                name.name
            )
        };
        let mut args = ty
            .arguments
            .iter()
            .map(|ty| self.type_spelling(ty))
            .collect::<Result<Vec<_>, _>>()?;
        for (id, value) in &ty.associated_types {
            args.push(format!(
                "{} = {}",
                id.path
                    .last()
                    .ok_or_else(|| DeclarationError("missing associated name".into()))?
                    .name,
                self.type_spelling(value)?
            ));
        }
        if !args.is_empty() {
            text.push_str(&format!("<{}>", args.join(", ")));
        }
        Ok(text)
    }
}

struct Renderer<'a> {
    module: &'a DeclarationView<'a>,
    text: String,
    sites: BTreeMap<DefinitionPath, NativeDeclarationSite>,
}

impl Renderer<'_> {
    fn doc(&mut self, id: &DefinitionPath) {
        if let Some(documentation) = self.module.documentation.get(id) {
            for line in documentation.lines() {
                if id.path.len() > 1 {
                    self.text.push_str("    ");
                }
                self.text.push_str("/// ");
                self.text.push_str(line);
                self.text.push('\n');
            }
        }
    }

    fn name(&mut self, name: &str) -> Span {
        let start = self.text.len();
        self.text.push_str(name);
        Span::new(start, self.text.len())
    }

    fn generics(&mut self, parameters: &[GenericParam]) -> Vec<Span> {
        if parameters.is_empty() {
            return vec![];
        }
        self.text.push('<');
        let mut spans = vec![];
        for (index, parameter) in parameters.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            spans.push(self.name(&parameter_spelling(&parameter.owner, parameter.position)));
        }
        self.text.push('>');
        spans
    }

    fn site(
        &mut self,
        id: DefinitionPath,
        start: usize,
        name_span: Span,
        generics: Vec<Span>,
        parameters: Vec<Span>,
    ) {
        self.sites.insert(
            id,
            NativeDeclarationSite {
                span: Span::new(start, self.text.trim_end_matches('\n').len()),
                name_span,
                generics,
                parameters,
                bounds: vec![],
            },
        );
    }

    fn function(
        &mut self,
        id: DefinitionPath,
        function: &FnDecl,
        method: bool,
    ) -> Result<(), DeclarationError> {
        self.doc(&id);
        let start = self.text.len();
        self.text
            .push_str(if self.module.private_functions.contains(&id) {
                "fn "
            } else if !method {
                "pub fn "
            } else if id.path[0].kind == DefinitionKind::Trait {
                "    fn "
            } else {
                "    pub fn "
            });
        let name_span = self.name(&function.name);
        let own = function
            .generic_params
            .iter()
            .filter(|parameter| !method || parameter.owner == id)
            .cloned()
            .collect::<Vec<_>>();
        let generics = self.generics(&own);
        self.text.push('(');
        let mut parameters = vec![];
        for (index, parameter) in function.params.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            parameters.push(self.name(&parameter.name));
            if parameter.name != "self" {
                self.text.push_str(": ");
                self.text
                    .push_str(&self.module.type_spelling(&parameter.ty)?);
            }
        }
        self.text.push(')');
        if function.return_type != Ty::Builtin(BuiltinType::Unit) {
            self.text.push_str(" -> ");
            self.text
                .push_str(&self.module.type_spelling(&function.return_type)?);
        }
        let bounds = self.bounds(&function.bounds)?;
        if let CallableImplementation::NativeDefault(application) = &function.implementation {
            self.default_body(application, function)?;
        } else {
            self.text.push_str(";\n");
        }
        self.site(id.clone(), start, name_span, generics, parameters);
        self.sites
            .get_mut(&id)
            .expect("rendered declaration")
            .bounds = bounds;
        Ok(())
    }

    /// Renders the registration recipe as a real, ordinarily checked tail call.
    fn default_body(
        &mut self,
        application: &NativeDefaultApplication,
        function: &FnDecl,
    ) -> Result<(), DeclarationError> {
        let target = &application.declaration;
        let path = target
            .path
            .iter()
            .map(|part| part.name.as_str())
            .collect::<Vec<_>>()
            .join("::");
        let path = if target.module == self.module.identity {
            path
        } else {
            format!(
                "{}::{path}",
                module_path(&target.module, self.module.providers)
            )
        };
        self.text.push_str(" {\n        ");
        self.text.push_str(&path);
        if !application.arguments.is_empty() {
            self.text.push_str("::<");
            for (index, argument) in application.arguments.iter().enumerate() {
                if index != 0 {
                    self.text.push_str(", ");
                }
                self.text.push_str(&self.module.type_spelling(argument)?);
            }
            self.text.push('>');
        }
        self.text.push('(');
        for (index, parameter) in function.params.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            self.text.push_str(&parameter.name);
        }
        self.text.push_str(")\n    }\n");
        Ok(())
    }

    fn bounds(
        &mut self,
        bounds: &[GenericBound],
    ) -> Result<Vec<NativeBoundSite>, DeclarationError> {
        let mut sites = vec![];
        if !bounds.is_empty() {
            self.text.push_str(" where ");
        }
        for (index, bound) in bounds.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            let target = self.name(&self.module.type_spelling(&bound.ty)?);
            let constraints = self.constraints(&bound.constraints)?;
            sites.push(NativeBoundSite {
                target,
                constraints,
            });
        }
        Ok(sites)
    }

    fn constraints(&mut self, constraints: &[Constraint]) -> Result<Vec<Span>, DeclarationError> {
        let mut spans = vec![];
        if !constraints.is_empty() {
            self.text.push_str(": ");
        }
        for (index, constraint) in constraints.iter().enumerate() {
            if index != 0 {
                self.text.push_str(" + ");
            }
            let name = match constraint {
                Constraint::Trait(trait_type) => self.module.nominal_spelling(trait_type)?,
                Constraint::Standard(kind) => kind
                    .source_bound_name()
                    .ok_or_else(|| DeclarationError("native bound has no source name".into()))?
                    .into(),
            };
            spans.push(self.name(&name));
        }
        Ok(spans)
    }
}

/// Separate lexical names for method binders and their enclosing declaration.
pub fn parameter_spelling(owner: &DefinitionPath, position: usize) -> String {
    let prefix = if owner
        .path
        .last()
        .is_some_and(|part| part.kind == DefinitionKind::Method)
    {
        "M"
    } else {
        "T"
    };
    format!("{prefix}{position}")
}
