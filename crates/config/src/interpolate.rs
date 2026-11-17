//! `${...}` variable interpolation used in commands, URLs and environment values.
//!
//! Supported variables (resolved by the runtime):
//!
//! | Variable                   | Value                                       |
//! |----------------------------|---------------------------------------------|
//! | `${port}`                  | port of the current service / frontend      |
//! | `${services.<name>.port}`  | port of another service                     |
//! | `${services.<name>.url}`   | base URL of another service                 |
//! | `${frontend.url}`          | URL of the frontend                         |
//! | `${project.root}`          | absolute project root                       |
//! | `${mode}`                  | `development` or `production`               |
//! | `${env.NAME}`              | environment variable of the shell process   |
//!
//! `$$` produces a literal `$`.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InterpolationError {
    #[error("unknown variable `${{{0}}}`")]
    Unknown(String),
    #[error("unterminated `${{` in `{0}`")]
    Unterminated(String),
}

/// Replace all `${name}` occurrences using `lookup`.
pub fn interpolate(
    input: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<String, InterpolationError> {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some((_, '$')) => {
                chars.next();
                out.push('$');
            }
            Some((_, '{')) => {
                chars.next();
                let rest = &input[i + 2..];
                let end = rest
                    .find('}')
                    .ok_or_else(|| InterpolationError::Unterminated(input.to_string()))?;
                let name = rest[..end].trim();
                let value =
                    lookup(name).ok_or_else(|| InterpolationError::Unknown(name.to_string()))?;
                out.push_str(&value);
                // skip the variable body and the closing brace
                for _ in 0..rest[..=end].chars().count() {
                    chars.next();
                }
            }
            _ => out.push('$'),
        }
    }
    Ok(out)
}

/// Extract all variable names referenced in `input` (used for validation).
pub fn variables(input: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let mut rest = input;
    while let Some(pos) = rest.find("${") {
        if pos > 0 && rest[..pos].ends_with('$') {
            rest = &rest[pos + 2..];
            continue;
        }
        let after = &rest[pos + 2..];
        match after.find('}') {
            Some(end) => {
                vars.push(after[..end].trim().to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    vars
}
