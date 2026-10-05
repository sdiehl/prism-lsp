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

Emacs 29 or later, with the built-in eglot client. `editors/emacs/prism-mode.el` provides highlighting and indentation and registers the server with eglot:

```elisp
(add-to-list 'load-path "~/Git/prism-lsp/editors/emacs")
(require 'prism-mode)
(add-hook 'prism-mode-hook #'eglot-ensure)
```

or with `use-package`:

```elisp
(use-package prism-mode
  :load-path "~/Git/prism-lsp/editors/emacs"
  :mode "\\.pr\\'"
  :hook ((prism-mode . eglot-ensure)
         (prism-mode . (lambda () (add-hook 'before-save-hook #'eglot-format-buffer nil t)))))
```

`M-.` goes to a definition, `M-?` finds references, `C-h .` shows the type at point, and `M-x eglot-format-buffer` formats.

## VS Code

```sh
cd editors/vscode
npm install
npm run compile
npx vsce package
code --install-extension prism-lsp-0.1.0.vsix
```

The extension starts `prism-lsp` from `PATH`. Set `prism.server.path` to use another binary.
