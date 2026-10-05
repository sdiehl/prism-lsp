# prism-lsp

A language server for [Prism](https://github.com/sdiehl/prism). It links the Prism compiler as a library and answers from the same checker that `prism check` runs.

- **Diagnostics** with error codes, notes, help, and related labels, refreshed as you type.
- **Hover** shows the inferred type of any expression, binder, or pattern variable, plus the `-- |` doc comment of the name under the cursor.
- **Go to definition** for top-level names, constructors, effect operations, and class methods: in the current file, in project modules, and in the prelude and standard library.
- **Find references** to top-level names within a file.
- **Document symbols** with constructors and operations nested under their declarations.
- **Formatting** with the canonical `prism fmt` layout.

Projects are found by walking up to the nearest `prism.toml`, and its `src` directory and path dependencies join the module search path. A file outside a project resolves imports against its own directory.

## Install

Prebuilt binaries for macOS arm64 and Linux x64/arm64, and the VS Code extension, are attached to each [GitHub release](https://github.com/sdiehl/prism-lsp/releases). Put `prism-lsp` on your `PATH`. To build from source:

```sh
cargo install --git https://github.com/sdiehl/prism-lsp
```

or from a checkout, `cargo install --path .`. The compiler is a git dependency pinned to a Prism release, so the server agrees with that release of `prism`.

## Neovim

Neovim 0.11 or later:

```lua
vim.filetype.add({ extension = { pr = "prism" } })

vim.lsp.config("prism", {
  cmd = { "prism-lsp" },
  filetypes = { "prism" },
  root_markers = { "prism.toml", ".git" },
})
vim.lsp.enable("prism")
```

Formatting on save:

```lua
vim.api.nvim_create_autocmd("BufWritePre", {
  pattern = "*.pr",
  callback = function() vim.lsp.buf.format() end,
})
```

Syntax highlighting comes from `scripts/nvim` in the Prism repository.

## Emacs

`prism-mode` (Emacs 29 or later) provides highlighting and indentation and registers the server with eglot, the built-in LSP client. Emacs 30, with `use-package`:

```elisp
(use-package prism-mode
  :vc (:url "https://github.com/sdiehl/prism-lsp" :lisp-dir "editors/emacs")
  :hook (prism-mode . eglot-ensure))
```

Emacs 29:

```elisp
(package-vc-install
 '(prism-mode :url "https://github.com/sdiehl/prism-lsp" :lisp-dir "editors/emacs"))
(add-hook 'prism-mode-hook #'eglot-ensure)
```

To format on save, add `eglot-format-buffer` to `before-save-hook` in Prism buffers:

```elisp
(add-hook 'prism-mode-hook
          (lambda () (add-hook 'before-save-hook #'eglot-format-buffer nil t)))
```

## VS Code

Download `prism-lsp.vsix` from the latest release and install it:

```sh
code --install-extension prism-lsp.vsix
```

To build it from source:

```sh
git clone https://github.com/sdiehl/prism-lsp
cd prism-lsp/editors/vscode
npm install
npm run compile
npx vsce package -o prism-lsp.vsix
```

The extension starts `prism-lsp` from `PATH`. Set `prism.server.path` to use another binary.
