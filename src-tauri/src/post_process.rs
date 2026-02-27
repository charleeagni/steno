use crate::runtime::RuntimeError;

pub trait PostProcessor: Send + Sync {
    fn process(&self, text: &str) -> Result<String, RuntimeError>;
}

#[derive(Default)]
pub struct DefaultPostProcessor;

impl PostProcessor for DefaultPostProcessor {
    fn process(&self, text: &str) -> Result<String, RuntimeError> {
        Ok(remove_trailing_artifact(text))
    }
}

fn remove_trailing_artifact(text: &str) -> String {
    let trimmed = text.trim_end();
    let token_bounds = collect_token_bounds(trimmed);
    if token_bounds.len() < 4 {
        return trimmed.to_string();
    }

    let tail = &token_bounds[token_bounds.len() - 3..];
    let tail_words = [
        normalize_token(&trimmed[tail[0].0..tail[0].1]),
        normalize_token(&trimmed[tail[1].0..tail[1].1]),
        normalize_token(&trimmed[tail[2].0..tail[2].1]),
    ];

    if tail_words == ["you", "you", "typed"] {
        return trimmed[..tail[0].0].trim_end().to_string();
    }

    trimmed.to_string()
}

fn collect_token_bounds(text: &str) -> Vec<(usize, usize)> {
    let mut tokens = Vec::new();
    let mut token_start: Option<usize> = None;

    for (index, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(start) = token_start.take() {
                tokens.push((start, index));
            }
            continue;
        }

        if token_start.is_none() {
            token_start = Some(index);
        }
    }

    if let Some(start) = token_start {
        tokens.push((start, text.len()));
    }

    tokens
}

fn normalize_token(token: &str) -> String {
    token
        .trim_matches(|ch: char| !ch.is_alphanumeric())
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_trailing_you_you_typed_phrase() {
        let input = "Please send the note now you you typed";
        assert_eq!(remove_trailing_artifact(input), "Please send the note now");
    }

    #[test]
    fn keeps_short_phrase_intentional_text() {
        let input = "you you typed";
        assert_eq!(remove_trailing_artifact(input), "you you typed");
    }

    #[test]
    fn keeps_text_without_trailing_artifact() {
        let input = "Please send the note now.";
        assert_eq!(remove_trailing_artifact(input), "Please send the note now.");
    }
}
