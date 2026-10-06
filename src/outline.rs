use lsp_types::SymbolKind;
use prism::parse::parse;
use prism::syntax::ast::Span;

/// A top-level declaration and the names it introduces.
#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub kind: SymbolKind,
    pub span: (usize, usize),
    pub sel: (usize, usize),
    pub members: Vec<Member>,
}

#[derive(Clone, Debug)]
pub struct Member {
    pub name: String,
    pub kind: SymbolKind,
    pub sel: (usize, usize),
}

/// Every top-level declaration in `src`, in source order. `None` if it does not parse.
pub fn outline(src: &str) -> Option<Vec<Item>> {
    let p = parse(src).ok()?.program;
    let mut items = Vec::new();
    let mut push = |name: &str, kind, span: Span, members: Vec<(&str, SymbolKind, Span)>| {
        let span = (span.start, span.end.min(src.len()));
        let sel = find_word(src, span, name).unwrap_or((span.0, span.0));
        let members = members
            .into_iter()
            .filter(|(_, _, at)| at.end > at.start)
            .map(|(m, kind, at)| Member {
                name: m.to_string(),
                kind,
                sel: (at.start, at.end),
            })
            .collect();
        items.push(Item {
            name: name.to_string(),
            kind,
            span,
            sel,
            members,
        });
    };
    for d in &p.types {
        let ctors = d
            .ctors
            .iter()
            .map(|c| (c.name.as_str(), SymbolKind::ENUM_MEMBER, c.span))
            .collect();
        push(&d.name, SymbolKind::ENUM, d.span, ctors);
    }
    for d in &p.effects {
        let ops = d
            .ops
            .iter()
            .map(|o| (o.name.as_str(), SymbolKind::METHOD, o.span))
            .collect();
        push(&d.name, SymbolKind::INTERFACE, d.span, ops);
    }
    for d in &p.classes {
        let methods = d
            .methods
            .iter()
            .map(|m| (m.name.as_str(), SymbolKind::METHOD, m.span))
            .collect();
        push(&d.name, SymbolKind::INTERFACE, d.span, methods);
    }
    for d in &p.errors {
        push(&d.name, SymbolKind::EVENT, d.span, Vec::new());
    }
    for d in &p.synonyms {
        push(&d.name, SymbolKind::TYPE_PARAMETER, d.span, Vec::new());
    }
    for d in &p.aliases {
        push(&d.name, SymbolKind::TYPE_PARAMETER, d.span, Vec::new());
    }
    for d in &p.stable {
        push(&d.name, SymbolKind::STRUCT, d.span, Vec::new());
    }
    for d in &p.patterns {
        push(&d.name, SymbolKind::FUNCTION, d.span, Vec::new());
    }
    for d in &p.instances {
        push(&d.class, SymbolKind::OBJECT, d.span, Vec::new());
    }
    for d in p.fns.iter().chain(&p.logic_fns) {
        let kind = if d.konst {
            SymbolKind::CONSTANT
        } else {
            SymbolKind::FUNCTION
        };
        push(&d.name, kind, d.span, Vec::new());
    }
    items.sort_by_key(|i| i.span.0);
    Some(items)
}

/// The `-- |` doc comment directly above the line containing `at`.
pub fn doc_above(src: &str, at: usize) -> Option<String> {
    let line_start = src[..at.min(src.len())].rfind('\n').map_or(0, |i| i + 1);
    let mut lines: Vec<&str> = src[..line_start]
        .lines()
        .rev()
        .map(str::trim_start)
        .take_while(|l| l.starts_with("--"))
        .collect();
    lines.reverse();
    let first = lines.iter().position(|l| l.starts_with("-- |"))?;
    let body: Vec<&str> = lines[first..]
        .iter()
        .map(|l| l.trim_start_matches("-- |").trim_start_matches("--"))
        .map(|l| l.strip_prefix(' ').unwrap_or(l))
        .collect();
    Some(body.join("\n").trim().to_string()).filter(|d| !d.is_empty())
}

// First whole-word occurrence of `word` within `src[range]`.
fn find_word(src: &str, (lo, hi): (usize, usize), word: &str) -> Option<(usize, usize)> {
    let hay = src.get(lo..hi)?;
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
    hay.match_indices(word)
        .map(|(i, _)| lo + i)
        .find(|&s| {
            let e = s + word.len();
            !src[..s].ends_with(ident) && !src[e..].starts_with(ident)
        })
        .map(|s| (s, s + word.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "\
-- | A shape.
-- Two kinds.
type Shape = Circle(Int) | Square(Int)

fn area(s: Shape) : Int =
  match s of
    Circle(r) => 3 * r * r
    Square(w) => w * w
";

    #[test]
    fn declarations_members_and_docs() {
        let items = outline(SRC).unwrap();
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Shape", "area"]);
        let sq = items[0].members[1].sel;
        assert_eq!(&SRC[sq.0..sq.1], "Square");
        assert_eq!(
            doc_above(SRC, items[0].sel.0).as_deref(),
            Some("A shape.\nTwo kinds.")
        );
        assert_eq!(doc_above(SRC, items[1].sel.0), None);
    }
}
