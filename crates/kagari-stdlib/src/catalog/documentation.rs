//! Generic Markdown rendering; prose and examples are owned by contributors.
pub(crate) fn example(description: &str, code: &str) -> String {
    format!(
        "{}\n\n# Examples\n\n```kgr\n{code}\n```",
        prose(description)
    )
}

pub(crate) fn markdown(document: (&str, &str)) -> String {
    example(document.0, document.1)
}

/// Wrap authored prose without changing the separately supplied executable code.
pub(crate) fn prose(description: &str) -> String {
    let mut output = String::new();
    for line in description.lines() {
        if !output.is_empty() {
            output.push('\n');
        }
        if line.starts_with(['#', '|'])
            || line.starts_with("    ")
            || line.starts_with("- ")
            || line.ends_with("  ")
        {
            output.push_str(line);
            continue;
        }
        let mut width = 0;
        for word in line.split_whitespace() {
            let length = word.chars().count();
            if width != 0 {
                if width + length + 1 > 80 {
                    output.push('\n');
                    width = 0;
                } else {
                    output.push(' ');
                    width += 1;
                }
            }
            output.push_str(word);
            width += length;
        }
    }
    output
}
