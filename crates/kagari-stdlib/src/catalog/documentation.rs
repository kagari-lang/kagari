//! Generic Markdown rendering; prose and examples are owned by contributors.
pub(crate) fn example(description: &str, code: &str) -> String {
    format!("{description}\n\n# Examples\n\n```kgr\n{code}\n```")
}

pub(crate) fn markdown(document: (&str, &str)) -> String {
    example(document.0, document.1)
}
