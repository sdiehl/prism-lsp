# Changelog

All notable changes to this project are documented in this file.

## [0.1.0] - 2026-10-05

### Added

- A language server for Prism, built against Prism v0.23.0 as a library, so it reports what `prism check` reports.
- Diagnostics with error codes, notes, help, and related labels, rechecked as the document changes.
- Hover shows the inferred type of an expression, binder, or pattern variable, and the doc comment of the name under the cursor.
- Go to definition for top-level names, constructors, effect operations, and class methods, in the file, project modules, the prelude, and the standard library.
- Same-file references, document symbols with members nested under their declarations, and `prism fmt` formatting.
- Project discovery from `prism.toml`, with its `src` directory and path dependencies on the module search path.
- A VS Code extension with a TextMate grammar, an Emacs major mode registered with eglot, and a Neovim configuration.
- Release binaries for macOS arm64 and Linux x64/arm64, and the VS Code extension, attached to each GitHub release.
