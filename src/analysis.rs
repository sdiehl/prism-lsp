use std::collections::HashMap;
use std::path::PathBuf;

use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString, Url,
};
use prism::error::SourceMap;
use prism::{Config, Error, SearchPath, TypeError, TypeSpan};

use crate::lines::Lines;
use crate::locate::Files;

const SOURCE: &str = "prism";

/// What the compiler says about one version of one document, in its own coordinates.
#[derive(Default)]
pub struct Analysis {
    pub version: i32,
    pub diagnostics: Vec<Diagnostic>,
    pub types: Vec<TypeSpan>,
    pub refs: Vec<Ref>,
    pub defs: HashMap<String, Def>,
}

/// A resolved reference: `text[start..end]` means `target`. A local carries the
/// offset of its binder, and its target is only the name as written.
#[derive(Clone, Debug)]
pub struct Ref {
    pub start: usize,
    pub end: usize,
    pub target: String,
    pub local: Option<usize>,
}

/// Where a name is written: in `file`, or in the document itself when `None`.
#[derive(Clone, Debug)]
pub struct Def {
    pub file: Option<PathBuf>,
    pub start: usize,
    pub end: usize,
}

impl Analysis {
    /// The innermost typed span covering `offset`.
    pub fn type_at(&self, offset: usize) -> Option<&TypeSpan> {
        self.types
            .iter()
            .filter(|s| s.start <= offset && offset < s.end)
            .min_by_key(|s| s.end - s.start)
    }

    pub fn ref_at(&self, offset: usize) -> Option<&Ref> {
        self.refs
            .iter()
            .find(|r| r.start <= offset && offset <= r.end)
    }

    /// Where a top-level name is defined.
    pub fn def(&self, target: &str) -> Option<&Def> {
        self.defs.get(target)
    }
}

pub fn analyze(
    uri: &Url,
    version: i32,
    text: &str,
    search: &SearchPath,
    files: &Files,
) -> Analysis {
    let full = search.with_prelude(text);
    let off = SourceMap::new(&full).prelude_len();
    let lines = Lines::new(text);
    let user =
        |start: usize, end: usize| (start >= off).then(|| (start - off, end.max(start) - off));

    let mut out = Analysis {
        version,
        ..Analysis::default()
    };
    let a = match prism::analyze(&full, &search.roots, &Config::default()) {
        Ok(a) => a,
        Err(e) => {
            out.diagnostics
                .push(error_diagnostic(uri, text, &lines, off, &e, search, files));
            return out;
        }
    };
    for w in &a.checked.reports.warnings {
        if let Some((s, e)) = user(w.span.start, w.span.end) {
            out.diagnostics.push(diagnostic(
                &lines,
                s,
                e,
                DiagnosticSeverity::WARNING,
                w.msg.clone(),
            ));
        }
    }
    out.types = a.typespans.spans;
    out.refs = a
        .occurrences
        .refs
        .into_iter()
        .filter(|r| r.module.is_empty())
        .filter_map(|r| {
            let (start, end) = user(r.start, r.end)?;
            Some(Ref {
                start,
                end,
                target: r.target,
                local: r.local.map(|b| b.saturating_sub(off)),
            })
        })
        .collect();
    let prelude = files.prelude(search);
    for d in a.occurrences.defs {
        let def = if !d.module.is_empty() {
            files.module(&d.module, &search.roots).map(|f| Def {
                file: Some(f),
                start: d.start,
                end: d.end,
            })
        } else if let Some((start, end)) = user(d.start, d.end) {
            Some(Def {
                file: None,
                start,
                end,
            })
        } else {
            prelude.clone().map(|f| Def {
                file: Some(f),
                start: d.start,
                end: d.end,
            })
        };
        if let Some(def) = def {
            out.defs.entry(d.name).or_insert(def);
        }
    }
    out
}

fn error_diagnostic(
    uri: &Url,
    text: &str,
    lines: &Lines,
    off: usize,
    e: &Error,
    search: &SearchPath,
    files: &Files,
) -> Diagnostic {
    let mut message = format!("{}: {e}", e.kind());
    let mut related = Vec::new();
    // An error inside an imported module indexes that module's source: point
    // at its import here and link the place in the module.
    let (start, end, place) = match e.origin() {
        Some(origin) => {
            let at = import_of(text, &origin.module).unwrap_or((0, 0));
            let file = files.module(&origin.module, &search.roots);
            let source = origin
                .source
                .clone()
                .or_else(|| std::fs::read_to_string(file.as_ref()?).ok());
            let place = file
                .zip(source)
                .and_then(|(f, s)| Some((Url::from_file_path(f).ok()?, s)));
            message = format!("in module {}: {message}", origin.module);
            (at.0, at.1, place)
        }
        None => {
            let span = e
                .primary_span()
                .filter(|s| s.start >= off)
                .map(|s| (s.start - off, s.end - off));
            let (s, e) = match span {
                Some((s, e)) if e > s => (s, e),
                Some((s, _)) => (s, s + 1),
                None => (0, 0),
            };
            (s, e, None)
        }
    };
    let module_loc = |span: std::ops::Range<usize>| {
        let (url, source) = place.as_ref()?;
        Some(Location::new(
            url.clone(),
            Lines::new(source).range(span.start, span.end),
        ))
    };
    if let Some(loc) = e.primary_span().and_then(module_loc) {
        related.push(DiagnosticRelatedInformation {
            location: loc,
            message: e.to_string(),
        });
    }
    if let Error::Type(TypeError::Kind(diag)) = e {
        for note in &diag.notes {
            message.push_str(&format!("\nnote: {note}"));
        }
        if let Some(help) = &diag.help {
            message.push_str(&format!("\nhelp: {help}"));
        }
        for (span, msg) in &diag.labels {
            let location = if place.is_some() {
                module_loc(span.start..span.end)
            } else {
                (span.start >= off).then(|| {
                    Location::new(uri.clone(), lines.range(span.start - off, span.end - off))
                })
            };
            if let Some(location) = location {
                related.push(DiagnosticRelatedInformation {
                    location,
                    message: msg.clone(),
                });
            }
        }
    }
    Diagnostic {
        code: Some(NumberOrString::String(e.code().as_str().to_string())),
        related_information: (!related.is_empty()).then_some(related),
        ..diagnostic(lines, start, end, DiagnosticSeverity::ERROR, message)
    }
}

// The `import` line naming `module`, as a range in `text`.
fn import_of(text: &str, module: &str) -> Option<(usize, usize)> {
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end();
        let named = body
            .strip_prefix("import ")
            .map(|rest| rest.split_whitespace().next() == Some(module));
        if named == Some(true) {
            return Some((at, at + body.len()));
        }
        at += line.len();
    }
    None
}

fn diagnostic(
    lines: &Lines,
    start: usize,
    end: usize,
    severity: DiagnosticSeverity,
    message: String,
) -> Diagnostic {
    Diagnostic {
        range: lines.range(start, end),
        severity: Some(severity),
        source: Some(SOURCE.to_string()),
        message,
        ..Diagnostic::default()
    }
}
