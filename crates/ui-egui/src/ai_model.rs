//! The agent and model every AI panel uses, and the one widget that chooses them.
//!
//! The choice is app-wide (`PdfCraftApp::ai_choice`) and persisted with the other preferences, so
//! the chat and the translation panel share it. Plain data only: it exists on the web too, where
//! the panels themselves are a stub.

use pdfcraft_ai::{ModelChoice, Options, Provider, presets, validate_model_id};

use crate::theme::Tokens;

/// Which agent to ask and which model each agent last used (`Auto` passes no model, so the agent
/// uses its own default).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiChoice {
    pub provider: Provider,
    pub model_claude: ModelChoice,
    pub model_codex: ModelChoice,
}

impl AiChoice {
    /// The model of the selected agent.
    pub fn model(&self) -> &ModelChoice {
        match self.provider {
            Provider::Claude => &self.model_claude,
            Provider::Codex => &self.model_codex,
        }
    }

    fn model_mut(&mut self) -> &mut ModelChoice {
        match self.provider {
            Provider::Claude => &mut self.model_claude,
            Provider::Codex => &mut self.model_codex,
        }
    }

    /// What `pdfcraft_ai` needs for a request.
    pub fn options(&self) -> Options {
        Options { provider: self.provider, model: self.model().clone() }
    }

    /// Reads the saved preference keys (`ai_provider`, `ai_model_claude`, `ai_model_codex`).
    /// Settings files are untrusted: an unknown agent or an invalid model id keeps the default.
    pub fn restore(&mut self, v: &serde_json::Value) {
        if let Some(p) = v["ai_provider"].as_str().and_then(Provider::parse) {
            self.provider = p;
        }
        if let Some(m) = model_from_json(&v["ai_model_claude"]) {
            self.model_claude = m;
        }
        if let Some(m) = model_from_json(&v["ai_model_codex"]) {
            self.model_codex = m;
        }
    }
}

/// The saved form of a model: `null` for Automatic, otherwise the identifier.
pub fn model_to_json(m: &ModelChoice) -> serde_json::Value {
    match m {
        ModelChoice::Auto => serde_json::Value::Null,
        ModelChoice::Named(id) => serde_json::Value::String(id.clone()),
    }
}

fn model_from_json(v: &serde_json::Value) -> Option<ModelChoice> {
    match v {
        serde_json::Value::Null => Some(ModelChoice::Auto),
        serde_json::Value::String(id) if validate_model_id(id).is_ok() => Some(ModelChoice::Named(id.clone())),
        _ => None,
    }
}

/// Whether the agent's command is installed. Always false on the web, where there are no agents.
fn installed(p: Provider) -> bool {
    cfg!(not(target_arch = "wasm32")) && pdfcraft_ai::find_program(p.id()).is_some()
}

/// The saved or default agent may not be installed (Codex is the default, and only Claude may be):
/// use the first one that is, so the panels work without the user having to find the selector.
fn use_an_installed_agent(choice: &mut AiChoice) {
    if !installed(choice.provider)
        && let Some(other) = Provider::ALL.into_iter().find(|p| installed(*p))
    {
        choice.provider = other;
    }
}

/// The agent and model selectors. Every AI panel shows this one widget.
pub fn picker(ui: &mut egui::Ui, choice: &mut AiChoice, t: &Tokens) {
    use_an_installed_agent(choice);
    let id = ui.scope_id().with("ai-model-picker");
    // One compact row; the combos carry their names for tooltips and assistive tech.
    ui.horizontal(|ui| {
        let _ = t;
        let agent_name = tl!("Agent");
        let shown = provider_label(choice.provider);
        let r = craft_ui::ComboBox::from_id_salt(id.with("agent")).selected_text(shown).show_ui(ui, |ui| {
            for provider in Provider::ALL {
                ui.selectable_value(&mut choice.provider, provider, provider_label(provider));
            }
        });
        r.response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, agent_name);
            info.current_text_value = Some(shown.to_string());
            info
        });
        r.response.on_hover_text(agent_name);
        let model = model_combo(ui, choice, id);
        model.on_hover_text(tl!("Model"));
    });
    custom_field(ui, choice, id);
}

/// The agent's name as shown to the user.
pub fn provider_label(p: Provider) -> &'static str {
    match p {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
    }
}

fn is_custom(choice: &AiChoice, forced: bool) -> bool {
    forced || matches!(choice.model(), ModelChoice::Named(n) if !presets(choice.provider).contains(&n.as_str()))
}

fn model_combo(ui: &mut egui::Ui, choice: &mut AiChoice, id: egui::Id) -> egui::Response {
    // "Custom…" stays selected while its field is open, even before a valid id is typed.
    let forced_key = id.with(("custom", choice.provider.id()));
    let mut forced: bool = ui.data(|d| d.get_temp(forced_key)).unwrap_or(false);
    let custom = is_custom(choice, forced);
    let shown = match choice.model() {
        _ if custom => tl!("Custom…").to_string(),
        ModelChoice::Auto => tl!("Automatic").to_string(),
        ModelChoice::Named(n) => n.clone(),
    };
    let provider = choice.provider;
    let model_name = tl!("Model");
    let shown_text = shown.clone();
    let combo = craft_ui::ComboBox::from_id_salt(id.with("model")).selected_text(shown).show_ui(ui, |ui| {
        if ui.selectable_label(!custom && *choice.model() == ModelChoice::Auto, tl!("Automatic")).clicked() {
            *choice.model_mut() = ModelChoice::Auto;
            forced = false;
        }
        for name in presets(provider) {
            let on = !custom && matches!(choice.model(), ModelChoice::Named(n) if n == name);
            if ui.selectable_label(on, *name).clicked() {
                *choice.model_mut() = ModelChoice::Named((*name).to_string());
                forced = false;
            }
        }
        if ui.selectable_label(custom, tl!("Custom…")).clicked() {
            forced = true;
        }
    });
    ui.data_mut(|d| d.insert_temp(forced_key, forced));
    combo.response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, model_name);
        info.current_text_value = Some(shown_text.clone());
        info
    });
    combo.response
}

/// The text field of "Custom…": only a valid identifier is stored in the choice.
fn custom_field(ui: &mut egui::Ui, choice: &mut AiChoice, id: egui::Id) {
    let forced_key = id.with(("custom", choice.provider.id()));
    let forced: bool = ui.data(|d| d.get_temp(forced_key)).unwrap_or(false);
    if !is_custom(choice, forced) {
        return;
    }
    let draft_key = id.with(("draft", choice.provider.id()));
    let mut draft: String = ui.data(|d| d.get_temp(draft_key)).unwrap_or_else(|| match choice.model() {
        ModelChoice::Named(n) => n.clone(),
        ModelChoice::Auto => String::new(),
    });
    let r = ui.add(crate::widgets::line(&mut draft).desired_width(f32::INFINITY).hint_text(tl!("Model identifier")));
    if r.changed() {
        if validate_model_id(&draft).is_ok() {
            *choice.model_mut() = ModelChoice::Named(draft.clone());
        } else if draft.is_empty() {
            *choice.model_mut() = ModelChoice::Auto;
        }
    }
    if !draft.is_empty() && validate_model_id(&draft).is_err() {
        ui.label(egui::RichText::new(tl!("Use letters, digits and . _ : - [ ] only (up to 100).")).small().color(ui.visuals().error_fg_color));
    }
    ui.data_mut(|d| d.insert_temp(draft_key, draft));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn restore_keeps_valid_values_and_ignores_hostile_ones() {
        let mut c = AiChoice::default();
        c.restore(&json!({"ai_provider": "claude", "ai_model_claude": "opus", "ai_model_codex": "--evil"}));
        assert_eq!(c.provider, Provider::Claude);
        assert_eq!(c.model_claude, ModelChoice::Named("opus".into()));
        assert_eq!(c.model_codex, ModelChoice::Auto);
        c.restore(&json!({"ai_provider": "gpt", "ai_model_claude": 7}));
        assert_eq!(c.provider, Provider::Claude);
        assert_eq!(c.model_claude, ModelChoice::Named("opus".into()));
        c.restore(&json!({"ai_model_claude": null}));
        assert_eq!(c.model_claude, ModelChoice::Auto);
    }

    #[test]
    fn options_follow_the_selected_agent() {
        let c = AiChoice { provider: Provider::Codex, model_claude: ModelChoice::Named("opus".into()), model_codex: ModelChoice::Auto };
        assert_eq!(c.options().model, ModelChoice::Auto);
        let c = AiChoice { provider: Provider::Claude, ..c };
        assert_eq!(c.options().model, ModelChoice::Named("opus".into()));
    }
}
