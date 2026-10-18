# Pdf app icon

The Pdf icon is an original red rounded-square document mark created for this private fork. It is
deliberately distinct from Adobe Acrobat, Adobe PDF, ArtCraft, PdfCraft and other existing brands.

`pdf-icon-source.png` is the generated source artwork. `pdfcraft-1024.png`, `pdfcraft.ico` and the
PNG files under `hicolor/` are derived build assets. Their internal filenames stay unchanged for
now to reduce conflicts when upstream code is merged; they are not the product name shown to
users.

Windows and Linux are the only current release targets. Run `packaging/icons.sh` after replacing
the source artwork, then update its hashes in `ATTRIBUTION.toml` and run
`cargo xtask assets --write`.
