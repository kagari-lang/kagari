use crate::{
    ast::{
        expr::{BlockExpr, Expr},
        macros::ast_node,
        misc::{
            FieldList, GenericParamList, Name, ParamList, Path, TraitBoundList, TraitRef,
            VariantList, WhereClause,
        },
        support,
        traits::AstNode,
        ty::TypeRef,
    },
    kind::SyntaxKind,
    syntax_node::SyntaxNode,
};

ast_node!(SourceFile, SourceFile);
ast_node!(Attribute, Attribute);
ast_node!(AttributeArgs, AttributeArgs);
ast_node!(AttributeArgList, AttributeArgList);
ast_node!(AttributeArg, AttributeArg);
ast_node!(AttributeValue, AttributeValue);
ast_node!(ModuleDef, ModuleDef);
ast_node!(ModuleBlock, ModuleBlock);
ast_node!(UseDecl, UseDecl);
ast_node!(UseTree, UseTree);
ast_node!(UseTreeList, UseTreeList);
ast_node!(TraitDef, TraitDef);
ast_node!(ImplBlock, ImplBlock);
ast_node!(MethodDef, MethodDef);
ast_node!(FnDef, FnDef);
ast_node!(ConstDef, ConstDef);
ast_node!(StructDef, StructDef);
ast_node!(EnumDef, EnumDef);
ast_node!(AssociatedType, AssociatedType);

impl AssociatedType {
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }
    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }
    pub fn bounds(&self) -> Option<super::misc::TraitBoundList> {
        support::child(self.syntax())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
    PublicSuper,
}

pub(crate) fn visibility_of(syntax: &SyntaxNode) -> Visibility {
    if support::token(syntax, SyntaxKind::PubKw).is_none() {
        Visibility::Private
    } else if support::token(syntax, SyntaxKind::SuperKw).is_some() {
        Visibility::PublicSuper
    } else {
        Visibility::Public
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    ModuleDef(ModuleDef),
    UseDecl(UseDecl),
    TraitDef(TraitDef),
    ImplBlock(ImplBlock),
    FnDef(FnDef),
    ConstDef(ConstDef),
    StructDef(StructDef),
    EnumDef(EnumDef),
}

impl AstNode for Item {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::ModuleDef
                | SyntaxKind::UseDecl
                | SyntaxKind::TraitDef
                | SyntaxKind::ImplBlock
                | SyntaxKind::FnDef
                | SyntaxKind::ConstDef
                | SyntaxKind::StructDef
                | SyntaxKind::EnumDef
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::ModuleDef => ModuleDef::cast(syntax).map(Self::ModuleDef),
            SyntaxKind::UseDecl => UseDecl::cast(syntax).map(Self::UseDecl),
            SyntaxKind::TraitDef => TraitDef::cast(syntax).map(Self::TraitDef),
            SyntaxKind::ImplBlock => ImplBlock::cast(syntax).map(Self::ImplBlock),
            SyntaxKind::FnDef => FnDef::cast(syntax).map(Self::FnDef),
            SyntaxKind::ConstDef => ConstDef::cast(syntax).map(Self::ConstDef),
            SyntaxKind::StructDef => StructDef::cast(syntax).map(Self::StructDef),
            SyntaxKind::EnumDef => EnumDef::cast(syntax).map(Self::EnumDef),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::ModuleDef(node) => node.syntax(),
            Self::UseDecl(node) => node.syntax(),
            Self::TraitDef(node) => node.syntax(),
            Self::ImplBlock(node) => node.syntax(),
            Self::FnDef(node) => node.syntax(),
            Self::ConstDef(node) => node.syntax(),
            Self::StructDef(node) => node.syntax(),
            Self::EnumDef(node) => node.syntax(),
        }
    }
}

impl Item {
    pub fn attributes(&self) -> impl Iterator<Item = Attribute> {
        self.syntax().children().filter_map(Attribute::cast)
    }
}

impl Attribute {
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.path().and_then(|path| path.text())
    }

    pub fn args(&self) -> Option<AttributeArgs> {
        support::child(self.syntax())
    }
}

impl AttributeArgs {
    pub fn arguments(&self) -> impl Iterator<Item = AttributeArg> {
        self.syntax()
            .children()
            .filter_map(AttributeArgList::cast)
            .flat_map(|list| list.syntax().children().filter_map(AttributeArg::cast))
    }
}

impl AttributeArg {
    pub fn name_text(&self) -> Option<String> {
        support::child::<Name>(self.syntax()).and_then(|name| name.text())
    }

    pub fn value(&self) -> Option<AttributeValue> {
        support::child(self.syntax())
    }
}

impl AttributeValue {
    pub fn literal(&self) -> Option<super::Literal> {
        support::child(self.syntax())
    }

    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    pub fn elements(&self) -> impl Iterator<Item = AttributeArg> {
        self.syntax()
            .children()
            .filter_map(AttributeArgList::cast)
            .flat_map(|list| list.syntax().children().filter_map(AttributeArg::cast))
    }
}

impl ModuleDef {
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn block(&self) -> Option<ModuleBlock> {
        support::child(self.syntax())
    }
}

impl ModuleBlock {
    pub fn items(&self) -> impl Iterator<Item = Item> {
        support::children(self.syntax())
    }
}

impl UseDecl {
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    pub fn tree(&self) -> Option<UseTree> {
        support::child(self.syntax())
    }
}

impl UseTree {
    pub fn is_glob(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::Star).is_some()
    }

    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    pub fn alias(&self) -> Option<Name> {
        let mut after_as = false;
        self.syntax()
            .children_with_tokens()
            .find_map(|element| match element {
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::AsKw => {
                    after_as = true;
                    None
                }
                rowan::NodeOrToken::Node(node) if after_as => Name::cast(node),
                _ => None,
            })
    }

    pub fn nested_trees(&self) -> impl Iterator<Item = UseTree> {
        self.syntax()
            .children()
            .filter_map(UseTreeList::cast)
            .flat_map(|list| list.trees().collect::<Vec<_>>())
    }
}

impl UseTreeList {
    pub fn trees(&self) -> impl Iterator<Item = UseTree> {
        support::children(self.syntax())
    }
}

impl TraitDef {
    pub fn associated_consts(&self) -> impl Iterator<Item = ConstDef> {
        support::children(self.syntax())
    }
    pub fn supertraits(&self) -> Option<TraitBoundList> {
        support::child(self.syntax())
    }
    pub fn associated_types(&self) -> impl Iterator<Item = AssociatedType> {
        support::children(self.syntax())
    }
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    pub fn methods(&self) -> impl Iterator<Item = MethodDef> {
        support::children(self.syntax())
    }
}

impl ImplBlock {
    pub fn associated_consts(&self) -> impl Iterator<Item = ConstDef> {
        support::children(self.syntax())
    }
    pub fn associated_types(&self) -> impl Iterator<Item = AssociatedType> {
        support::children(self.syntax())
    }
    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    pub fn trait_ref(&self) -> Option<TraitRef> {
        support::child(self.syntax())
    }

    pub fn target_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }

    pub fn methods(&self) -> impl Iterator<Item = MethodDef> {
        support::children(self.syntax())
    }
}

impl MethodDef {
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    pub fn param_list(&self) -> Option<ParamList> {
        support::child(self.syntax())
    }

    pub fn return_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }

    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }
}

impl SourceFile {
    pub fn items(&self) -> impl Iterator<Item = Item> {
        support::children(self.syntax())
    }
}

impl FnDef {
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn param_list(&self) -> Option<ParamList> {
        support::child(self.syntax())
    }

    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }

    pub fn return_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }

    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(self.syntax())
    }
}

impl ConstDef {
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn ty(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    pub fn initializer(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }
}

impl StructDef {
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn field_list(&self) -> Option<FieldList> {
        support::child(self.syntax())
    }

    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }
}

impl EnumDef {
    pub fn visibility(&self) -> Visibility {
        visibility_of(self.syntax())
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn variant_list(&self) -> Option<VariantList> {
        support::child(self.syntax())
    }

    pub fn generic_params(&self) -> Option<GenericParamList> {
        support::child(self.syntax())
    }
}
