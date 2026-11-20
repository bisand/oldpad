//! The tablet page must run on old Safari (iOS 5–9). This rejects JavaScript
//! syntax and APIs those browsers do not have. It is a word-level check, not a
//! parser: comments and string literals are stripped first, then forbidden
//! words and operators are looked for.

const FORBIDDEN_WORDS: &[&str] = &[
    "let", "const", "class", "async", "await", "yield", "of", "import", "export", // ES2015+ syntax
    "fetch", "Promise", "Symbol", "Map", "Set", "WeakMap", "Proxy", "Reflect", // ES2015+ APIs
    "includes", "startsWith", "endsWith", "padStart", "assign", "entries", "find", "findIndex", "from",
];
const FORBIDDEN_OPERATORS: &[&str] = &["=>", "`", "...", "?.", "??", "**"];

fn strip(src: &str) -> String {
    let mut out = String::new();
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
            }
            '\'' | '"' => {
                let mut escaped = false;
                for d in chars.by_ref() {
                    if escaped {
                        escaped = false;
                    } else if d == '\\' {
                        escaped = true;
                    } else if d == c {
                        break;
                    }
                }
                out.push_str("''");
            }
            _ => out.push(c),
        }
    }
    out
}

fn problems(src: &str) -> Vec<String> {
    let code = strip(src);
    let mut found = Vec::new();
    for (n, line) in code.lines().enumerate() {
        // Regex literals are not stripped; skip operator checks on lines that use them.
        let has_regex = line.contains(".replace(/") || line.contains("= /");
        for op in FORBIDDEN_OPERATORS {
            if !has_regex && line.contains(op) {
                found.push(format!("line {}: `{op}`", n + 1));
            }
        }
        for word in line.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')) {
            if FORBIDDEN_WORDS.contains(&word) {
                found.push(format!("line {}: `{word}`", n + 1));
            }
        }
    }
    found
}

#[test]
fn tablet_js_is_es5() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/web/tablet.js")).unwrap();
    let found = problems(&src);
    assert!(found.is_empty(), "tablet.js uses syntax old iPads do not support:\n{}", found.join("\n"));
}

#[test]
fn the_check_catches_modern_code() {
    assert!(!problems("var f = (a) => a;").is_empty());
    assert!(!problems("const x = 1;").is_empty());
    assert!(!problems("fetch('/x').then(f);").is_empty());
    assert!(!problems("var s = `hi`;").is_empty());
    assert!(problems("var s = 'const => `';  // let it be\n/* async */ var x = 1;").is_empty());
}
