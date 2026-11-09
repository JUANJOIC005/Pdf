# Registro de cambios

- 2026-11-08: Pdf se sincroniza con PdfCraft 0.19.0 como versión 0.19.1, conservando la identidad y funciones del fork.
\n- 2026-11-08: El sincronizador de upstream atribuye los commits al propietario y valida las actualizaciones en una rama antes de integrarlas.

- 2026-11-08: La Release Linux incorpora las traducciones de `craft-i18n` y publica el DEB aunque la cuota de artifacts temporales esté llena.
- 2026-11-08: Los paquetes de Pdf usan el logotipo de Adobe Acrobat solicitado para el lanzador y la ventana.
- 2026-11-09: Piloto de traducción en el sitio (`crates/edit` `rewrite_page_blocks`, `tag_runs`/`untag_runs`, ejemplo `translate_page`; `crates/ai` `translate_segments`); `has_glyph` comprueba la fuente CID subset embebida y los índices con puntos guía ya no se fusionan; ver `docs/translate-in-place.md`.
- 2026-11-09: Panel de IA rediseñado (`crates/ui-egui/src/ai_ui/`: cabecera, mensajes y compositor; selector de agente y modelo compartido en `ai_model.rs`; respuestas en Markdown sin burbuja, tablas con borde); ver `docs/ai-panel.md`.
- 2026-11-09: Motor `translate_copy` y herramienta `text_translate` (traducción de páginas a una copia) integrados desde `agent/p2e-engine-translate`.
- 2026-11-09: Panel de traducción en la barra lateral (`crates/ui-egui/src/translate_ui.rs`, `RightPanel::Translate`): idioma, páginas, progreso con cancelar, abrir o guardar la copia; ver `docs/translate-panel.md`.
- 2026-11-09: Versión 0.19.2 (paneles de IA y traducción).
- 2026-11-09: Nuevo ajuste «Always open documents with continuous scrolling» (`force_continuous`): abre siempre en desplazamiento continuo, por encima de `/PageLayout` del PDF y de la última vista recordada; documentado en `docs/page-display.md`.
- 2026-11-09: El desplazamiento continuo pasa a estar activado por defecto (`force_continuous`), para que los PDF con `/PageLayout /SinglePage` (cursos de AWS) no se abran página a página.
- 2026-11-09: Los paneles de IA y traducción usan el primer agente instalado cuando el guardado o el predeterminado (Codex) no lo está (`ai_model::use_an_installed_agent`); antes fallaban sin avisar con solo Claude instalado.
- 2026-11-09: Versión 0.19.3.
