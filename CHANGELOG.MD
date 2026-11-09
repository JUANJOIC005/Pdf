# Registro de cambios

- 2026-11-08: Pdf se sincroniza con PdfCraft 0.19.0 como versión 0.19.1, conservando la identidad y funciones del fork.
\n- 2026-11-08: El sincronizador de upstream atribuye los commits al propietario y valida las actualizaciones en una rama antes de integrarlas.

- 2026-11-08: La Release Linux incorpora las traducciones de `craft-i18n` y publica el DEB aunque la cuota de artifacts temporales esté llena.
- 2026-11-08: Los paquetes de Pdf usan el logotipo de Adobe Acrobat solicitado para el lanzador y la ventana.
- 2026-11-09: Piloto de traducción en el sitio (`crates/edit` `rewrite_page_blocks`, `tag_runs`/`untag_runs`, ejemplo `translate_page`; `crates/ai` `translate_segments`); `has_glyph` comprueba la fuente CID subset embebida y los índices con puntos guía ya no se fusionan; ver `docs/translate-in-place.md`.
- 2026-11-09: Panel de IA rediseñado (`crates/ui-egui/src/ai_ui/`: cabecera, mensajes y compositor; selector de agente y modelo compartido en `ai_model.rs`; respuestas en Markdown sin burbuja, tablas con borde); ver `docs/ai-panel.md`.
