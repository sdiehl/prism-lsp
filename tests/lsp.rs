//! Drives the server binary over stdio through one editing session.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next: i64,
}

impl Client {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_prism-lsp"))
            .env(
                "XDG_CACHE_HOME",
                Path::new(env!("CARGO_TARGET_TMPDIR")).join("cache"),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut client = Self {
            child,
            stdin,
            stdout,
            next: 0,
        };
        client.request("initialize", json!({ "capabilities": {} }));
        client.notify("initialized", json!({}));
        client
    }

    fn send(&mut self, msg: &Value) {
        let body = msg.to_string();
        let stdin = self.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut len = 0;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            match line.trim_end() {
                "" => break,
                l => {
                    if let Some(n) = l.strip_prefix("Content-Length: ") {
                        len = n.parse().unwrap();
                    }
                }
            }
        }
        let mut body = vec![0; len];
        self.stdout.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let msg = self.recv();
            if msg["id"] == id {
                assert!(msg["error"].is_null(), "{method}: {msg}");
                return msg["result"].clone();
            }
        }
    }

    fn diagnostics(&mut self) -> Vec<Value> {
        loop {
            let msg = self.recv();
            if msg["method"] == "textDocument/publishDiagnostics" {
                return msg["params"]["diagnostics"].as_array().unwrap().clone();
            }
        }
    }

    fn at(&mut self, method: &str, uri: &str, line: u32, character: u32) -> Value {
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": true },
        });
        self.request(method, params)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn change(client: &mut Client, uri: &str, version: i32, text: &str) {
    let params = json!({
        "textDocument": { "uri": uri, "version": version },
        "contentChanges": [{ "text": text }],
    });
    client.notify("textDocument/didChange", params);
}

#[test]
fn editing_session() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/shapes.pr");
    let uri = format!("file://{}", path.display());
    let text = std::fs::read_to_string(&path).unwrap();
    let mut c = Client::start();
    let doc = json!({ "uri": uri, "languageId": "prism", "version": 1, "text": text });
    c.notify("textDocument/didOpen", json!({ "textDocument": doc }));
    assert_eq!(c.diagnostics(), Vec::<Value>::new());

    // Hover on a local binder, and on a top-level name carrying a doc comment.
    let x = c.at("textDocument/hover", &uri, 10, 6);
    assert_eq!(x["contents"]["value"], "```prism\nx : Int\n```");
    let area = c.at("textDocument/hover", &uri, 10, 10)["contents"]["value"].clone();
    assert!(
        area.as_str().unwrap().contains("area : (Shape) -> Int"),
        "{area}"
    );
    assert!(
        area.as_str()
            .unwrap()
            .ends_with("Twice the area, so it stays an integer."),
        "{area}"
    );

    // Definitions: a constructor here, `max` in the prelude, `reverse` in Data.List.
    let circle = c.at("textDocument/definition", &uri, 10, 16);
    assert_eq!(circle["uri"], uri);
    assert_eq!(
        circle["range"]["start"],
        json!({ "line": 1, "character": 13 })
    );
    let max = c.at("textDocument/definition", &uri, 11, 11);
    assert!(
        max["uri"].as_str().unwrap().ends_with("/prelude.pr"),
        "{max}"
    );
    let reverse = c.at("textDocument/definition", &uri, 12, 19);
    assert!(
        reverse["uri"].as_str().unwrap().ends_with("/Data/List.pr"),
        "{reverse}"
    );

    // References from the declaration site: two calls plus the declaration.
    let refs = c.at("textDocument/references", &uri, 4, 4);
    assert_eq!(refs.as_array().unwrap().len(), 3, "{refs}");

    // A local: its binder and both uses, and goto from a use lands on the binder.
    let xs = c.at("textDocument/references", &uri, 10, 6);
    assert_eq!(xs.as_array().unwrap().len(), 3, "{xs}");
    let binder = c.at("textDocument/definition", &uri, 11, 14);
    assert_eq!(
        binder["range"]["start"],
        json!({ "line": 10, "character": 6 })
    );

    let symbols = c.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": uri } }),
    );
    let names: Vec<&str> = symbols
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Shape", "area", "main"]);
    assert_eq!(symbols[0]["children"].as_array().unwrap().len(), 2);

    // A type error is reported where it happens, with its code.
    change(&mut c, &uri, 2, &text.replace("area(Circle(2))", "area(2)"));
    let diags = c.diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["severity"], 1);
    assert_eq!(diags[0]["range"]["start"]["line"], 10);
    assert!(diags[0]["code"].as_str().unwrap().starts_with('E'));

    // Formatting rewrites a misformatted document and leaves a canonical one alone.
    let messy = text.replace("max(x, area", "max( x,area");
    change(&mut c, &uri, 3, &messy);
    c.diagnostics();
    let edits = c.request("textDocument/formatting", json!({ "textDocument": { "uri": uri }, "options": { "tabSize": 2, "insertSpaces": true } }));
    assert_eq!(edits[0]["newText"], text);
    change(&mut c, &uri, 4, &text);
    c.diagnostics();
    let edits = c.request("textDocument/formatting", json!({ "textDocument": { "uri": uri }, "options": { "tabSize": 2, "insertSpaces": true } }));
    assert_eq!(edits, json!([]));

    c.request("shutdown", Value::Null);
    c.notify("exit", Value::Null);
}

#[test]
fn errors_in_an_imported_module_point_at_the_import() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/broken");
    let path = dir.join("main.pr");
    let uri = format!("file://{}", path.display());
    let text = std::fs::read_to_string(&path).unwrap();
    let mut c = Client::start();
    let doc = json!({ "uri": uri, "languageId": "prism", "version": 1, "text": text });
    c.notify("textDocument/didOpen", json!({ "textDocument": doc }));
    let diags = c.diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(
        diags[0]["range"]["start"],
        json!({ "line": 0, "character": 0 })
    );
    assert!(
        diags[0]["message"]
            .as_str()
            .unwrap()
            .starts_with("in module Helper:"),
        "{diags:?}"
    );
    let related = &diags[0]["relatedInformation"][0]["location"];
    assert!(
        related["uri"]
            .as_str()
            .unwrap()
            .ends_with("/broken/Helper.pr"),
        "{diags:?}"
    );
    assert_eq!(related["range"]["start"]["line"], 0);
    c.request("shutdown", Value::Null);
    c.notify("exit", Value::Null);
}

#[test]
fn exits_when_the_editor_goes_away() {
    let mut c = Client::start();
    c.stdin = None;
    for _ in 0..100 {
        if c.child.try_wait().unwrap().is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("server still running five seconds after its stdin closed");
}
