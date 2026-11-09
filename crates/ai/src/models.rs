//! Model selection. `Auto` passes no `--model`, so the CLI uses the user's own default.

use crate::{AiError, Provider};

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ModelChoice {
    #[default]
    Auto,
    Named(String),
}

/// Aliases offered in the UI. Claude's come from `claude --help` ("fable", "opus", "sonnet");
/// codex is not installed here so nothing can be verified and no preset is invented. Any valid
/// identifier can still be typed in by hand.
pub fn presets(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &["fable", "opus", "sonnet"],
        Provider::Codex => &[],
    }
}

/// 1..=100 characters of `[A-Za-z0-9._:\-\[\]]`, not starting with `-` (it would be read as a flag).
pub fn validate_model_id(id: &str) -> Result<(), AiError> {
    let ok_len = (1..=100).contains(&id.chars().count());
    let ok_chars = id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-' | '[' | ']'));
    if ok_len && ok_chars && !id.starts_with('-') { Ok(()) } else { Err(AiError::InvalidModel) }
}

impl ModelChoice {
    /// The identifier to pass to the CLI, if any; validated.
    pub fn id(&self) -> Result<Option<&str>, AiError> {
        match self {
            Self::Auto => Ok(None),
            Self::Named(id) => validate_model_id(id).map(|()| Some(id.as_str())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plausible_ids() {
        for id in ["sonnet", "claude-sonnet-4-5", "gpt-5.1:high", "opus[1m]", "a.b_c"] {
            assert_eq!(validate_model_id(id), Ok(()), "{id}");
        }
        assert!(validate_model_id(&"a".repeat(100)).is_ok());
    }

    #[test]
    fn rejects_hostile_ids() {
        let long = "a".repeat(101);
        for id in ["", "-x", "--model", "a b", "a\nb", "modelo-ñ", "a;rm", "$(x)", "a/b", "\u{202e}x", long.as_str()] {
            assert_eq!(validate_model_id(id), Err(AiError::InvalidModel), "{id:?}");
        }
    }

    #[test]
    fn auto_passes_nothing_and_named_is_validated() {
        assert_eq!(ModelChoice::Auto.id(), Ok(None));
        assert_eq!(ModelChoice::Named("opus".into()).id(), Ok(Some("opus")));
        assert!(ModelChoice::Named("-x".into()).id().is_err());
        assert_eq!(ModelChoice::default(), ModelChoice::Auto);
    }

    #[test]
    fn presets_are_the_documented_aliases() {
        assert_eq!(presets(Provider::Claude), ["fable", "opus", "sonnet"]);
        assert!(presets(Provider::Codex).is_empty());
    }
}
