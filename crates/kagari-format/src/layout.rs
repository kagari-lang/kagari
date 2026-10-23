//! Small layout documents and the sole owner of width decisions.

use kagari_common::cancellation::{CancellationToken, Cancelled};

#[derive(Debug, Clone)]
pub(crate) enum Doc {
    Text(String),
    RequiredLine,
    Line { flat: &'static str, hard: bool },
    Concat(Vec<Doc>),
    Indent(Box<Doc>),
    Group(Box<Doc>),
}

impl Doc {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn soft(flat: &'static str) -> Self {
        Self::Line { flat, hard: false }
    }

    pub fn hard() -> Self {
        Self::Line {
            flat: " ",
            hard: true,
        }
    }

    pub fn group(self) -> Self {
        Self::Group(Box::new(self))
    }

    pub fn indent(self) -> Self {
        Self::Indent(Box::new(self))
    }
}

pub(crate) fn print(
    doc: &Doc,
    width: usize,
    indent: usize,
    newline: &str,
    cancel: &CancellationToken,
) -> Result<String, Cancelled> {
    let mut printer = Printer {
        output: String::new(),
        column: 0,
        pending_indent: 0,
        width,
        indent,
        newline,
        cancel,
    };
    printer.write(doc, 0, false)?;
    while printer.output.ends_with(['\r', '\n']) {
        printer.output.pop();
    }
    printer.output.push_str(newline);
    cancel.check()?;
    Ok(printer.output)
}

struct Printer<'a> {
    output: String,
    column: usize,
    pending_indent: usize,
    width: usize,
    indent: usize,
    newline: &'a str,
    cancel: &'a CancellationToken,
}

impl Printer<'_> {
    fn write(&mut self, doc: &Doc, depth: usize, flat: bool) -> Result<(), Cancelled> {
        self.cancel.check()?;
        match doc {
            Doc::Text(text) => {
                if !text.is_empty() {
                    if self.pending_indent > 0 {
                        self.output.push_str(&" ".repeat(self.pending_indent));
                        self.column = self.pending_indent;
                        self.pending_indent = 0;
                    }
                    self.output.push_str(text);
                    self.column = text
                        .rsplit_once('\n')
                        .map_or(self.column + text.chars().count(), |(_, tail)| {
                            tail.chars().count()
                        });
                }
            }
            Doc::RequiredLine => {
                self.output.push_str(self.newline);
                self.column = 0;
                self.pending_indent = depth * self.indent;
            }
            Doc::Line {
                flat: replacement,
                hard,
            } => {
                if flat && !hard {
                    self.write(&Doc::text(*replacement), depth, flat)?;
                } else {
                    self.output.push_str(self.newline);
                    self.column = 0;
                    self.pending_indent = depth * self.indent;
                }
            }
            Doc::Concat(parts) => {
                for part in parts {
                    self.write(part, depth, flat)?;
                }
            }
            Doc::Indent(inner) => self.write(inner, depth + 1, flat)?,
            Doc::Group(inner) => {
                let remaining = self
                    .width
                    .saturating_sub(self.column.max(self.pending_indent));
                let fits = flat || flat_width(inner, remaining, self.cancel)?.is_some();
                self.write(inner, depth, fits)?;
            }
        }
        Ok(())
    }
}

fn flat_width(
    doc: &Doc,
    budget: usize,
    cancel: &CancellationToken,
) -> Result<Option<usize>, Cancelled> {
    cancel.check()?;
    let width = match doc {
        Doc::Text(text) if !text.contains(['\r', '\n']) => Some(text.chars().count()),
        Doc::Text(_) | Doc::RequiredLine | Doc::Line { hard: true, .. } => None,
        Doc::Line { flat, .. } => Some(flat.len()),
        Doc::Indent(inner) | Doc::Group(inner) => flat_width(inner, budget, cancel)?,
        Doc::Concat(parts) => {
            let mut used = 0;
            for part in parts {
                let Some(width) = flat_width(part, budget.saturating_sub(used), cancel)? else {
                    return Ok(None);
                };
                used += width;
                if used > budget {
                    return Ok(None);
                }
            }
            Some(used)
        }
    };
    Ok(width.filter(|width| *width <= budget))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_choose_width_and_nested_indentation_without_syntax() {
        let list = Doc::Concat(vec![
            Doc::text("call("),
            Doc::Concat(vec![
                Doc::soft(""),
                Doc::text("first,"),
                Doc::soft(" "),
                Doc::text("second"),
            ])
            .indent(),
            Doc::soft(""),
            Doc::text(")"),
        ])
        .group();
        assert_eq!(
            print(&list, 100, 4, "\n", &Default::default()).unwrap(),
            "call(first, second)\n"
        );
        assert_eq!(
            print(&list, 12, 2, "\r\n", &Default::default()).unwrap(),
            "call(\r\n  first,\r\n  second\r\n)\r\n"
        );
        let literal = Doc::Concat(vec![Doc::hard(), Doc::text("\"first\n  second\n\"")]).indent();
        assert_eq!(
            print(&literal, 1, 4, "\r\n", &Default::default()).unwrap(),
            "\r\n    \"first\n  second\n\"\r\n"
        );
    }
}
