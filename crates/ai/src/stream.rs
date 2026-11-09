//! Pure parsing of the agents' JSON-lines output into UI events.

use serde_json::Value;

use crate::Provider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Preparing,
    Thinking,
    Writing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Phase(Phase),
    /// The model the CLI reports using.
    Model(String),
    /// A chunk of the answer as it is written.
    Delta(String),
    /// The complete text of an answer message.
    Final(String),
}

/// Events carried by one output line. Missing or unexpected fields yield no events, never a panic.
pub fn parse(provider: Provider, line: &Value) -> Vec<Event> {
    let mut out = Vec::new();
    let kind = line["type"].as_str().unwrap_or_default();
    match provider {
        Provider::Claude => match kind {
            "system" if line["subtype"] == "init" => {
                if let Some(model) = line["model"].as_str().filter(|m| !m.is_empty()) {
                    out.push(Event::Model(model.to_string()));
                }
            }
            "stream_event" => {
                let event = &line["event"];
                if event["type"] == "content_block_delta"
                    && event["delta"]["type"] == "text_delta"
                    && let Some(text) = event["delta"]["text"].as_str().filter(|t| !t.is_empty())
                {
                    out.push(Event::Delta(text.to_string()));
                }
            }
            "assistant" => {
                let text = line["message"]["content"]
                    .as_array()
                    .map(|parts| {
                        parts.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().concat()
                    })
                    .unwrap_or_default();
                if !text.trim().is_empty() {
                    out.push(Event::Final(text));
                }
            }
            _ => {}
        },
        // Codex: only the documented `item.completed` / `agent_message` pair; no real output was
        // available to check anything else.
        Provider::Codex => {
            if kind == "item.completed"
                && line["item"]["type"] == "agent_message"
                && let Some(text) = line["item"]["text"].as_str().filter(|t| !t.trim().is_empty())
            {
                out.push(Event::Final(text.to_string()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_init_delta_and_final() {
        assert_eq!(
            parse(Provider::Claude, &json!({"type":"system","subtype":"init","model":"claude-x"})),
            vec![Event::Model("claude-x".into())]
        );
        let delta = json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Ho"}}});
        assert_eq!(parse(Provider::Claude, &delta), vec![Event::Delta("Ho".into())]);
        let fin = json!({"type":"assistant","message":{"content":[{"type":"text","text":"Hola"},{"type":"tool_use"}]}});
        assert_eq!(parse(Provider::Claude, &fin), vec![Event::Final("Hola".into())]);
    }

    #[test]
    fn claude_ignores_other_events() {
        let thinking = json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"x"}}});
        assert!(parse(Provider::Claude, &thinking).is_empty());
        assert!(parse(Provider::Claude, &json!({"type":"system","subtype":"hook"})).is_empty());
    }

    #[test]
    fn codex_agent_message() {
        let fin = json!({"type":"item.completed","item":{"type":"agent_message","text":"Resumen"}});
        assert_eq!(parse(Provider::Codex, &fin), vec![Event::Final("Resumen".into())]);
        assert!(parse(Provider::Codex, &json!({"type":"item.completed","item":{"type":"reasoning","text":"x"}})).is_empty());
    }

    #[test]
    fn tolerates_missing_and_wrong_typed_fields() {
        let hostile = [
            json!(null),
            json!({}),
            json!([]),
            json!("x"),
            json!({"type":5}),
            json!({"type":"assistant","message":{"content":"x"}}),
            json!({"type":"stream_event","event":null}),
            json!({"type":"item.completed","item":{"type":"agent_message","text":7}}),
            json!({"type":"system","subtype":"init","model":3}),
        ];
        for p in Provider::ALL {
            for v in &hostile {
                assert!(parse(p, v).is_empty(), "{v}");
            }
        }
    }
}
