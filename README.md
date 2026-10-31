# Pdf

Pdf is a public, Rust-native PDF workbench based on the open-source
[PdfCraft](https://github.com/storytold/pdfcraft) project.

The product name, links and release pipeline are maintained independently. Internal Rust crate
names and executable names remain `pdfcraft` for now so updates from the upstream project can be
merged with fewer conflicts.

## Current status

This project is synchronized with PdfCraft 0.16.1 upstream, which is alpha software. Keep backups of important files
and verify exported documents before using them in production.

## Build

Install a current Rust toolchain, then run:

```sh
cargo run --release -p pdfcraft -- document.pdf
```

The headless command-line application is available as `pdf-cli` (the internal Cargo package remains `pdfcraft-cli` to simplify upstream merges).

## Upstream updates

- `main` contains the independently branded product.
- `upstream-main` mirrors `storytold/pdfcraft:main`.
- `.github/workflows/sync-upstream.yml` refreshes `upstream-main` daily and opens an update pull
  request when upstream changes.
- Upstream update pull requests must pass CI and be reviewed before merging.

The original repository remains configured locally as the `upstream` Git remote.

## Releases and updates

Release builds currently target **Linux x86_64** only. The release workflow creates a `.deb`,
uploads it as a workflow artifact and publishes it in a GitHub Release.

The application points its update page to
<https://github.com/JUANJOIC005/Pdf/releases>.

## Licensing and attribution

The inherited source is available under the MIT License or Apache License 2.0, at your option.
Keep `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` and `ATTRIBUTION.md` with redistributed builds.

Copyright (c) 2026 the original project contributors and subsequent Pdf contributors.
