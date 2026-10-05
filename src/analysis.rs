use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString, Url,
};
use prism::error::SourceMap;
use prism::index::occurrences::extract;
use prism::{Config, Error, Root, TypeError, TypeSpan, TypeSpans};

use crate::lines::Lines;

const SOURCE: &str = "prism";

/// What the compiler says about one version of one document, in its own coordinates.
#[derive(Default)]
pub struct Analysis {
    pub version: i32,
    pub diagnostics: Vec<Diagnostic>,
    pub types: Vec<TypeSpan>,
    pub refs: Vec<Ref>,
}

/// A resolved reference to a top-level name: `text[start..end]` means `target`.
#[derive(Clone, Debug)]
pub struct Ref {
    pub start: usize,
    pub end: usize,
    pub target: String,
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
}

pub fn analyze(uri: &Url, version: i32, text: &str, roots: &[Root]) -> Analysis {
    let full = prism::driver::with_prelude(text);
    let off = SourceMap::new(&full).prelude_len();
    let lines = Lines::new(text);
    let cfg = Config::default();
    let user =
        |start: usize, end: usize| (start >= off).then(|| (start - off, end.max(start) - off));

    let mut out = Analysis {
        version,
        ..Analysis::default()
    };
    match prism::check_validated_on_in(&full, roots, &cfg) {
        Ok(checked) => {
            for w in &checked.reports.warnings {
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
            if let Ok(json) = prism::dump_on("typespans", &full, roots, &cfg) {
                out.types = TypeSpans::from_json(&json)
                    .map(|t| t.spans)
                    .unwrap_or_default();
            }
        }
        Err(e) => out.diagnostics.push(error_diagnostic(uri, &lines, off, &e)),
    }
    if let Ok(occ) = extract(&full, roots) {
        out.refs = occ
            .refs
            .into_iter()
            .filter(|r| r.module.is_empty())
            .filter_map(|r| {
                user(r.start, r.end).map(|(start, end)| Ref {
                    start,
                    end,
                    target: r.target,
                })
            })
            .collect();
    }
    out
}

fn error_diagnostic(uri: &Url, lines: &Lines, off: usize, e: &Error) -> Diagnostic {
    let span = e
        .primary_span()
        .filter(|s| s.start >= off)
        .map(|s| (s.start - off, s.end - off));
    let (start, end) = match span {
        Some((s, e)) if e > s => (s, e),
        Some((s, _)) => (s, s + 1),
        None => (0, 0),
    };
    let mut message = format!("{}: {e}", e.kind());
    let mut related = Vec::new();
    if let Error::Type(TypeError::Kind(diag)) = e {
        for note in &diag.notes {
            message.push_str(&format!("\nnote: {note}"));
        }
        if let Some(help) = &diag.help {
            message.push_str(&format!("\nhelp: {help}"));
        }
        for (span, msg) in diag.labels.iter().filter(|(s, _)| s.start >= off) {
            related.push(DiagnosticRelatedInformation {
                location: Location::new(uri.clone(), lines.range(span.start - off, span.end - off)),
                message: msg.clone(),
            });
        }
    }
    Diagnostic {
        code: Some(NumberOrString::String(e.code().as_str().to_string())),
        related_information: (!related.is_empty()).then_some(related),
        ..diagnostic(lines, start, end, DiagnosticSeverity::ERROR, message)
    }
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
