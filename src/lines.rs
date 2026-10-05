use lsp_types::{Position, Range};

/// Byte offsets <-> LSP positions (UTF-16 columns).
pub struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    pub fn new(text: &'a str) -> Self {
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self { text, starts }
    }

    pub fn position(&self, offset: usize) -> Position {
        let offset = floor_char(self.text, offset);
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        let col = self.text[self.starts[line]..offset].encode_utf16().count();
        Position::new(line as u32, col as u32)
    }

    pub fn range(&self, start: usize, end: usize) -> Range {
        Range::new(self.position(start), self.position(end))
    }

    pub fn offset(&self, pos: Position) -> usize {
        let Some(&start) = self.starts.get(pos.line as usize) else {
            return self.text.len();
        };
        let mut units = 0;
        for (i, c) in self.text[start..].char_indices() {
            if units >= pos.character as usize || c == '\n' {
                return start + i;
            }
            units += c.len_utf16();
        }
        self.text.len()
    }

    pub fn end(&self) -> Position {
        self.position(self.text.len())
    }
}

fn floor_char(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_round_trip() {
        let text = "let a = \"\u{1F600}\"\nfn x";
        let lines = Lines::new(text);
        let x = text.rfind('x').unwrap();
        assert_eq!(lines.position(x), Position::new(1, 3));
        assert_eq!(lines.offset(Position::new(1, 3)), x);
        let after = text.find('\u{1F600}').unwrap() + 4;
        assert_eq!(lines.position(after), Position::new(0, 11));
        assert_eq!(lines.offset(Position::new(0, 11)), after);
    }
}
