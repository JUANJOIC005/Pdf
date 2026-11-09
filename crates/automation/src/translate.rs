//! `text_translate`: translate pages of an open document into a new PDF file.
//!
//! The open document is never changed: the engine translates a private copy and this tool
//! writes it to `out`. `out` goes through the usual `--root` / automation-root confinement,
//! checked before anything is sent to a translator, and may not be the input file. The offline
//! `echo` provider is always allowed; `codex` and `claude` call an external agent and are refused
//! unless the session was given `allow_ai` ([`Automation::with_allow_ai`]), whichever way the tool
//! is reached (MCP, `command_run`, batches).

use std::path::Path;
use std::sync::atomic::AtomicBool;

use pdfcraft_engine::EditError;
use pdfcraft_engine::ai::{ModelChoice, Options, Provider, validate_model_id};
use pdfcraft_engine::translate::{AiTranslator, EchoTranslator, TranslateOptions, TranslationOutput, Translator, parse_page_range, translate_copy};
use serde_json::{Value, json};

use super::Automation;
use super::error::{Result, ToolError, failed};
use super::json::Args;
use super::paths::write_atomic;

impl Automation {
    pub(crate) fn text_translate(&mut self, a: &Args) -> Result<Value> {
        let invalid = |m: String| ToolError::InvalidArgs(m);
        let doc = self.doc(a)?;
        let id = doc.id;
        let count = doc.info.pages.len();
        let language = a.str("language")?.trim().to_owned();
        if language.is_empty() || language.chars().count() > 64 || language.chars().any(char::is_control) {
            return Err(invalid("language must be 1 to 64 characters without control characters".into()));
        }
        let pages = parse_page_range(a.opt_str("pages")?.unwrap_or("all"), count).map_err(invalid)?;
        let mut opts = TranslateOptions::default();
        if let Some(scale) = a.opt_num("min_scale")? {
            if !(0.7..=1.0).contains(&scale) {
                return Err(invalid("min_scale must be between 0.7 and 1".into()));
            }
            opts.min_scale = scale;
        }
        let provider = a.opt_str("provider")?.unwrap_or("echo");
        let translator: Box<dyn Translator> = match provider {
            "echo" => {
                if a.opt_str("model")?.is_some() {
                    return Err(invalid("model applies only to the codex and claude providers".into()));
                }
                Box::new(EchoTranslator)
            }
            other => {
                let parsed = Provider::parse(other).ok_or_else(|| invalid(format!("unknown provider `{other}` (echo, codex or claude)")))?;
                if !self.allow_ai {
                    return Err(failed(format!(
                        "the {other} provider is disabled: AI translation sends the text to an external agent and must be allowed explicitly (allow_ai)"
                    )));
                }
                let model = match a.opt_str("model")? {
                    Some(m) => {
                        validate_model_id(m).map_err(|e| invalid(e.to_string()))?;
                        ModelChoice::Named(m.to_owned())
                    }
                    None => ModelChoice::Auto,
                };
                Box::new(AiTranslator { options: Options { provider: parsed, model } })
            }
        };
        // Confinement first: nothing is sent anywhere for a path that may not be written.
        let out = a.str("out")?;
        let target = self.resolve(out, true)?;
        if let Some(own) = self.doc(a)?.path.as_deref()
            && same_file(Path::new(own), &target)
        {
            return Err(invalid("out must not be the input document: the translation is written to a new file".into()));
        }
        let cancel = AtomicBool::new(false);
        let TranslationOutput { bytes, report, signed } =
            translate_copy(&self.session, id, &pages, &language, &opts, translator.as_ref(), &mut |_| {}, &cancel)
                .map_err(|e: EditError| failed(e))?;
        write_atomic(&target, &bytes)?;
        let rows: Vec<Value> = report
            .pages
            .iter()
            .map(|p| {
                json!({
                    "page": p.page.saturating_add(1), "applied": p.applied, "unchanged": p.unchanged, "lost": p.lost, "merged": p.merged,
                    "failed": p.failed, "overflowed": p.overflowed, "substituted": p.substituted, "plain": p.plain,
                    "skipped": p.skipped.map(|s| format!("{s:?}")), "error": p.error,
                })
            })
            .collect();
        Ok(json!({
            "path": target.to_string_lossy(), "bytes": bytes.len(), "provider": provider, "language": language,
            "signed": signed, "signatures_not_kept": signed, "pages": rows,
            "applied": report.pages.iter().map(|p| p.applied).sum::<usize>(),
        }))
    }
}

/// Do the two paths name the same file (links followed when they exist)?
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}
