# AI assistant panel

Status: redesigned 2026-11-09. Read-only chat about the open PDF; it never edits the document.

## Layout (`crates/ui-egui/src/ai_ui/`)
- `header.rs`: title, the "⋯" menu (new conversation, export to Markdown), close, and the agent/model row.
- `messages.rs`: the conversation. The empty state is a welcome with four suggestion rows; answers flow as Markdown across the full width (no bubble) under an "Assistant" name row, with Copy / Regenerate / model in the footer; questions are right-aligned soft bubbles; errors are a tinted card with Retry.
- `composer.rs`: status line, quick-action pills (only once there is a conversation), the input card (context picker on the left, round Send/Stop button on the right) and the privacy note. It sits in a bottom `egui::Panel`, so wrapped rows never overlap what is below.
- `native.rs`: state (`AiPane`), the request/answer plumbing and the frame that joins the three parts.

## Decisions
- The agent and model choice lives in `ai_model.rs` (`AiChoice`, saved with the preferences) and is one widget (`picker`) that every AI panel shows, so the chat and the translation panel share it.
- Combo boxes are named through `widget_info` (Agent, Model, Context): the accessibility check in the tests requires a name on every widget.
- Esc stops an answer while the pointer is over the panel (`Actions::pointer_over`, measured on the whole panel) or the input has focus.
- Tables in answers are framed and unstriped (egui's striping only painted the first cell of each row).
