//! Enter in code: preserve indentation, add a level after an opener, and
//! split an existing bracket pair without moving its closing bracket inward.

pub(super) struct Newline {
    pub text: String,
    pub caret: usize,
    pub remove: usize,
}

pub(super) fn newline(text: &str, at: usize, lang: &str) -> Newline {
    let before = &text[..at];
    let line = &before[before.rfind('\n').map_or(0, |i| i + 1)..];
    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let lang = crate::lang::from_token(lang);
    let brackets = matches!(lang.as_str(), "Python" | "Rust" | "JavaScript" | "TypeScript" | "Java" | "C" | "C++" | "C#" | "Go" | "Swift" | "Kotlin" | "JSON" | "CSS" | "SCSS" | "Dart" | "PHP" | "Scala" | "Groovy" | "PowerShell");
    let last = if brackets { last_code(before, &lang) } else { None };
    let closer = match last { Some(b'{') => Some(b'}'), Some(b'[') => Some(b']'), Some(b'(') => Some(b')'), _ => None };
    let deeper = closer.is_some() || (lang == "Python" && last == Some(b':'));
    let mut ins = format!("\n{indent}");
    if deeper { ins.push_str(&indent_unit(text, &indent)); }
    let caret = ins.len();
    let mut remove = 0;
    if let Some(closer) = closer {
        let after = &text.as_bytes()[at..];
        let spaces = after.iter().take_while(|c| matches!(c, b' ' | b'\t')).count();
        if after.get(spaces) == Some(&closer) {
            ins.push_str(&format!("\n{indent}"));
            remove = spaces;
        }
    }
    Newline { text: ins, caret, remove }
}

/// Reuse tabs or the smallest existing indent (usually 2 or 4 spaces).
/// A fresh code block matches the editor's four-space Tab key.
fn indent_unit(text: &str, indent: &str) -> String {
    let first = text.lines().find(|l| !l.trim().is_empty() && l.starts_with([' ', '\t']));
    if indent.contains('\t') || (indent.is_empty() && first.is_some_and(|l| l.starts_with('\t'))) { return "\t".into() }
    let spaces = text.lines().filter(|l| !l.trim().is_empty()).map(|l| l.bytes().take_while(|c| *c == b' ').count()).filter(|n| *n > 0).min().filter(|n| *n <= 8).unwrap_or(4);
    " ".repeat(spaces)
}

/// A small lexical scan, not a parser: punctuation inside quoted strings or
/// comments must not trigger indentation. Scan earlier lines too so Python
/// triple quotes, template strings and block comments keep their meaning.
fn last_code(text: &str, lang: &str) -> Option<u8> {
    let b = text.as_bytes();
    let hash = matches!(lang, "Python" | "PowerShell");
    let slash = lang != "Python";
    let (mut i, mut last) = (0, None);
    while i < b.len() {
        if b[i] == b'\n' { last = None; i += 1; continue }
        if b[i].is_ascii_whitespace() { i += 1; continue }
        if (hash && b[i] == b'#') || (slash && b[i..].starts_with(b"//")) {
            match b[i..].iter().position(|c| *c == b'\n') { Some(n) => { i += n + 1; last = None; continue }, None => return last }
        }
        if slash && b[i..].starts_with(b"/*") {
            let end = b[i + 2..].windows(2).position(|w| w == b"*/")? + i + 4;
            if b[i..end].contains(&b'\n') { last = None; }
            i = end;
            continue;
        }
        if matches!(b[i], b'\'' | b'"' | b'`') {
            // Rust lifetimes such as 'a are not character literals.
            if lang == "Rust" && b[i] == b'\'' && b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) && b.get(i + 2) != Some(&b'\'') { last = Some(b'\''); i += 1; continue }
            let q = b[i];
            let n = if lang == "Python" && b[i..].starts_with(&[q, q, q]) { 3 } else { 1 };
            i += n;
            loop {
                if i >= b.len() { return None }
                if b[i] == b'\\' { i += 2; continue }
                if b[i..].starts_with(&[q, q, q][..n]) { i += n; break }
                i += 1;
            }
            last = Some(b'x');
            continue;
        }
        last = Some(b[i]);
        i += 1;
    }
    last
}
