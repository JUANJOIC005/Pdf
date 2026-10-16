# Guía completa del código de Pdf

> Manual de orientación y modificación del repositorio privado `JUANJOIC005/Pdf`.
> Revisado sobre la rama `chore/pdf-rebrand` el 16 de octubre de 2026.

Esta guía explica cómo está construido el programa, dónde vive cada función, qué archivo conviene tocar para cada cambio, qué otras piezas resultan afectadas y cómo comprobar que una modificación no rompe el resto. Cubre todo el código propio, las pruebas, la configuración, los recursos, el empaquetado y las dependencias vendorizadas. Los miles de archivos generados dentro de `target/` no son código fuente y no se documentan uno por uno.

## 1. Respuesta rápida: ¿dónde tengo que ir?

| Si quieres cambiar… | Empieza por… | También suele afectar a… |
|---|---|---|
| Nombre visible, icono o identidad | `assets/app-icon/`, `apps/pdfcraft/src/main.rs`, `packaging/` | metadatos Linux/Windows, capturas, atribuciones |
| Ventana, menús, paneles o botones | `crates/ui-egui/src/` | traducciones, pruebas UI, registro de comandos |
| Un comando y su atajo | `crates/engine/src/commands.rs` | UI, automatización, paridad y pruebas |
| Una edición real del PDF | `crates/engine/src/lib.rs` (`Edit`) y el crate especializado | automatización, UI, deshacer/rehacer, guardado |
| Abrir o interpretar un PDF | `crates/cos/` | `model`, `render`, tests con PDFs dañados/cifrados |
| Mostrar una página | `crates/render/` | caché y lienzo de `ui-egui`, exportación/impresión |
| Editar texto o imágenes existentes | `crates/edit/` | `engine`, `ui-egui`, `automation`, fuentes |
| Páginas, combinar, dividir o marcadores | `crates/organize/` | `engine`, CLI, UI y automatización |
| Formularios | `crates/forms/` | JavaScript, XFA, engine, UI y automatización |
| Comentarios y anotaciones | `crates/annot/` | XFDF, firmas gráficas, medidas, UI |
| Redacción segura | `crates/redact/` | auditoría, guardado completo, UI, automatización |
| Firma digital | `crates/sign/` | identidades del sistema, UI, empaquetado y tests |
| OCR | `crates/ocr/` | modelos descargados, render, engine y paquetes |
| Crear PDF nuevo | `crates/create/` | UI, CLI/automatización, fuentes |
| Optimizar/comprimir | `crates/optimize/` | guardado completo, auditoría y render |
| Imprimir | `crates/print/` | `engine`, diálogo UI y spool del sistema |
| CLI o MCP | `apps/pdfcraft-cli/`, `crates/automation/` | schemas, confinamiento de rutas, engine |
| Actualizador | `apps/pdfcraft/src/updates.rs`, `crates/ui-egui/src/updates.rs` | releases privadas y autenticación |
| Paquetes Windows | `packaging/windows/`, `.github/workflows/release.yml` | icono, nombres de ejecutables, firma |
| Paquetes Linux | `packaging/linux/`, `.github/workflows/release.yml` | desktop entry, AppStream, iconos, MIME |
| Sincronizar con PdfCraft | `.github/workflows/sync-upstream.yml` | rama `upstream-main`, conflictos de marca |
| Añadir un idioma | `crates/ui-egui/src/i18n/` | catálogo y pruebas de localización |
| Versión | `Cargo.toml` mediante `cargo xtask version set X.Y.Z` | `Cargo.lock`, paquetes y release |

## 2. La regla mental más importante

Una función completa sigue normalmente esta ruta:

```text
Interfaz gráfica / CLI / MCP
              │
              ▼
        pdfcraft-engine
  sesión, permisos, Edit, undo/redo
              │
              ▼
 crate especializado (edit, forms, organize…)
              │
              ▼
 pdfcraft-cos: grafo de objetos del PDF
              │
              ▼
 guardado incremental o guardado completo
```

No conviene implementar una edición únicamente en un botón. El botón debe llamar al `engine`; el `engine` debe llamar al crate que sabe modificar el PDF. Así la operación obtiene deshacer/rehacer, estado “modificado”, permisos, CLI/MCP y pruebas coherentes.

El flujo de visualización es paralelo:

```text
bytes del PDF → cos/model → render → píxeles/estructura/texto → canvas de ui-egui
```

## 3. Capas y límites

| Capa | Crates | Responsabilidad |
|---|---|---|
| L0 | `geom`, `filters`, `crypt` | matemáticas, codecs y cifrado; no conocen la aplicación |
| L1 | `cos` | objetos PDF, parser, seguridad, escritura |
| L2 | `content`, `model`, `fonts` | operadores, páginas tipadas y fuentes |
| L3 | `render`, `annot`, `forms`, `js`, `xfa` | lectura/visualización y subsistemas interactivos |
| L4 | `edit`, `organize`, `redact`, `xfdf`, `sign`, `ocr`, `create`, `export`, `optimize`, `preflight`, `a11y`, `compare`, `measure`, `print`, `audit` | funciones de producto que alteran o analizan documentos |
| L6 | `engine` | fachada común, sesión, permisos, historial, catálogo |
| L7 | `platform`, `ui-egui`, `automation` | sistema operativo, interfaz y control headless |
| Apps | `apps/pdfcraft`, `apps/pdfcraft-cli`, `apps/pdfcraft-web` | ejecutables finales |
| Desarrollo | `xtask`, `oracle`, `testkit` | controles, corpus, herramientas y fixtures |

`cargo xtask layers` comprueba que no aparezcan dependencias hacia arriba. En particular, ningún crate por debajo de L7 puede depender de `egui`, `eframe`, `winit` o diálogos nativos.

## 4. Modelo de datos y guardado

El objeto principal es `pdfcraft_cos::Document`. Conserva los bytes originales y coloca los cambios en una superposición copy-on-write. Esto tiene cuatro consecuencias:

1. Clonar para crear un punto de deshacer es barato: los datos no modificados se comparten.
2. El guardado normal añade una revisión incremental y no reescribe lo anterior.
3. Los objetos desconocidos se conservan, algo esencial para no destruir partes del PDF que el programa aún no entiende.
4. Operaciones como redacción, conversión PDF/A, imposición o cambio de contraseña requieren guardado completo porque deben eliminar o reconstruir datos.

En `crates/engine/src/lib.rs`, `Session` posee los documentos abiertos. Cada `Document` del engine contiene el COS, historial, ruta, bytes de trabajo, versión, permisos y estado de guardado. `Edit` es el enum central de operaciones deshacibles. Al añadir una edición hay que:

1. añadir una variante a `Edit`;
2. manejarla en `Session::apply`;
3. implementar el trabajo en el crate especializado;
4. exponerla en automatización y en UI;
5. añadir pruebas del nivel bajo y de extremo a extremo.

## 5. Archivos de la raíz

| Archivo | Qué contiene | Cuándo tocarlo / efecto |
|---|---|---|
| `Cargo.toml` | workspace, versión, dependencias compartidas, perfiles y parches locales | al añadir crates/dependencias o cambiar versión; afecta toda la compilación |
| `Cargo.lock` | versiones exactas resueltas | se regenera con Cargo; no editar a mano |
| `README.md` | presentación pública, funciones y construcción | cuando cambia lo que ve un usuario |
| `ROADMAP.md` | enlace/visión general del plan | cambios de dirección del producto |
| `AGENTS.md` | normas obligatorias de contribución, activos y arquitectura | sólo si cambia la política del proyecto |
| `CLAUDE.md` | guía de desarrollo y gates del repositorio heredado | mantener compatible con upstream |
| `ATTRIBUTION.toml` | registro fuente de todos los activos y licencias | obligatorio al añadir/quitar iconos, fuentes, modelos o imágenes |
| `ATTRIBUTION.md` | informe generado para humanos | regenerar con `cargo xtask assets --write` |
| `NOTICE` | avisos legales distribuidos | si una dependencia/activo exige aviso |
| `LICENSE-MIT`, `LICENSE-APACHE` | doble licencia del código | normalmente no tocar |
| `deny.toml` | licencias, fuentes, duplicados y advisories aceptados | cambios de política/dependencias |
| `clippy.toml` | configuración de Clippy | reglas estáticas globales |
| `rustfmt.toml` | formato Rust | afecta a todo `cargo fmt` |
| `.gitignore` | artefactos no versionados (`target`, corpus, salidas…) | al crear nuevos artefactos generados |
| `.gitattributes` | tratamiento Git de archivos | finales de línea/binarios |

`.cargo/config.toml` define el alias `cargo xtask`, equivalente a `cargo run -p xtask --`. No contiene configuración secreta ni específica de una máquina.

`contributors/people.toml` guarda nombres opcionales aportados con consentimiento para la ventana de créditos. `contributors/contributors.json` es el inventario generado de actividad del upstream. No se deben inventar nombres reales ni añadir datos personales de otra persona sin su permiso; la UI consume estos datos desde `credits.rs`.

La raíz conserva nombres internos `pdfcraft-*` en paquetes, crates, variables y rutas. No son marca visible: mantenerlos reduce enormemente los conflictos cuando se trae código de `storytold/pdfcraft`.

## 6. Aplicaciones finales

### 6.1 `apps/pdfcraft`: escritorio

| Archivo | Función |
|---|---|
| `Cargo.toml` | dependencias del ejecutable de escritorio |
| `build.rs` | recursos incorporados y metadatos específicos de plataforma |
| `src/main.rs` | arranque, argumentos, creación de ventana, icono, rutas, instancia única y conexión con `PdfCraftApp` |
| `src/apple_events.rs` | apertura de documentos por eventos de macOS |
| `src/compositor_gpu.rs` | selección/diagnóstico del compositor y GPU |
| `src/decorations.rs` | decoración nativa de ventana según plataforma |
| `src/logging.rs` | archivos de log y manejo de errores/pánicos de nivel aplicación |
| `src/motion.rs` | preferencias de animación/movimiento del sistema |
| `src/reopen.rs` | reapertura y activación de la ventana |
| `src/single_instance.rs` | una sola instancia y entrega de archivos a la instancia existente |
| `src/text_scale.rs` | escala de texto obtenida del sistema |
| `src/updates.rs` | consulta HTTPS a la última release de `JUANJOIC005/Pdf`; valida URL/versión |
| `src/windows_manifest.rs` | manifiesto y recursos de Windows |

Los tests `winit_*` fijan arreglos locales de la dependencia vendorizada `winit`: IME, X11, Wayland, DPI, maximizado y entrada coreana. Si se actualiza `winit`, deben ejecutarse porque justifican el parche local.

### 6.2 `apps/pdfcraft-cli`: línea de comandos

`src/main.rs` implementa `info`, `render`, `text`, `edit`, `combine`, `extract`, `split`, `check`, `audit`, `tools`, `run`, `mcp` y `ui`. Las órdenes directas llaman al mismo engine/render; `run` y `mcp` comparten la tabla de `automation`. `check` aísla cada PDF en un proceso hijo con timeout para que un archivo hostil no derribe el barrido completo.

`src/batch.rs` analiza y ejecuta lotes de conversión, OCR y optimización. `build.rs` añade metadatos de versión/plataforma. Los archivos de `tests/` cubren cada comando y, por nombre, indican la superficie: argumentos raíz, salida estándar/error, edición, render, texto, split, MCP compacto, control UI y recurso de versión.

Para añadir una orden de bajo nivel reutilizable, es preferible crear primero una herramienta en `crates/automation`; para una sintaxis humana especial se añade después el adaptador CLI.

### 6.3 `apps/pdfcraft-web`: navegador

| Archivo | Función |
|---|---|
| `src/main.rs` | arranque WASM, canvas, carga por URL, aviso de cambios sin guardar y puente con iframe padre |
| `index.html` | contenedor HTML/canvas |
| `Trunk.toml` | configuración del empaquetador Trunk |
| `Cargo.toml` | dependencias WebAssembly |

Aunque el fork sólo publica Windows y Linux, este código heredado sigue compilándose en el gate WASM. No forma parte del flujo de release actual.

## 7. Núcleo: crate por crate

Cada `Cargo.toml` define las dependencias del crate; cada `README.md` presente contiene contrato y ejemplos de su API. `lib.rs` es siempre la puerta pública.

### 7.1 `crates/geom`

Primitivas geométricas L0. `lib.rs` define rectángulos PDF y rotaciones; `perspective.rs` calcula transformaciones/perspectiva usadas por corrección de cámara. Un error aquí afecta selección, recorte, anotaciones y render.

### 7.2 `crates/filters`

Decodifica y codifica streams PDF. `ascii85.rs`, `asciihex.rs`, `flate.rs`, `lzw.rs` y `runlength.rs` implementan filtros; `predictor.rs` maneja predictores de imagen; `lib.rs` encadena filtros y limita salidas. Tocar esto afecta la apertura de contenido, imágenes y fuentes. Probar límites, streams truncados y bombas de descompresión.

### 7.3 `crates/crypt`

Seguridad estándar PDF R2–R6. `aes_cbc.rs` y `rc4.rs` son primitivas; `lib.rs` implementa autenticación, claves, permisos, diccionario `/Encrypt` y filtros criptográficos. `tests.rs` cubre vectores y errores. Un cambio afecta apertura, guardado, permisos y contraseñas; requiere especial cuidado criptográfico.

### 7.4 `crates/cos`

Es el corazón estructural del PDF:

| Archivo | Responsabilidad |
|---|---|
| `bytes.rs` | almacenamiento/ventanas de bytes y acceso seguro |
| `object.rs` | `Object`, `Dict`, `Stream`, referencias y strings PDF |
| `parser.rs` | cabecera, objetos, xref clásico/stream, trailers y recuperación tolerante |
| `document.rs` | grafo, resolución de referencias, overlay copy-on-write, páginas/objetos |
| `security.rs` | integración entre COS y `crypt` |
| `writer.rs` | escritura incremental y completa, xref y serialización |
| `page_labels.rs` | lectura de etiquetas/numeración de páginas |
| `lib.rs` | API y errores públicos |

Si una modificación del parser parece arreglar un PDF concreto, hay que probar también archivos truncados, referencias cíclicas, objetos enormes, cifrado y round-trip. Nunca reconstruir todo el documento sólo para entender una clave.

### 7.5 `crates/content`

Parsea y serializa los operadores de streams de contenido. `lib.rs` contiene operadores, operandos, matrices y spans que preservan bytes; `tests.rs` fija round-trip y sintaxis. Lo consumen texto, edición, formularios, redacción, accesibilidad e impresión.

### 7.6 `crates/model`

`lib.rs` ofrece una vista tipada sobre COS: páginas, cajas, recursos y transformaciones entre espacio PDF (origen abajo-izquierda) y espacio de vista (origen arriba-izquierda). Es la frontera para evitar que cada función interprete páginas de forma distinta.

### 7.7 `crates/fonts`

| Archivo | Responsabilidad |
|---|---|
| `std14.rs`, `std14_metrics.rs` | fuentes PDF estándar y métricas |
| `encodings.rs` | WinAnsi, MacRoman y codificaciones PDF |
| `pdf.rs` | lectura de recursos/font dictionaries |
| `embedding.rs` | incrustación/subsets de fuentes |
| `shaping.rs`, `source_shaping.rs`, `script.rs` | shaping, escrituras y uso de fuentes originales |
| `fallback.rs` | elección alternativa cuando faltan glifos |
| `craft.rs` | integración opcional con el paquete externo de fuentes Craft |
| `lib.rs` | fachada pública |

Cambiar fuentes repercute en anchuras, saltos de línea, edición, formularios, OCR y exportación. Comprobar idiomas RTL e índicos, no sólo ASCII.

### 7.8 `crates/render`

| Archivo | Responsabilidad |
|---|---|
| `raster.rs` | `PageRenderer`, pool, solicitudes, tiles, escalas y límites |
| `raster/work_queue.rs` | cola/concurrencia de render |
| `pixels.rs` | búfer y conversiones de píxeles |
| `inspect.rs` | resumen del documento: páginas, campos, links, capas, adjuntos |
| `text.rs` | extracción geométrica de glifos y texto |
| `reading.rs` | colores/modo de lectura |
| `structure.rs` | estructura lógica/etiquetada |
| `lib.rs` | API estable vista por engine/UI |
| `lab_bradford_tests.rs` | conversión de color Lab/Bradford |

Rasteriza mediante el `hayro` vendorizado e inspecciona con COS y compatibilidad `lopdf`. Un cambio de renderer debe permanecer detrás de esta API para no contaminar el resto.

### 7.9 `crates/annot`

`lib.rs` lee/crea/edita anotaciones; `appearance.rs` genera apariencias visuales; `links.rs` trata anotaciones de enlace; `tests.rs` cubre preservación y geometría. Comentarios, firmas dibujadas, sellos y medidas dependen de él.

### 7.10 `crates/forms`

`lib.rs` modela AcroForm y valores. `author.rs` crea/elimina campos; `appearance.rs` regenera su aspecto; `actions.rs` lee acciones; `scripting.rs` integra eventos; `detect.rs` detecta candidatos; `af.rs` trata formato de archivos asociados. Los tests verifican que valores y claves desconocidas sobrevivan.

### 7.11 `crates/js`

Sandbox de JavaScript de Acrobat sobre Boa. `lib.rs` implementa `event`, documento, campos, `app`, `util`, `console`, límites y efectos devueltos al caller. `formcalc.rs` soporta FormCalc; `xfa.rs` y `xfa_session.rs` conectan scripts XFA; `tests.rs` comprueba API y confinamiento. No debe obtener acceso libre a red, disco ni temporizadores.

### 7.12 `crates/xfa`

| Archivo | Responsabilidad |
|---|---|
| `packets.rs`, `parse.rs`, `pdf.rs` | extraer paquetes XFA y parsearlos |
| `model.rs` | nodos y propiedades del formulario |
| `data.rs` | datasets y binding de datos |
| `layout.rs` | maquetación/paginación dinámica |
| `text.rs`, `image.rs` | contenido visual |
| `script.rs` | eventos y scripts conectados con `js` |
| `fixtures.rs`, `tests.rs` | documentos sintéticos y cobertura |
| `lib.rs` | API pública |

### 7.13 `crates/edit`

| Archivo | Responsabilidad |
|---|---|
| `lib.rs` | marcas de página: cabecera/pie, watermark, fondo, Bates |
| `text.rs`, `text_regions.rs` | sustituir/añadir texto y detectar regiones |
| `images.rs` | edición y movimiento de imágenes existentes |
| `objects.rs` | inventario/movimiento de objetos seleccionables |
| `added.rs` | objetos nuevos añadidos por Pdf |
| `flatten.rs` | aplanar contenido interactivo |
| `bidi.rs`, `indic.rs` | texto bidireccional e índico |
| `added/arabic_tests.rs`, `tests.rs` | escritura compleja y regresiones |

Es una de las zonas de mayor impacto: una operación debe preservar operadores, recursos, matrices, clipping y bytes de glifos siempre que sea posible.

### 7.14 `crates/organize`

`lib.rs` rota, mueve, inserta y elimina páginas, reconstruyendo de forma segura el árbol. `import.rs` combina/extrae/divide e importa recursos; `outline.rs` maneja marcadores; `labels.rs` numeración; `boxes.rs` Media/Crop/etc.; `dedupe.rs` deduplica recursos; `prune.rs` elimina inalcanzables; `view.rs` vista inicial; `xmp.rs` metadatos; `pdfx.rs` estructura PDF/X. `tests.rs` cubre operaciones y preservación.

### 7.15 `crates/redact`

Redacción irreversible. `lib.rs` coordina marcas y aplicación; `interp.rs` interpreta contenido; `codes.rs` relaciona códigos y texto; `image.rs` elimina/recorta píxeles; `patterns.rs` busca patrones sensibles; `sanitize.rs` borra metadatos, adjuntos, scripts y contenido oculto; `tags.rs` repara etiquetas; `tests.rs` verifica que lo tapado deje de existir, no sólo de verse. Requiere guardado completo.

### 7.16 `crates/audit`

`cover.rs` detecta rectángulos opacos colocados encima de texto todavía extraíble; `lib.rs` expone hallazgos; `tests.rs` fija falsos positivos/negativos. Es análisis, no redacción.

### 7.17 `crates/xfdf`

`lib.rs` importa/exporta datos de formulario y comentarios en XFDF; `tests.rs` cubre round-trip y XML. Afecta intercambio con otros lectores, no el render base.

### 7.18 `crates/sign`

| Archivos | Responsabilidad |
|---|---|
| `pdf.rs`, `pdf_discovery_tests.rs` | campos de firma, ByteRange, reserva e incrustación |
| `cms.rs`, `der.rs`, `x509.rs` | contenedor CMS/DER y certificados |
| `pkcs12.rs`, `keys.rs` | identidades/llaves importadas |
| `rsa_pad.rs`, `ec512.rs` | detalles criptográficos de firma |
| `timestamp.rs` | sellado de tiempo RFC 3161 |
| `revocation.rs`, `dss.rs` | OCSP/CRL y LTV/DSS |
| `graphic.rs` | apariencia visible |
| `keychain.rs`, `windows.rs` | almacenes de identidad del sistema |
| `lib.rs` | API coordinadora |

Firmar depende de bytes exactos. Todo cambio en writer, placeholder o ByteRange requiere tests de validación externa y de actualización incremental.

### 7.19 `crates/ocr`

`lib.rs` usa `ocrs`, recibe píxeles renderizados y genera una capa de texto invisible alineada sobre la imagen. Los modelos no están en Git: `cargo xtask models` los descarga y verifica según `ATTRIBUTION.toml`. Cambiar versión/modelo afecta tamaño de los paquetes y licencias.

### 7.20 `crates/create`

`lib.rs` crea documentos en blanco, desde imágenes o texto; `extract.rs` decodifica formatos fuente; `tests.rs` valida estructura y salida. Reutiliza COS, fuentes y model.

### 7.21 `crates/export`

`lib.rs` contiene exportaciones de datos/formatos; `pptx.rs` construye PowerPoint; `zip.rs` escribe el contenedor ZIP. Es deliberadamente independiente del COS: recibe datos ya preparados por engine.

### 7.22 `crates/optimize`

`images.rs` recomprime/reduce imágenes; `unused.rs` elimina objetos no usados; `links.rs` revisa enlaces; `audit.rs` calcula oportunidades/resultado; `lib.rs` coordina opciones. Una optimización nunca debe cambiar visualmente ni romper firmas sin comunicarlo.

### 7.23 `crates/preflight`

`lib.rs` comprueba/construye conformidad PDF/A y PDF/X; `icc.rs` gestiona perfiles de color; `xmp.rs` metadatos de estándares; `tests.rs` valida requisitos. Conversión suele implicar reescritura completa.

### 7.24 `crates/a11y`

`structure.rs` inspecciona árbol estructural; `tags.rs` etiquetas; `alt.rs` textos alternativos; `headings.rs` jerarquía; `content.rs` relación contenido-estructura; `autotag.rs` etiquetado automático; `report.rs` informe; `lib.rs` coordina reglas; `tests.rs` cubre resultados. No confundir “pasa reglas automáticas” con certificación de accesibilidad.

### 7.25 `crates/compare`

`lib.rs` implementa comparación de documentos/páginas y su modelo de diferencias. Engine prepara documentos y UI presenta resultados. Al ampliarlo, distinguir cambios visuales, textuales y estructurales.

### 7.26 `crates/measure`

`lib.rs` crea/lee medidas calibradas y exporta; `snap.rs` ajusta a geometría; `tests.rs` verifica escalas, unidades y anotaciones. Usa coordenadas mostradas, pero escribe correctamente en espacio PDF.

### 7.27 `crates/print`

`range.rs` analiza rangos; `lib.rs` prepara selección, escalado e imposición; `spool.rs` entrega al sistema; `tests.rs` fija orden y geometría. `engine/src/printcheck.rs` añade comprobaciones previas.

### 7.28 `crates/platform`

Servicios compartidos sin toolkit. `staging.rs` crea nombres temporales resistentes para guardado atómico; `desktop_theme.rs` detecta tema del sistema; `lib.rs` exporta. Cambiar staging afecta integridad de guardado tanto en UI como en automatización.

### 7.29 `crates/oracle` y `crates/testkit`

No entran en el producto. `oracle/lib.rs` localiza `qpdf`, `pdftoppm` y `pdftotext`; `pixels.rs` compara renderizados tolerando diferencias de borde. `testkit/lib.rs` construye PDFs sintéticos byte a byte. Si falta un oracle en CI con `PDFCRAFT_ORACLES_REQUIRED`, la prueba falla en lugar de aprobar silenciosamente.

## 8. `crates/engine`: centro de coordinación

| Archivo | Responsabilidad / impacto |
|---|---|
| `lib.rs` | `Session`, `Document`, `DocId`, enum `Edit`, apertura, aplicación, undo/redo, guardado y operaciones que crean documentos |
| `commands.rs` | registro único de ids, etiquetas, menú, atajos y precondiciones; alimenta menú, paleta, control y automatización |
| `catalog.rs` | catálogo “todas las herramientas”, grupos, iconos, color y estado Ready/Planned/Provider |
| `actions.rs` | ejecuta acciones PDF: navegación, URI, named, capas y JavaScript |
| `links.rs` | detectar, crear, modificar y abrir enlaces |
| `js.rs`, `xfa.rs` | puentes hacia scripts y formularios XFA |
| `ocr.rs` | adapta render/páginas al OCR y aplica capa textual |
| `compare.rs`, `export.rs` | fachadas de comparación y exportación |
| `optimizer.rs` | planes/opciones de optimización |
| `autotag.rs` | flujo de etiquetado automático |
| `camera.rs`, `camera_clean.rs`, `camera_edges.rs`, `scan_colours.rs` | corrección de documento fotografiado/escaneado |
| `copy_image.rs`, `upright_image.rs` | extracción/copia y orientación de imágenes |
| `signature_image.rs`, `signature_scan.rs` | preparación de imagen de firma y detección desde escaneo |
| `text_fonts.rs` | descubrir/importar recursos de fuente para editar texto |
| `text_preview.rs` | preview sin el bloque seleccionado, sin modificar documento |
| `dates.rs` | parseo/formato de fechas PDF |
| `printcheck.rs` | avisos antes de imprimir |
| `build_info.rs` | versión, compilación y datos mostrados en Acerca de/diagnóstico |
| `tests.rs` y `tests/*` | contratos de sesión y regresiones de fuentes, rotación, OCR, texto e imagen |

Una función que modifica un documento y no pasa por `Session::apply` probablemente perderá historial, dirty flag o paridad headless. Las operaciones que producen un documento nuevo —combine, split, create, export, print— son métodos de `Session`, pero no variantes `Edit` sobre el original.

## 9. `crates/automation`: CLI avanzada y MCP

| Archivo | Responsabilidad |
|---|---|
| `tools.rs` | lista de herramientas, descripción, JSON Schema, flags read-only/destructive y comando asociado |
| `lib.rs` | `Automation`, dispatch principal, validación común, sesión y confinamiento de rutas |
| `mcp.rs` | servidor JSON-RPC MCP por stdin/stdout, tools y resources |
| `text_cache.rs` | LRU de extracción: límites de memoria/páginas e invalidación por versión |
| `conventions.rs` | convenciones de argumentos, páginas y geometría |
| `batch.rs` | procesamiento de carpetas/lotes |
| `a11y.rs`, `comments.rs`, `content.rs`, `forms.rs`, `links.rs`, `measure.rs`, `printing.rs`, `reading.rs`, `redact.rs`, `signing.rs` | adaptadores del dominio correspondiente |
| `tests/automation.rs` | recorrido E2E y consistencia tabla/schema/comandos |
| `tests/batch.rs`, `form_bulk.rs`, `hostile_args.rs` | lotes, atomicidad y argumentos/rutas hostiles |

Para añadir una herramienta: capacidad en engine, definición en `tools.rs`, dispatch/adaptador, test E2E. Las páginas públicas son 1-based y las posiciones usan puntos desde arriba-izquierda. No aceptar argumentos desconocidos silenciosamente.

## 10. `crates/ui-egui`: mapa completo de interfaz

`lib.rs` define `PdfCraftApp`, estado global y ciclo de dibujo. La mayor parte de los archivos representa una pantalla o comportamiento muy concreto:

### Estructura, navegación y aspecto

| Archivos | Qué controlan |
|---|---|
| `chrome.rs`, `window_controls.rs` | marco, barra superior y controles de ventana |
| `docking.rs`, `panels.rs`, `tab_context.rs` | distribución, paneles, pestañas y contexto activo |
| `home.rs` | pantalla inicial/recientes |
| `canvas.rs` | documento visible, render, páginas y overlays |
| `theme.rs`, `appearance.rs` | colores, modo claro/oscuro y preferencias visuales |
| `ui_scale.rs`, `text_scale` en app | escalas de interfaz/texto |
| `icons.rs`, `icon_data.rs` | iconos internos y datos incrustados |
| `widgets.rs` | componentes egui reutilizables |
| `palette.rs`, `commands.rs` | paleta, menús, atajos y ejecución del registro engine |
| `dialogs.rs`, `pickers.rs`, `image_pick.rs` | diálogos y selección de archivos/imágenes |
| `motion.rs`, `autoscroll.rs`, `scroll_speed.rs`, `wheel_pager.rs`, `zoom_snap.rs` | animación, scroll, rueda y zoom |
| `units.rs`, `bidi.rs` | unidades y apoyo bidireccional |

### Archivos, estado y recuperación

| Archivos | Qué controlan |
|---|---|
| `files.rs` | abrir, guardar, guardar como y cierre |
| `open_worker.rs`, `open_worker_tests.rs` | apertura en segundo plano y entrega de resultado |
| `file_watch.rs` | cambios externos del archivo (no WASM) |
| `last_session.rs` | restauración de pestañas/sesión |
| `recovery.rs` | autosaves y recuperación tras cierre/fallo |
| `portable.rs` | rutas del modo portable en Windows |
| `system_fonts.rs` | descubrimiento de fuentes instaladas |
| `control.rs` | canal `ui.*` para controlar/capturar una app en ejecución |
| `updates.rs` | diálogo de versión y enlace a releases del fork |
| `credits.rs` | créditos, licencias y Acerca de |

### Herramientas de documento

| Archivos | Herramienta |
|---|---|
| `editing.rs`, `content_ui.rs`, `object_ui.rs` | selección/edición general y objetos |
| `edit_text_ui.rs` | editor de texto existente |
| `crop.rs`, `pageboxes.rs`, `page_selection_ui.rs` | recorte, cajas y selección de páginas |
| `combine_ui.rs`, `combine_grid.rs` | combinar documentos y miniaturas |
| `create_ui.rs`, `create_multiple_ui.rs` | crear PDF |
| `comments.rs`, `comments_panel.rs`, `comment_props.rs` | comentarios y propiedades |
| `forms_ui.rs`, `bulk_fields.rs` | formularios y edición masiva de campos |
| `fill_sign.rs`, `sign_ui.rs`, `signature_drag.rs` | rellenar/firmar, firma digital y arrastre gráfico |
| `redact_ui.rs` | marcar, aplicar, buscar y sanear |
| `ocr_ui.rs`, `camera_ui.rs` | OCR y corrección de cámara |
| `optimize_ui.rs` | compresión/optimización |
| `print_ui.rs`, `web_print.rs` | impresión nativa/web |
| `export_ui.rs` | exportaciones |
| `compare_ui.rs` | comparación |
| `measure_ui.rs` | medidas |
| `marks_ui.rs` | cabeceras, pies, Bates, fondos y marcas de agua |
| `stamps_ui.rs` | sellos |
| `link_ui.rs`, `actions_ui.rs`, `js_ui.rs` | enlaces, acciones y scripts |
| `a11y_ui.rs`, `tags_ui.rs` | accesibilidad y etiquetas |
| `standards_ui.rs`, `prepare.rs` | estándares/preflight y preparación final |
| `folders_ui.rs` | procesos sobre carpetas |
| `search_ui.rs` | búsqueda |
| `drag_pointer.rs` | semántica compartida de arrastre/puntero |

### Traducciones

`i18n/mod.rs` resuelve idioma y traducción; `i18n/catalog.rs` define/carga el catálogo; cada `.tsv` es un idioma (`es.tsv`, `fr.tsv`, `de.tsv`, etc.). Los literales UI usan `tl!` o `tl_ctx!`. Al añadir un literal se debe incorporar al catálogo y mantener las claves inglesas estables.

### Pruebas UI

`tests/*.rs` agrupa pruebas por función con nombres equivalentes a los módulos. `tests/ui.rs` y `tests/ui_group/main.rs` reúnen conjuntos grandes; `tests/ui_group/*.rs` separa escenarios específicos. `tests/support/mod.rs` son helpers y `tests/data/form.pdf` es fixture. `examples/shot.rs` genera capturas. Si se modifica un panel, buscar primero un test con el mismo nombre que el módulo.

## 11. Recursos y marca

### `assets/app-icon/`

Contiene las variantes fuente y exportadas del icono de la aplicación para Windows/Linux y otros targets heredados. El icono actual es original del fork; no debe sustituirse por el logotipo de Adobe, que es una marca/activo propietario. `packaging/icons.sh` genera formatos/tamaños derivados. Toda alta o sustitución debe reflejarse en `ATTRIBUTION.toml`.

### `assets/icons/`

Iconos funcionales de la interfaz. No cambiar nombres sin actualizar `crates/ui-egui/src/icons.rs` y cualquier referencia en catálogo/UI. Son 187 archivos: se tratan como una colección gobernada por el manifiesto de atribución, no como 187 módulos de lógica.

### `assets/fonts/`

Fuentes incorporadas y licencias. Afectan texto de interfaz o creación/edición según su uso. No confundirlas con fuentes del documento ni con el checkout privado `craft-fonts` usado en releases.

### `assets/demo/`

Material para documento/demo y capturas. No entra en el motor salvo que un build script lo incluya explícitamente.

### Cambiar la marca visible sin romper upstream

Los lugares propios del fork son:

- `README.md`, metadatos y URLs públicas;
- `assets/app-icon/` y su atribución;
- títulos/literales visibles de app/UI;
- `packaging/linux/` y `packaging/windows/`;
- `.github/workflows/release.yml`;
- endpoints en `apps/pdfcraft/src/updates.rs` y `crates/ui-egui/src/updates.rs`.

Conviene **no** renombrar crates `pdfcraft-*`, carpetas, símbolos `PdfCraftApp`, variables `PDFCRAFT_*` ni binarios internos de Cargo. El paquete puede copiar/renombrar el ejecutable final a `Pdf.exe`, `pdf` o `pdf-cli`. Esta frontera mantiene pequeña la capa de personalización y hace viable fusionar upstream.

## 12. Empaquetado

### Windows: `packaging/windows/`

| Archivo | Función |
|---|---|
| `package.ps1` | compila x64, prepara staging, copia `Pdf.exe`/`pdf-cli.exe`, modelos, licencias, MSI y portable ZIP |
| `pdfcraft.wxs` | producto WiX: archivos, registro, asociaciones, accesos directos, menús contextuales y upgrade |
| `installer-ui.wxs` | pantallas/opciones del instalador |
| `sign.ps1` | Authenticode para binarios/paquetes |
| `test-msi.ps1` | inspección automatizada de tablas y comportamiento MSI |
| `security-audit.ps1` | hashes, firma y diagnóstico de detecciones antivirus |
| `portable.txt` | activa almacenamiento junto al ejecutable |

Los nombres internos WiX como `PdfcraftExe` son identificadores, no texto visible; cambiarlos puede romper upgrades. Los GUID existentes deben permanecer estables salvo que se quiera deliberadamente un producto distinto incompatible con instalaciones anteriores.

### Linux: `packaging/linux/`

| Archivo | Función |
|---|---|
| `package.sh` | compila y produce AppImage, zsync, deb, rpm y tar.gz |
| `nfpm.yaml` | metadatos/archivos de deb y rpm |
| `io.github.juanjoic005.pdf.desktop` | nombre, comando, categorías y asociación de escritorio |
| `io.github.juanjoic005.pdf.metainfo.xml.in` | AppStream, descripción, URLs y releases |
| `io.github.juanjoic005.pdf.mime.xml` | MIME de integración |
| `postinst.sh` | actualización de cachés tras instalar |
| `flatpak-bundle.sh` | convierte tarball en bundle Flatpak |
| `flatpak/*.yml` | manifiestos de build/bundle Flatpak |

`packaging/env.sh` comparte versión/rutas, `packaging/icons.sh` fabrica iconos, `.shellcheckrc` configura lint. Las carpetas `macos/`, `web/` y `freebsd/` se conservan de upstream pero el workflow privado no publica esos sistemas.

## 13. GitHub Actions y actualización desde upstream

| Workflow | Función |
|---|---|
| `.github/workflows/ci.yml` | fmt, clippy, tests, capas, activos, CLI sin MCP, WASM, licencias y fuzz |
| `packaging-lint.yml` | lint de scripts/manifiestos de empaquetado |
| `release.yml` | release manual/tag únicamente Windows x64 y Linux x86_64; termina en draft de GitHub |
| `sync-upstream.yml` | obtiene `storytold/pdfcraft/main`, actualiza `upstream-main` y abre PR contra `main` |
| `windows-arm64.yml` | gate heredado de compatibilidad ARM64, no artefacto del release principal |
| `.github/actionlint.yaml` | configuración de actionlint |
| `.github/pull_request_template.md` | checklist de PR |

Flujo seguro de actualización:

```text
storytold/pdfcraft/main
        │ workflow programado/manual
        ▼
origin/upstream-main
        │ Pull Request + CI + resolución de conflictos
        ▼
origin/main (tu versión Pdf)
```

Git conserva automáticamente cambios independientes. Si upstream modifica la misma línea que la capa de marca, habrá conflicto y debe elegirse conscientemente. Nunca hacer `force push` de upstream sobre `main`; el force del workflow se limita a la rama espejo `upstream-main`.

## 14. Actualizaciones dentro de la app

Actualmente `apps/pdfcraft/src/updates.rs` consulta `https://api.github.com/repos/JUANJOIC005/Pdf/releases/latest`; `crates/ui-egui/src/updates.rs` compara versiones y abre la página privada de releases. La app **no descarga ni instala** automáticamente.

En un repositorio privado, una actualización totalmente automática necesitaría autenticación segura. Incrustar un token de GitHub en el ejecutable sería inseguro: cualquiera podría extraerlo. Las opciones sanas serían un servidor propio con artefactos firmados, releases públicas sólo para binarios o autenticación de usuario mediante dispositivo/OAuth y almacenamiento seguro. Además habría que verificar firma/hash, descargar atómicamente, gestionar rollback y coordinar instaladores Windows/AppImage. Por eso esta zona no debe “simplificarse” añadiendo un token fijo.

## 15. `xtask`: mantenimiento del repositorio

| Archivo | Función |
|---|---|
| `src/main.rs` | dispatch de todos los subcomandos |
| `gates.rs` | gates CI, WASM, corpus, check y oracles |
| `layers.rs` | grafo de capas permitido |
| `assets.rs` | valida atribuciones y descarga/verifica modelos |
| `version.rs` | lectura/cambio coherente de versión |
| `parity.rs` | valida matriz de funciones frente a comandos/tools/tests |
| `corpus.rs` | corpus pdf.js fijado por hash |
| `fuzz.rs` | mutación en procesos hijos y conservación de hallazgos |
| `screenshots.rs` | regenera capturas de README |
| `macos_integration.rs` | diagnóstico heredado de bundle/macOS |
| `demo_pdf/*` | construye demo: PDF, formularios, anotaciones, raster, Chrome y postproceso |
| `baselines/pdfjs.json` | regresiones conocidas del corpus |

Comandos esenciales:

```bash
cargo xtask layers
cargo xtask assets
cargo xtask parity
cargo xtask wasm
cargo xtask ci
cargo xtask version set 0.9.0
```

## 16. Documentación existente

| Archivo | Tema |
|---|---|
| `docs/architecture.md` | diseño y capas de referencia |
| `docs/development.md` | preparación y rutina de desarrollo |
| `docs/releasing.md` | proceso de release heredado; contrastar con workflow privado |
| `docs/roadmap.md`, `docs/gaps.md` | estado y huecos pendientes |
| `docs/editing-existing-text.md` | restricciones y flujo de texto existente |
| `docs/editing-images.md` | selección, hit testing y transformación de imágenes |
| `docs/editing-multiple-objects.md` | referencias generacionales y movimiento múltiple |
| `docs/cli-batch.md` | lotes CLI |
| `docs/mcp.md` | protocolo, herramientas y recursos MCP |
| `docs/localization.md`, `localization-parity.md` | traducciones y cobertura |
| `docs/file-format-parity.md`, `pdf-spec-parity.md`, `target-app-parity.md`, `ui-parity.md`, `hardware-parity.md` | matrices de compatibilidad |
| `docs/contributors.md` | créditos de contribución |
| `docs/windows-renderer-startup.md` | diagnóstico de renderer al arrancar Windows |
| `docs/images/*` | capturas usadas por README/documentación |

`parity/acrobat-features.toml` es la matriz estructurada de funciones objetivo. Si se declara terminada una función, actualizar registro, herramienta/prueba y paridad juntos.

## 17. Dependencias vendorizadas: cuándo NO tocar

`vendor/` contiene copias parcheadas de proyectos externos seleccionadas en `[patch.crates-io]` del `Cargo.toml` raíz:

- `hayro`, `hayro-interpret`, `hayro-syntax`, `hayro-jbig2`: interpretación/render PDF y JBIG2;
- `lopdf`: compatibilidad de lectura/manipulación;
- `winit`: ventanas y entrada/IME;
- `egui-wgpu`: backend gráfico egui.

Sus muchos archivos son código de terceros, no módulos de negocio de Pdf. Se conservan con su estructura y README originales. Sólo deben editarse cuando:

1. el fallo está demostrado dentro de la dependencia;
2. existe una prueba de regresión local;
3. el parche es mínimo y documentable;
4. se contempla enviarlo upstream o retirarlo al actualizar.

No hacer rebrand dentro de `vendor/`. Al actualizar una dependencia vendorizada, comparar el fork, reaplicar parches deliberadamente y ejecutar los tests específicos (por ejemplo los `winit_*`).

## 18. Tests: cómo encontrar el que corresponde

Hay tres niveles:

1. `src/tests.rs` o `#[cfg(test)]`: unidad junto a la implementación.
2. `crates/<nombre>/tests/*.rs`: integración pública del crate.
3. tests de `ui-egui`, `automation` y apps: extremo a extremo de la superficie del usuario.

Regla práctica:

| Cambio | Pruebas mínimas |
|---|---|
| algoritmo interno | unidad del crate + integración del crate |
| nueva variante `Edit` | crate + engine undo/redo/save |
| nuevo botón | engine + test UI con nombre de herramienta |
| nueva tool MCP | engine + `automation` schema/dispatch/E2E |
| parser/writer | round-trip, PDF hostil, cifrado y qpdf si aplica |
| render | fixture sintético + oracle/píxeles si aplica |
| paquete | scripts/lint + inspección MSI o metadatos Linux |
| marca/icono | `cargo xtask assets` + packaging lint |

Para un cambio acotado no es necesario recompilar 25 GB siempre. Ejecutar primero:

```bash
cargo fmt --all -- --check
cargo test -p pdfcraft-<crate-afectado>
cargo clippy -p pdfcraft-<crate-afectado> --all-targets -- -D warnings
cargo xtask layers
```

Antes de fusionar, CI cubre el workspace completo.

## 19. Recetas de modificación

### Añadir un botón que aplica una mejora al PDF

1. Implementar función pura/mutación en el crate L4 adecuado.
2. Añadir variante a `Edit` y branch en engine.
3. Registrar comando en `engine/commands.rs` si debe tener menú/atajo/paleta.
4. Añadir tool en automation para mantener paridad headless.
5. Crear estado/diálogo en el módulo UI correspondiente y llamar al engine.
6. Añadir traducciones.
7. Probar operación, undo, redo, save/reopen y permisos.

### Añadir una herramienta sólo de análisis

Crear una API read-only en el crate adecuado, fachada en engine, tool marcada `read_only` y panel UI. No usar `Edit` si no cambia el documento.

### Cambiar el icono o nombre

Modificar activos fuente, regenerar derivados, actualizar `ATTRIBUTION.toml`, app/packaging/metadatos y ejecutar assets/packaging lint. No tocar nombres internos masivamente. Nunca usar iconos de Adobe sin licencia expresa.

### Añadir formato de exportación

Código del formato en `crates/export`, preparación de datos en engine, tool en automation, UI en `export_ui.rs`, extensión/filtro de diálogo y tests de estructura del archivo generado.

### Añadir soporte a una parte del estándar PDF

Decidir primero la capa: sintaxis/objetos en COS, vista tipada en model, operadores en content, visualización en render o función en L3/L4. Preservar claves desconocidas y añadir fixture mínimo construido con `testkit`.

## 20. Riesgos que hay que vigilar

- **Pérdida silenciosa de datos:** reconstruir diccionarios eliminando claves desconocidas.
- **Coordenadas incorrectas:** mezclar origen superior de UI con origen inferior del PDF o ignorar `/Rotate`.
- **Firmas invalidadas:** reescribir bytes en vez de guardar incrementalmente.
- **Redacción falsa:** pintar negro sin eliminar texto/imágenes subyacentes.
- **Fuentes rotas:** cambiar texto sin disponer de glifos/anchuras correctas.
- **OOM/DoS:** confiar en longitudes, dimensiones, recursión o streams de un PDF no fiable.
- **Path traversal:** aceptar `..`, symlinks o rutas de otro dispositivo en automation.
- **Token filtrado:** incrustar credenciales del repo privado en el binario.
- **Conflictos upstream innecesarios:** renombrar masivamente símbolos internos sólo por estética.
- **UI sin paridad:** función disponible sólo con clic y no desde engine/automation.

El proyecto prohíbe `unsafe` y rutas de producción con `unwrap`, `expect`, `panic!`, `todo!` o `unreachable!`. Los límites derivados de entrada deben usar aritmética comprobada.

## 21. Inventario por convención

Para que ningún archivo quede ambiguo:

- Todo `Cargo.toml` describe paquete, features y dependencias del directorio.
- Todo `build.rs` se ejecuta durante compilación y genera/incorpora recursos de plataforma.
- Todo `src/lib.rs` define la API pública del crate.
- Todo `src/main.rs` es la entrada de un ejecutable.
- Todo `src/tests.rs`, `tests/*.rs` o `*_tests.rs` es prueba, no código enviado directamente al usuario.
- Todo `README.md` dentro de un crate es el contrato humano de esa API.
- Todo `.tsv` de `i18n` es un catálogo de traducción.
- Todo archivo de `docs/images/` es captura documental gobernada por atribuciones.
- Todo archivo de `assets/` es un recurso binario/vectorial gobernado por `ATTRIBUTION.toml`.
- Todo archivo de `vendor/` pertenece a una dependencia parcheada y debe tratarse como upstream externo.
- Todo lo de `target/`, `dist/`, `corpus/`, `fuzz-out/` o modelos descargados es generado y no es fuente principal.

Para ver el inventario exacto versionado en cualquier momento:

```bash
git ls-files
git ls-files 'crates/*/src/**'
git ls-files 'crates/ui-egui/tests/**'
git ls-files 'vendor/**'
```

Esta combinación —mapa individual de todos los módulos propios más las convenciones para assets, tests y vendor— cubre los 1.341 archivos que tenía el repositorio al iniciar el recorrido (1.342 contando esta nueva guía), sin convertirla en una repetición inútil de cientos de PNG, fixtures y fuentes externas.

## 22. Checklist antes de tocar algo

1. ¿Es presentación, coordinación o lógica PDF?
2. ¿Existe ya un crate responsable?
3. ¿El cambio preserva datos desconocidos?
4. ¿Necesita `Edit` y undo/redo?
5. ¿Respeta permisos del documento?
6. ¿Debe existir también como tool headless?
7. ¿Qué coordenadas utiliza?
8. ¿Necesita guardado incremental o completo?
9. ¿Añade un activo/licencia?
10. ¿Afecta Windows, Linux o ambos?
11. ¿Crea conflicto probable con upstream?
12. ¿Qué prueba demostraría que funciona después de guardar y volver a abrir?

## 23. Ruta recomendada para aprender el proyecto

Leer en este orden evita perderse:

1. `docs/architecture.md`.
2. `crates/cos/README.md` y `src/lib.rs`.
3. `crates/engine/README.md`, luego `engine/src/lib.rs` y `commands.rs`.
4. `crates/ui-egui/src/lib.rs`, `canvas.rs`, `files.rs` y el módulo de la herramienta que interese.
5. `crates/automation/README.md`, `tools.rs` y el adaptador correspondiente.
6. El README/lib del crate especializado que se quiera modificar.
7. Sus tests y el test UI/automation equivalente.
8. Packaging y workflows sólo cuando la función ya esté terminada.

En una frase: **COS conserva el documento, los crates especializados saben modificarlo, engine coordina y protege la operación, UI/automation la presentan, y packaging la convierte en producto.**
