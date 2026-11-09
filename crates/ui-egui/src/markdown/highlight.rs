//! A small generic lexer for code blocks: common keywords, strings, comments and numbers. It is not
//! a per-language highlighter; it only picks the comment syntax from the fence language. No egui.

/// What a run of code is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
}

/// Keywords shared by the usual languages (Rust, C family, JS/TS, Python, Go, shell, SQL in lower case).
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "case", "catch", "class", "const", "continue", "def", "default", "defer", "do", "else", "elif",
    "enum", "export", "extends", "false", "fi", "finally", "fn", "for", "from", "func", "function", "if", "impl", "import", "in",
    "interface", "is", "let", "lambda", "loop", "match", "mod", "move", "mut", "new", "nil", "None", "not", "null", "or", "and", "pass",
    "pub", "return", "self", "Self", "static", "struct", "switch", "then", "this", "throw", "trait", "true", "True", "False", "try",
    "type", "typeof", "use", "var", "void", "where", "while", "with", "yield", "select", "insert", "update", "delete", "create", "table",
    "join", "on", "group", "order", "by", "limit", "values", "into", "set", "echo", "done", "esac",
];

#[derive(Clone, Copy)]
struct Syntax {
    hash: bool,
    dashes: bool,
    slashes: bool,
    block: bool,
}

fn syntax(lang: Option<&str>) -> Syntax {
    let l = lang.unwrap_or("").to_ascii_lowercase();
    match l.as_str() {
        "py" | "python" | "sh" | "bash" | "zsh" | "shell" | "toml" | "yaml" | "yml" | "ruby" | "rb" | "r" | "make" | "makefile" | "dockerfile" => {
            Syntax { hash: true, dashes: false, slashes: false, block: false }
        }
        "sql" | "lua" | "haskell" | "hs" => Syntax { hash: false, dashes: true, slashes: false, block: false },
        _ => Syntax { hash: false, dashes: false, slashes: true, block: true },
    }
}

/// End of the run that starts at `start` and keeps `keep`; always a char boundary.
fn run_end(src: &str, start: usize, keep: impl Fn(char) -> bool) -> usize {
    match src.get(start..) {
        Some(rest) => rest.char_indices().find(|&(_, c)| !keep(c)).map_or(src.len(), |(i, _)| start.saturating_add(i)),
        None => src.len(),
    }
}

/// End of a string literal opened at `start` (a quote char), honouring backslash escapes; an
/// unterminated string runs to the end of its line, so half-streamed code stays readable.
fn string_end(src: &str, start: usize, quote: char) -> usize {
    let Some(rest) = src.get(start..) else { return src.len() };
    let mut escaped = false;
    for (i, c) in rest.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return start.saturating_add(i).saturating_add(c.len_utf8());
        } else if c == '\n' && quote != '`' {
            return start.saturating_add(i);
        }
    }
    src.len()
}

fn comment_end(src: &str, start: usize, block: bool) -> usize {
    if block {
        match src.get(start.saturating_add(2)..).and_then(|r| r.find("*/")) {
            Some(i) => start.saturating_add(2).saturating_add(i).saturating_add(2),
            None => src.len(),
        }
    } else {
        run_end(src, start, |c| c != '\n')
    }
}

/// Splits `src` into runs; concatenating them gives `src` back exactly.
pub fn tokens<'a>(lang: Option<&str>, src: &'a str) -> Vec<(Tok, &'a str)> {
    let syn = syntax(lang);
    let sql = matches!(lang, Some(l) if l.eq_ignore_ascii_case("sql"));
    let mut out: Vec<(Tok, &str)> = Vec::new();
    let mut pos = 0usize;
    let mut plain_from = 0usize;
    while let Some(rest) = src.get(pos..) {
        let Some(c) = rest.chars().next() else { break };
        let (tok, end) = if (syn.slashes && rest.starts_with("//")) || (syn.hash && c == '#') || (syn.dashes && rest.starts_with("--")) {
            (Tok::Comment, comment_end(src, pos, false))
        } else if syn.block && rest.starts_with("/*") {
            (Tok::Comment, comment_end(src, pos, true))
        } else if c == '"' || c == '\'' || c == '`' {
            (Tok::Str, string_end(src, pos, c))
        } else if c.is_ascii_digit() {
            (Tok::Number, run_end(src, pos, |c| c.is_ascii_alphanumeric() || c == '.' || c == '_'))
        } else if c.is_alphabetic() || c == '_' {
            let e = run_end(src, pos, |c| c.is_alphanumeric() || c == '_');
            let word = src.get(pos..e).unwrap_or("");
            let kw = KEYWORDS.contains(&word) || (sql && KEYWORDS.contains(&word.to_ascii_lowercase().as_str()));
            (if kw { Tok::Keyword } else { Tok::Plain }, e)
        } else {
            (Tok::Plain, pos.saturating_add(c.len_utf8()))
        };
        // Guarantee progress even if a helper returned the start.
        let end = end.max(pos.saturating_add(c.len_utf8()));
        if tok != Tok::Plain {
            if let Some(p) = src.get(plain_from..pos).filter(|p| !p.is_empty()) {
                out.push((Tok::Plain, p));
            }
            out.push((tok, src.get(pos..end).unwrap_or("")));
            plain_from = end;
        }
        pos = end;
    }
    if let Some(p) = src.get(plain_from..).filter(|p| !p.is_empty()) {
        out.push((Tok::Plain, p));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_rebuild_the_source() {
        for (lang, src) in [
            (Some("rust"), "fn main() { let x = \"é😀\"; // hi\n/* a */ 42 }"),
            (Some("py"), "def f(): # c\n    return 'x\n"),
            (None, "\"unterminated"),
            (Some("sql"), "SELECT 1 -- n\n"),
            (None, "/* open"),
            (None, "ñ😀 日本語 0x1F"),
        ] {
            let joined: String = tokens(lang, src).iter().map(|(_, s)| *s).collect();
            assert_eq!(joined, src);
        }
    }

    #[test]
    fn classifies() {
        let t = tokens(Some("rust"), "let s = \"a\"; // c\n7");
        assert!(t.contains(&(Tok::Keyword, "let")));
        assert!(t.contains(&(Tok::Str, "\"a\"")));
        assert!(t.contains(&(Tok::Comment, "// c")));
        assert!(t.contains(&(Tok::Number, "7")));
        assert!(tokens(Some("python"), "# x").contains(&(Tok::Comment, "# x")));
        assert!(tokens(Some("sql"), "SELECT").contains(&(Tok::Keyword, "SELECT")));
    }

    #[test]
    fn empty() {
        assert!(tokens(None, "").is_empty());
    }
}
