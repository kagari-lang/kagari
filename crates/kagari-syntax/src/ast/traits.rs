use crate::{kind::SyntaxKind, syntax_node::SyntaxNode};

pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;

    fn cast(syntax: SyntaxNode) -> Option<Self>;

    fn syntax(&self) -> &SyntaxNode;

    /// Consecutive outer line documentation preceding this declaration.
    fn documentation(&self, source: &str) -> String {
        let start = usize::from(self.syntax().text_range().start());
        let Some(prefix) = source.get(..start) else {
            return String::new();
        };
        let mut lines = prefix.lines().rev();
        let mut docs = Vec::new();
        if let Some(last) = lines.next() {
            if !last.trim().is_empty() {
                if let Some(doc) = last.trim_start().strip_prefix("///") {
                    docs.push(doc.strip_prefix(' ').unwrap_or(doc));
                } else {
                    return String::new();
                }
            }
        }
        for line in lines {
            let Some(doc) = line.trim_start().strip_prefix("///") else {
                break;
            };
            docs.push(doc.strip_prefix(' ').unwrap_or(doc));
        }
        docs.reverse();
        docs.join("\n")
    }
}
