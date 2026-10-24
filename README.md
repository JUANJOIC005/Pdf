# Pdf

Pdf is a private, Rust-native PDF workbench based on the open-source
[PdfCraft](https://github.com/storytold/pdfcraft) project.

The product name, links and release pipeline are maintained independently. Internal Rust crate
names and executable names remain `pdfcraft` for now so updates from the upstream project can be
merged with fewer conflicts.

## Current status

This project is synchronized with PdfCraft 0.13.0 upstream, which is alpha software. Keep backups of important files
and verify exported documents before using them in production.

## Build

Install a current Rust toolchain, then run:

```sh
cargo run --release -p pdfcraft -- document.pdf
```

The headless command-line application is available as `pdf-cli` (the internal Cargo package remains `pdfcraft-cli` to simplify upstream merges).

## Upstream updates

- `main` contains the private branded product.
- `upstream-main` mirrors `storytold/pdfcraft:main`.
- `.github/workflows/sync-upstream.yml` refreshes `upstream-main` daily and opens an update pull
  request when upstream changes.
- Upstream update pull requests must pass CI and be reviewed before merging.

The original repository remains configured locally as the `upstream` Git remote.

## Releases and updates

Release builds currently target **Windows x64** and **Linux x86_64** only. The release workflow
creates those packages and opens a draft GitHub release; it does not build macOS, web or FreeBSD
packages.

Releases belong to this private repository. The application points its update page to
<https://github.com/JUANJOIC005/Pdf/releases>.

Because the repository is private, GitHub does not expose release metadata or downloads to an
unauthenticated application. Never embed a personal access token in the binary. A future automatic
installer must use an authenticated helper or a separately hosted signed update feed. Until then,
Help → Check for updates opens the private Releases page for an authenticated repository owner.

## Licensing and attribution

The inherited source is available under the MIT License or Apache License 2.0, at your option.
Keep `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` and `ATTRIBUTION.md` with redistributed builds.

Copyright (c) 2026 ArtCraft Team, the PdfCraft contributors and subsequent Pdf contributors.
