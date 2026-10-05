;;; prism-mode.el --- Major mode for the Prism language -*- lexical-binding: t; -*-

;; Author: Stephen Diehl
;; Version: 0.1.0
;; Package-Requires: ((emacs "29.1"))
;; Keywords: languages
;; URL: https://github.com/sdiehl/prism-lsp

;;; Commentary:

;; Syntax highlighting and indentation for Prism, and registration of
;; `prism-lsp' with eglot when eglot loads.

;;; Code:

(defgroup prism nil
  "Prism language support."
  :group 'languages)

(defcustom prism-indent-offset 2
  "Indentation step for Prism code."
  :type 'integer)

(defconst prism-keywords
  '("fn" "fip" "fbip" "replayable" "logic" "requires" "ensures" "test" "total"
    "assume" "decreases" "path" "deprecated" "pub" "import" "as" "type" "newtype"
    "stable" "upgrade" "downgrade" "opaque" "effect" "error" "throw" "try" "catch"
    "transact" "probe" "reflect" "alias" "class" "instance" "canonical" "pattern"
    "view" "make" "deriving" "where" "given" "handle" "with" "handler" "partial"
    "mask" "val" "return" "resume" "let" "var" "borrow" "in" "for" "while" "loop"
    "break" "continue" "do" "if" "then" "else" "elif" "match" "of" "each" "forall"
    "using"))

(defconst prism-font-lock-keywords
  `((,(regexp-opt prism-keywords 'symbols) . font-lock-keyword-face)
    (,(regexp-opt '("true" "false") 'symbols) . font-lock-constant-face)
    ("\\_<fn\\s-+\\([a-z_][A-Za-z0-9_']*\\)" 1 font-lock-function-name-face)
    ("'\\(?:\\\\.\\|[^'\\\\]\\)'" . font-lock-string-face)
    ("\\_<[A-Z][A-Za-z0-9_]*\\_>" . font-lock-type-face)
    ("\\_<[0-9][0-9_]*\\(?:\\.[0-9_]+\\)?\\(?:i64\\|u64\\)?\\_>" . font-lock-constant-face)))

(defvar prism-mode-syntax-table
  (let ((st (make-syntax-table)))
    (modify-syntax-entry ?- ". 12" st)
    (modify-syntax-entry ?\n ">" st)
    (modify-syntax-entry ?_ "_" st)
    (modify-syntax-entry ?' "_" st)
    (modify-syntax-entry ?\" "\"" st)
    (modify-syntax-entry ?\\ "\\" st)
    st))

(defconst prism--opener
  "\\(?:=\\|=>\\|->\\|\\_<\\(?:of\\|then\\|else\\|do\\|with\\|in\\)\\)\\s-*\\(?:--.*\\)?$"
  "A line ending in one of these opens an indented block.")

(defun prism-indent-line ()
  "Indent like the previous code line, one step deeper after a block opener."
  (interactive)
  (let ((target
         (save-excursion
           (beginning-of-line)
           (if (not (re-search-backward "^\\s-*[^ \t\n]" nil t))
               0
             (+ (current-indentation)
                (if (looking-at (concat ".*" prism--opener)) prism-indent-offset 0))))))
    (if (<= (current-column) (current-indentation))
        (indent-line-to target)
      (save-excursion (indent-line-to target)))))

;;;###autoload
(define-derived-mode prism-mode prog-mode "Prism"
  "Major mode for editing Prism code."
  (setq-local comment-start "-- ")
  (setq-local comment-start-skip "--+\\s-*")
  (setq-local font-lock-defaults '(prism-font-lock-keywords))
  (setq-local indent-line-function #'prism-indent-line)
  (setq-local indent-tabs-mode nil)
  (setq-local tab-width prism-indent-offset))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.pr\\'" . prism-mode))

(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs '(prism-mode . ("prism-lsp"))))

(provide 'prism-mode)
;;; prism-mode.el ends here
