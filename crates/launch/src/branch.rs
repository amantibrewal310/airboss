//! Branch names: `core`'s rule, and the name Do it generates for a task.

use airboss_core::TaskId;

/// The longest slug in a generated name.
const SLUG_MAX: usize = 40;

/// `core`'s branch rule (rule 25): only `[A-Za-z0-9._/-]`, 1 to 200 characters, no `..`,
/// not starting with `-` or `/`, not ending with `/` or `.lock`. `git check-ref-format
/// --branch` is checked as well before a name is used.
pub fn follows_rule(name: &str) -> bool {
    let chars_ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'));
    let shape_ok = !name.contains("..")
        && !name.starts_with(['-', '/'])
        && !name.ends_with('/')
        && !name.ends_with(".lock");
    chars_ok && shape_ok && !name.is_empty() && name.len() <= 200
}

/// The title lowercased, ASCII letters and digits kept, every other run of characters one
/// `-`, trimmed of `-`, and cut to 40 characters at the last `-`.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.len() <= SLUG_MAX {
        return out.to_string();
    }
    // Only ASCII is left, so byte offsets are character offsets.
    let head = &out[..SLUG_MAX];
    if out.as_bytes()[SLUG_MAX] == b'-' {
        return head.to_string();
    }
    match head.rfind('-') {
        Some(at) => head[..at].to_string(),
        None => head.to_string(),
    }
}

/// `task-<id>-<slug>`, or `task-<id>` when the title has no ASCII letters or digits.
pub fn task_branch(id: TaskId, title: &str) -> String {
    match slug(title) {
        s if s.is_empty() => format!("task-{}", id.0),
        s => format!("task-{}-{s}", id.0),
    }
}
