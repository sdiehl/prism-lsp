mod analysis;
mod lines;
mod locate;
mod outline;

use std::collections::HashMap;
use std::error::Error;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{
    DocumentSymbolRequest, Formatting, GotoDefinition, HoverRequest, References,
};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, DocumentSymbol, DocumentSymbolResponse, GotoDefinitionResponse,
    Hover, HoverContents, HoverProviderCapability, Location, MarkupContent, MarkupKind, OneOf,
    Position, PublishDiagnosticsParams, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Url,
};
use prism::Root;

use analysis::{Analysis, analyze};
use lines::Lines;
use locate::{Locator, roots_for};
use outline::{Item, doc_above, outline};

type Res<T> = Result<T, Box<dyn Error + Send + Sync>>;

// The compiler recurses deeply on large programs; give it room.
const STACK: usize = 512 << 20;
// Edits arriving within this window are checked once, at their final text.
const SETTLE: Duration = Duration::from_millis(150);
// A hovered span longer than this is shown by its type alone.
const LABEL_MAX: usize = 40;

fn main() -> Res<()> {
    if std::env::args().any(|a| a == "--version") {
        println!("prism-lsp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    thread::Builder::new()
        .name("prism-lsp".into())
        .stack_size(STACK)
        .spawn(serve)?
        .join()
        .map_err(|_| "server thread panicked")?
}

fn serve() -> Res<()> {
    let (conn, io) = Connection::stdio();
    let caps = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    };
    conn.initialize(serde_json::to_value(caps)?)?;
    let analyses = Arc::default();
    let (jobs, rx) = crossbeam_channel::unbounded();
    {
        let (sender, analyses) = (conn.sender.clone(), Arc::clone(&analyses));
        thread::Builder::new()
            .name("prism-check".into())
            .stack_size(STACK)
            .spawn(move || worker(&rx, &sender, &analyses))?;
    }
    let mut server = Server {
        docs: HashMap::new(),
        analyses,
        jobs,
        locator: Locator::new(),
    };
    for msg in &conn.receiver {
        match msg {
            Message::Request(req) => {
                if conn.handle_shutdown(&req)? {
                    break;
                }
                let id = req.id.clone();
                let resp = match server.request(req) {
                    Ok(value) => Response::new_ok(id, value),
                    Err(e) => Response::new_err(
                        id,
                        lsp_server::ErrorCode::RequestFailed as i32,
                        e.to_string(),
                    ),
                };
                conn.sender.send(resp.into())?;
            }
            Message::Notification(n) => server.notify(n)?,
            Message::Response(_) => {}
        }
    }
    // The writer thread ends once every sender is gone, the worker's included.
    drop(server);
    drop(conn);
    io.join()?;
    Ok(())
}

struct Doc {
    version: i32,
    text: String,
    roots: Arc<Vec<Root>>,
    outline: Vec<Item>,
}

struct Job {
    uri: Url,
    version: i32,
    text: Option<String>,
    roots: Arc<Vec<Root>>,
}

type Analyses = Arc<Mutex<HashMap<Url, Arc<Analysis>>>>;

struct Server {
    docs: HashMap<Url, Doc>,
    analyses: Analyses,
    jobs: Sender<Job>,
    locator: Locator,
}

impl Server {
    fn notify(&mut self, n: Notification) -> Res<()> {
        match n.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let p: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(n.params)?;
                let doc = p.text_document;
                self.update(doc.uri, doc.version, doc.text)
            }
            DidChangeTextDocument::METHOD => {
                let p: lsp_types::DidChangeTextDocumentParams = serde_json::from_value(n.params)?;
                let Some(change) = p.content_changes.into_iter().last() else {
                    return Ok(());
                };
                self.update(p.text_document.uri, p.text_document.version, change.text)
            }
            DidCloseTextDocument::METHOD => {
                let p: lsp_types::DidCloseTextDocumentParams = serde_json::from_value(n.params)?;
                let Some(doc) = self.docs.remove(&p.text_document.uri) else {
                    return Ok(());
                };
                let job = Job {
                    uri: p.text_document.uri,
                    version: doc.version,
                    text: None,
                    roots: doc.roots,
                };
                Ok(self.jobs.send(job)?)
            }
            _ => Ok(()),
        }
    }

    fn update(&mut self, uri: Url, version: i32, text: String) -> Res<()> {
        let roots = match self.docs.get(&uri) {
            Some(doc) => Arc::clone(&doc.roots),
            None => Arc::new(uri.to_file_path().map_or_else(
                |()| prism::default_roots(&PathBuf::from(".")),
                |p| roots_for(&p),
            )),
        };
        let prev = self
            .docs
            .remove(&uri)
            .map(|d| d.outline)
            .unwrap_or_default();
        let outline = outline(&text).unwrap_or(prev);
        self.jobs.send(Job {
            uri: uri.clone(),
            version,
            text: Some(text.clone()),
            roots: Arc::clone(&roots),
        })?;
        self.docs.insert(
            uri,
            Doc {
                version,
                text,
                roots,
                outline,
            },
        );
        Ok(())
    }

    fn request(&mut self, req: Request) -> Res<serde_json::Value> {
        let req = match cast::<HoverRequest>(req) {
            Ok((_, p)) => {
                let at = p.text_document_position_params;
                return json(self.hover(&at.text_document.uri, at.position));
            }
            Err(req) => req,
        };
        let req = match cast::<GotoDefinition>(req) {
            Ok((_, p)) => {
                let at = p.text_document_position_params;
                let loc = self.definition(&at.text_document.uri, at.position);
                return json(loc.map(GotoDefinitionResponse::Scalar));
            }
            Err(req) => req,
        };
        let req = match cast::<References>(req) {
            Ok((_, p)) => {
                let at = p.text_document_position;
                return json(self.references(
                    &at.text_document.uri,
                    at.position,
                    p.context.include_declaration,
                ));
            }
            Err(req) => req,
        };
        let req = match cast::<DocumentSymbolRequest>(req) {
            Ok((_, p)) => {
                return json(
                    self.symbols(&p.text_document.uri)
                        .map(DocumentSymbolResponse::Nested),
                );
            }
            Err(req) => req,
        };
        match cast::<Formatting>(req) {
            Ok((_, p)) => self.format(&p.text_document.uri).and_then(json),
            Err(req) => Err(format!("unsupported request {}", req.method).into()),
        }
    }

    // The analysis of a document's current text, if the checker has caught up.
    fn current(&self, uri: &Url) -> Option<(&Doc, Arc<Analysis>)> {
        let doc = self.docs.get(uri)?;
        let analysis = self.analyses.lock().ok()?.get(uri).cloned()?;
        (analysis.version == doc.version).then_some((doc, analysis))
    }

    fn hover(&mut self, uri: &Url, pos: Position) -> Option<Hover> {
        let (doc, analysis) = self.current(uri)?;
        let lines = Lines::new(&doc.text);
        let at = lines.offset(pos);
        let span = analysis.type_at(at)?;
        let label = &doc.text[span.start..span.end];
        let ty = pure(&span.rendered);
        let shown = match span.level.tag() {
            "" | "patternvar" | "hole" | "logic" if is_label(label) => format!("{label} : {ty}"),
            _ => ty.to_string(),
        };
        let mut value = format!("```prism\n{shown}\n```");
        let range = lines.range(span.start, span.end);
        let target = analysis.ref_at(at).map(|r| r.target.clone()).or_else(|| {
            doc.outline
                .iter()
                .find(|i| i.sel.0 <= at && at <= i.sel.1)
                .map(|i| i.name.clone())
        });
        if let Some(docs) = target.and_then(|t| self.docs_of(uri, &t)) {
            value.push_str("\n\n---\n\n");
            value.push_str(&docs);
        }
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(range),
        })
    }

    fn docs_of(&mut self, uri: &Url, target: &str) -> Option<String> {
        let doc = self.docs.get(uri)?;
        let (file, (start, _)) = self.locator.define(target, &doc.roots, &doc.outline)?;
        match file {
            None => doc_above(&doc.text, start),
            Some(file) => doc_above(&std::fs::read_to_string(file).ok()?, start),
        }
    }

    fn definition(&mut self, uri: &Url, pos: Position) -> Option<Location> {
        let (doc, analysis) = self.current(uri)?;
        let at = Lines::new(&doc.text).offset(pos);
        let target = analysis.ref_at(at)?.target.clone();
        self.location(uri, &target)
    }

    fn location(&mut self, uri: &Url, target: &str) -> Option<Location> {
        let doc = self.docs.get(uri)?;
        match self.locator.define(target, &doc.roots, &doc.outline)? {
            (None, (s, e)) => Some(Location::new(
                uri.clone(),
                Lines::new(&doc.text).range(s, e),
            )),
            (Some(file), (s, e)) => {
                let text = std::fs::read_to_string(&file).ok()?;
                Some(Location::new(
                    Url::from_file_path(&file).ok()?,
                    Lines::new(&text).range(s, e),
                ))
            }
        }
    }

    fn references(&mut self, uri: &Url, pos: Position, with_decl: bool) -> Option<Vec<Location>> {
        let (doc, analysis) = self.current(uri)?;
        let lines = Lines::new(&doc.text);
        let at = lines.offset(pos);
        let target = match analysis.ref_at(at) {
            Some(r) => r.target.clone(),
            None => decl_at(&doc.outline, at)?,
        };
        let mut locs: Vec<Location> = analysis
            .refs
            .iter()
            .filter(|r| r.target == target)
            .map(|r| Location::new(uri.clone(), lines.range(r.start, r.end)))
            .collect();
        if with_decl {
            locs.extend(self.location(uri, &target));
        }
        Some(locs)
    }

    fn symbols(&self, uri: &Url) -> Option<Vec<DocumentSymbol>> {
        let doc = self.docs.get(uri)?;
        let lines = Lines::new(&doc.text);
        #[allow(deprecated)]
        let sym = |name: &str, kind, span: (usize, usize), sel: (usize, usize), children| {
            DocumentSymbol {
                name: name.to_string(),
                detail: None,
                kind,
                tags: None,
                deprecated: None,
                range: lines.range(span.0, span.1),
                selection_range: lines.range(sel.0, sel.1),
                children,
            }
        };
        Some(
            doc.outline
                .iter()
                .map(|i| {
                    let members = i
                        .members
                        .iter()
                        .map(|m| sym(&m.name, m.kind, m.sel, m.sel, None))
                        .collect::<Vec<_>>();
                    sym(
                        &i.name,
                        i.kind,
                        i.span,
                        i.sel,
                        (!members.is_empty()).then_some(members),
                    )
                })
                .collect(),
        )
    }

    fn format(&self, uri: &Url) -> Res<Option<Vec<TextEdit>>> {
        let Some(doc) = self.docs.get(uri) else {
            return Ok(None);
        };
        let formatted = prism::format(&doc.text)?;
        if formatted == doc.text {
            return Ok(Some(Vec::new()));
        }
        let range = lsp_types::Range::new(Position::new(0, 0), Lines::new(&doc.text).end());
        Ok(Some(vec![TextEdit::new(range, formatted)]))
    }
}

// The declaration whose name the cursor is on, as a reference target.
fn decl_at(items: &[Item], at: usize) -> Option<String> {
    items
        .iter()
        .flat_map(|i| {
            std::iter::once((&i.name, i.sel)).chain(i.members.iter().map(|m| (&m.name, m.sel)))
        })
        .find(|(_, (s, e))| *s <= at && at <= *e)
        .map(|(name, _)| name.clone())
}

// `Int ! {| e0}` is an open row with no labels: nothing is performed here.
fn pure(ty: &str) -> &str {
    match ty.rsplit_once(" ! {| ") {
        Some((ty, tail))
            if tail
                .strip_suffix('}')
                .is_some_and(|v| v.chars().all(|c| c.is_alphanumeric() || c == '_')) =>
        {
            ty
        }
        _ => ty,
    }
}

fn is_label(text: &str) -> bool {
    text.len() <= LABEL_MAX && !text.contains(char::is_whitespace)
}

fn worker(rx: &Receiver<Job>, sender: &Sender<Message>, analyses: &Analyses) {
    while let Ok(first) = rx.recv() {
        let mut pending = HashMap::from([(first.uri.clone(), first)]);
        while let Ok(job) = rx.recv_timeout(SETTLE) {
            pending.insert(job.uri.clone(), job);
        }
        for job in pending.into_values() {
            let diagnostics = match &job.text {
                None => {
                    analyses.lock().map(|mut a| a.remove(&job.uri)).ok();
                    Vec::new()
                }
                Some(text) => {
                    let analysis = catch_unwind(AssertUnwindSafe(|| {
                        analyze(&job.uri, job.version, text, &job.roots)
                    }))
                    .unwrap_or_else(|_| crashed(job.version));
                    let diagnostics = analysis.diagnostics.clone();
                    analyses
                        .lock()
                        .map(|mut a| a.insert(job.uri.clone(), Arc::new(analysis)))
                        .ok();
                    diagnostics
                }
            };
            let params = PublishDiagnosticsParams {
                uri: job.uri,
                diagnostics,
                version: Some(job.version),
            };
            let note = Notification::new(PublishDiagnostics::METHOD.to_string(), params);
            if sender.send(note.into()).is_err() {
                return;
            }
        }
    }
}

fn crashed(version: i32) -> Analysis {
    let diagnostic = Diagnostic {
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("prism".into()),
        message: "internal compiler error while checking this file (see the server log)".into(),
        ..Diagnostic::default()
    };
    Analysis {
        version,
        diagnostics: vec![diagnostic],
        ..Analysis::default()
    }
}

fn cast<R: lsp_types::request::Request>(req: Request) -> Result<(RequestId, R::Params), Request> {
    req.extract(R::METHOD).map_err(|e| match e {
        ExtractError::MethodMismatch(req) => req,
        ExtractError::JsonError { method, error } => Request::new(
            RequestId::from(0),
            format!("{method}: {error}"),
            serde_json::Value::Null,
        ),
    })
}

fn json<T: serde::Serialize>(value: T) -> Res<serde_json::Value> {
    Ok(serde_json::to_value(value)?)
}
