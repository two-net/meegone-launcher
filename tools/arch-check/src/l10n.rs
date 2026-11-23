//! The localisation rules of ADR 0040 that a scan of the sources can check (L10N-01, L10N-03):
//!
//! - **The Java host writes no text for the user.** No literal with a letter goes into a text
//!   sink ([`TEXT_SINKS`]), such as `setText("Hello")`. No other literal in the code that ships
//!   reads as words, except those in [`TEXT_ALLOWED`], each with its reason: a log's tag or
//!   message, a thread's name or an exception's message never reaches the screen, so those pass.
//!   No number becomes text for a sink without the locale's digits ([`NUMBER_TO_TEXT`]).
//! - **No layout or manifest attribute holds text** instead of a string resource.
//! - **The Rust app writes no words.** Outside tests and fakes, no Rust literal reads as prose,
//!   except in a panic's or an assertion's message and in [`PROSE_ALLOWED`]. What goes wrong is a
//!   `Problem`, whose code the host words.
//! - **Every `Problem` is worded.** Its code has a constant and a case in `Problems.java` and a
//!   `problem_<code>` string resource, and no code has only some of them.
//! - **Every string resource can be translated.** It follows a comment for translators, in its
//!   group of resources; it numbers its arguments (`%1$s`) when it has more than one; and only
//!   the product's name is marked untranslatable.
//!
//! The rules are pure functions over source text. `tests/l10n.rs` runs them on this repository.
//! Like the other scans, they catch accidents rather than determined workarounds; what is drawn
//! is checked on a device, in the pseudo-locales, by the accessibility audit (ADR 0040).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::a11y::{closing_paren, is_word_char, java_code_only, java_files, line_of, word_before};
use crate::legal::{ANDROID_SOURCES, directories};
use crate::{Violation, files_under, relative, rust_files};

/// The Java source sets that ship: the debug build's own code, such as the audit, never does.
pub const SHIPPED_SOURCE_SETS: [&str; 2] = ["main", "release"];

/// Calls whose arguments are text for the user to read or hear. The last part of the call's
/// name is matched, so `view.setText` and `new AccessibilityAction` count.
pub const TEXT_SINKS: [&str; 16] = [
    "setText",
    "setHint",
    "setContentDescription",
    "setStateDescription",
    "setTooltipText",
    "setError",
    "setTitle",
    "setSubtitle",
    "setMessage",
    "setPositiveButton",
    "setNegativeButton",
    "setNeutralButton",
    "makeText",
    "announceForAccessibility",
    "AccessibilityAction",
    "addShortcut",
];

/// Ways a number becomes text in ASCII digits whatever the locale. In a text sink, a number goes
/// through `NumberFormat`, a string resource's `%d`, or `String.format` with a `Locale`
/// (L10N-03).
pub const NUMBER_TO_TEXT: [&str; 5] = [
    "Integer.toString",
    "Long.toString",
    "Float.toString",
    "Double.toString",
    "String.valueOf",
];

/// Java literals that read as words but are never shown, as `(file, where, reason)`. `where` is
/// the name the literal is given (`skeleton = "Hm"`), a call it is an argument of
/// (`setRequestProperty`) or, in Rust, the function it is in. The file is relative to the repository root. An entry that matches
/// nothing is reported, so the list stays true.
pub const TEXT_ALLOWED: [(&str, &str, &str); 5] = [
    (
        "android/app/src/main/java/io/github/twonet/meegone/Bridge.java",
        "USER_AGENT",
        "the User-Agent of the image feed's requests, never shown (PRIV-02)",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/Bridge.java",
        "setRequestProperty",
        "an HTTP header's name",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/Bridge.java",
        "getHeaderField",
        "an HTTP header's name",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/Bridge.java",
        "LATIN_LETTERS",
        "the ICU transliterator's id, for search (L10N-04)",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/Bridge.java",
        "skeleton",
        "ICU date skeletons, which the locale turns into its own pattern (L10N-03)",
    ),
];

/// Rust literals that read as prose but are never shown, as `(file, where, reason)`, matched as
/// in [`TEXT_ALLOWED`].
pub const PROSE_ALLOWED: [(&str, &str, &str); 5] = [
    (
        "crates/core/ui/src/tokens.rs",
        "what",
        "what a pairing of colours draws, for the contrast tests' messages (ADR 0038)",
    ),
    (
        "src/panics.rs",
        "described",
        "where a panic happened, never its message, written to logcat and thrown as a Java \
         exception for logcat only (ADR 0005, ADR 0042, ADR 0043)",
    ),
    (
        "src/android.rs",
        "ok_or",
        "a failure `guarded` throws as a Java exception, for logcat only (ADR 0005)",
    ),
    (
        "src/android.rs",
        "map_err",
        "a failure `guarded` throws as a Java exception, for logcat only (ADR 0005)",
    ),
    (
        "src/android.rs",
        "Err",
        "a failure `guarded` throws as a Java exception, for logcat only (ADR 0005)",
    ),
];

/// Calls whose literal arguments are for developers: a panic's, an assertion's, a build
/// error's or a log's message, a thread's name, an exception's message. The last part of the
/// name is matched.
const FOR_DEVELOPERS: [&str; 13] = [
    "expect",
    "compile_error!",
    "panic!",
    "unreachable!",
    "todo!",
    "unimplemented!",
    "assert!",
    "assert_eq!",
    "assert_ne!",
    "debug_assert!",
    "debug_assert_eq!",
    "debug_assert_ne!",
    "new Thread",
];

/// A string literal in source code, and where it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Literal {
    /// The 1-based line it starts on.
    pub line: usize,
    /// Its text, with the language's escapes resolved.
    pub text: String,
    /// The calls it is an argument of, innermost first, as written, such as `view.setText`,
    /// `Log.w`, `new Thread` or `format!`.
    pub calls: Vec<String>,
    /// The name it is given, if it is given one directly: the variable, constant or field before
    /// `=`, or in Rust before `:`.
    pub name: Option<String>,
    /// Whether it is inside a Rust attribute, such as `#[allow(…, reason = "…")]`.
    pub in_attribute: bool,
    /// The Rust function it is in, if any.
    pub function: Option<String>,
}

impl Literal {
    /// Whether `what` is its name, one of its calls, by the call's last part, or the function
    /// it is in.
    fn is_in(&self, what: &str) -> bool {
        self.name.as_deref() == Some(what)
            || self.function.as_deref() == Some(what)
            || self.calls.iter().any(|call| last_part(call) == what)
    }
}

/// The last part of a call's name: `setText` of `view.setText`, `AccessibilityAction` of
/// `new AccessibilityAction`, and `map_err` of `map_err`.
fn last_part(call: &str) -> &str {
    let call = call.strip_prefix("new ").unwrap_or(call);
    call.rsplit(['.', ':']).next().unwrap_or(call)
}

/// Whether `text` reads as words a person would read: two letters with white space between
/// them, or a word that starts with a capital and goes on in small letters, such as "Hello" or
/// "MeeGone". Identifiers, keys and dotted names, such as `alertTitle` or `android.appInfo`,
/// don't.
pub fn reads_as_words(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if spaces_words(&chars, i) {
            return true;
        }
        let starts_word = i == 0
            || !(chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], '.' | '_' | '\\' | '{'));
        if starts_word
            && c.is_uppercase()
            && chars.get(i + 1).is_some_and(|next| next.is_lowercase())
        {
            return true;
        }
    }
    false
}

/// Whether `text` reads as prose: two letters with white space between them.
pub fn reads_as_prose(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len()).any(|i| spaces_words(&chars, i))
}

/// Whether `chars[i]` is white space between two letters.
fn spaces_words(chars: &[char], i: usize) -> bool {
    chars[i].is_whitespace()
        && chars[..i]
            .iter()
            .rev()
            .find(|c| !c.is_whitespace())
            .is_some_and(|c| c.is_alphabetic())
        && chars[i..]
            .iter()
            .find(|c| !c.is_whitespace())
            .is_some_and(|c| c.is_alphabetic())
}

/// The string literals of Java `source`, including text blocks, with where each stands.
pub fn java_literals(source: &str) -> Vec<Literal> {
    let code: Vec<char> = java_code_only(source).chars().collect();
    let original: Vec<char> = source.chars().collect();
    let mut literals = Vec::new();
    let mut i = 0;
    while i < code.len() {
        if code[i] == '\'' {
            // A character literal, its content blanked: `' '`, or `'  '` for an escape.
            i += 1;
            while i < code.len() && code[i] != '\'' && code[i] != '\n' {
                i += 1;
            }
            i += 1;
            continue;
        }
        if code[i] != '"' {
            i += 1;
            continue;
        }
        let block = code[i..].starts_with(&['"', '"', '"']);
        let (open, close) = if block { (3, "\"\"\"") } else { (1, "\"") };
        let start = i + open;
        let mut end = start;
        while end < code.len() && !code[end..].starts_with(&close.chars().collect::<Vec<_>>()) {
            if !block && code[end] == '\n' {
                break;
            }
            end += 1;
        }
        let raw: String = original[start..end.min(original.len())].iter().collect();
        let (calls, name, _) = context(&code, i, false);
        literals.push(Literal {
            line: line_at(&code, i),
            text: unescape(&raw),
            calls,
            name,
            in_attribute: false,
            function: None,
        });
        i = end + close.len();
    }
    literals
}

/// The string literals of Rust `source` outside its `#[cfg(test)]` items, with where each
/// stands. Byte strings aren't text, so they are left out; a raw string is taken as written.
pub fn rust_literals(source: &str) -> Vec<Literal> {
    let (code, found) = rust_code(source);
    let tests = test_items(&code);
    found
        .into_iter()
        .filter(|(at, _, _)| !tests.iter().any(|range| range.contains(at)))
        .map(|(at, raw, is_raw)| {
            let (calls, name, in_attribute) = context(&code, at, true);
            Literal {
                line: line_at(&code, at),
                text: if is_raw { raw } else { unescape(&raw) },
                calls,
                name,
                in_attribute,
                function: enclosing_function(&code, at),
            }
        })
        .collect()
}

/// The name of the Rust function whose body holds character `at` of `code`, if any.
fn enclosing_function(code: &[char], at: usize) -> Option<String> {
    let mut depth = 0usize;
    let mut i = at;
    while i > 0 {
        i -= 1;
        match code[i] {
            '}' => depth += 1,
            '{' if depth > 0 => depth -= 1,
            '{' => {
                // The block's header, back to the end of what came before it.
                let start = (0..i)
                    .rev()
                    .find(|k| matches!(code[*k], ';' | '{' | '}'))
                    .map_or(0, |k| k + 1);
                let header: String = code[start..i].iter().collect();
                if let Some(after) = header
                    .split_whitespace()
                    .skip_while(|word| *word != "fn")
                    .nth(1)
                {
                    let name: String = after.chars().take_while(|c| is_word_char(*c)).collect();
                    if !name.is_empty() {
                        return Some(name);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Rust `source` as characters with comments and literals blanked, line breaks kept, so each
/// character stands where it did; and each text literal, by where its opening quote stands, as
/// written between its quotes, and whether it is raw.
fn rust_code(source: &str) -> (Vec<char>, Vec<(usize, String, bool)>) {
    let chars: Vec<char> = source.chars().collect();
    let at = |i: usize, text: &str| {
        text.chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };
    let word = |i: usize| {
        chars
            .get(i)
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
    };
    let mut code: Vec<char> = Vec::with_capacity(chars.len());
    let mut literals = Vec::new();
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    let mut i = 0;
    while i < chars.len() {
        let before_word = i > 0 && word(i - 1);
        if at(i, "//") {
            while i < chars.len() && chars[i] != '\n' {
                code.push(' ');
                i += 1;
            }
        } else if at(i, "/*") {
            // Block comments nest.
            let mut depth = 0;
            while i < chars.len() {
                if at(i, "/*") {
                    depth += 1;
                    code.extend("  ".chars());
                    i += 2;
                } else if at(i, "*/") {
                    depth -= 1;
                    code.extend("  ".chars());
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    code.push(blank(chars[i]));
                    i += 1;
                }
            }
        } else if !before_word
            && (at(i, "r\"") || at(i, "r#") || at(i, "br\"") || at(i, "br#"))
            && raw_end(&chars, i).is_some()
        {
            let bytes = chars[i] == 'b';
            let prefix = usize::from(bytes) + 1;
            let hashes = chars[i + prefix..]
                .iter()
                .take_while(|c| **c == '#')
                .count();
            let open = i + prefix + hashes;
            let end = raw_end(&chars, i).unwrap_or(chars.len());
            let close = end - 1 - hashes;
            if !bytes {
                literals.push((open, chars[open + 1..close].iter().collect(), true));
            }
            code.extend(chars[i..end].iter().enumerate().map(|(k, c)| {
                if i + k == open || i + k == close {
                    '"'
                } else {
                    blank(*c)
                }
            }));
            i = end;
        } else if chars[i] == '"' || (!before_word && at(i, "b\"")) {
            let bytes = chars[i] == 'b';
            let open = i + usize::from(bytes);
            let mut end = open + 1;
            while end < chars.len() && chars[end] != '"' {
                end += if chars[end] == '\\' { 2 } else { 1 };
            }
            let end = end.min(chars.len());
            if !bytes {
                literals.push((open, chars[open + 1..end].iter().collect(), false));
            }
            let after = (end + 1).min(chars.len());
            code.extend(chars[i..after].iter().enumerate().map(|(k, c)| {
                if i + k == open || i + k == end {
                    '"'
                } else {
                    blank(*c)
                }
            }));
            i = end + 1;
        } else if at(i, "'\\") {
            let start = i;
            i += 2;
            while i < chars.len() && chars[i] != '\'' {
                i += 1;
            }
            i += 1;
            code.extend((start..i.min(chars.len())).map(|_| ' '));
        } else if chars[i] == '\'' && chars.get(i + 2) == Some(&'\'') {
            code.extend("   ".chars());
            i += 3;
        } else {
            code.push(chars[i]);
            i += 1;
        }
    }
    (code, literals)
}

/// Where the raw string starting at `start`, with its `b` and `r`, ends, if it is one.
fn raw_end(chars: &[char], start: usize) -> Option<usize> {
    let prefix = if chars[start] == 'b' { 2 } else { 1 };
    let hashes = chars[start + prefix..]
        .iter()
        .take_while(|c| **c == '#')
        .count();
    let open = start + prefix + hashes;
    if chars.get(open) != Some(&'"') {
        return None;
    }
    let mut i = open + 1;
    while i < chars.len() {
        if chars[i] == '"'
            && chars[i + 1..]
                .iter()
                .take(hashes)
                .filter(|c| **c == '#')
                .count()
                == hashes
        {
            return Some(i + 1 + hashes);
        }
        i += 1;
    }
    None
}

/// The character ranges of `code`'s `#[cfg(test)]` items: a module, a function, a field, or a
/// statement such as an `if`. Each runs from its attribute to the end of its block, or to the
/// `;` or `,` that ends it.
fn test_items(code: &[char]) -> Vec<std::ops::Range<usize>> {
    const ATTRIBUTE: &str = "#[cfg(test)]";
    let text: String = code.iter().collect();
    let mut ranges = Vec::new();
    for (byte, _) in text.match_indices(ATTRIBUTE) {
        let at = text[..byte].chars().count();
        let mut parens = 0usize;
        let mut end = code.len();
        let mut k = at + ATTRIBUTE.chars().count();
        while k < code.len() {
            match code[k] {
                '(' | '[' => parens += 1,
                ')' | ']' => parens = parens.saturating_sub(1),
                ';' | ',' if parens == 0 => {
                    end = k + 1;
                    break;
                }
                '{' if parens == 0 => {
                    end = block_end(code, k);
                    break;
                }
                _ => {}
            }
            k += 1;
        }
        ranges.push(at..end);
    }
    ranges
}

/// Where the block whose `{` stands at `open` ends, just past its `}`.
fn block_end(code: &[char], open: usize) -> usize {
    let mut depth = 0usize;
    for (k, c) in code.iter().enumerate().skip(open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return k + 1;
                }
            }
            _ => {}
        }
    }
    code.len()
}

/// The 1-based line of character `at` of `code`.
fn line_at(code: &[char], at: usize) -> usize {
    code[..at].iter().filter(|c| **c == '\n').count() + 1
}

/// The calls the literal whose opening quote stands at `start` is an argument of, innermost
/// first; the name it is given directly, if any; and, for Rust, whether it is in an attribute.
/// It looks back to the start of the statement or block, and through a closure's braces.
fn context(code: &[char], start: usize, rust: bool) -> (Vec<String>, Option<String>, bool) {
    let mut calls = Vec::new();
    let mut name = None;
    let mut direct = true;
    // After a Rust `=`, a `:` before it names what has a type: `const X: &str = "…"`.
    let mut typed = false;
    let (mut parens, mut braces, mut brackets) = (0usize, 0usize, 0usize);
    let mut i = start;
    while i > 0 {
        i -= 1;
        let level = parens == 0 && braces == 0 && brackets == 0;
        match code[i] {
            ')' => parens += 1,
            '(' if parens > 0 => parens -= 1,
            '(' => {
                calls.push(callee(code, i));
                direct = false;
            }
            ']' => brackets += 1,
            '[' if brackets > 0 => brackets -= 1,
            '[' => {
                if i > 0 && code[i - 1] == '#' {
                    return (calls, name, true);
                }
                direct = false;
            }
            '}' => braces += 1,
            '{' if braces > 0 => braces -= 1,
            '{' => {
                // A closure's body goes on to the call it is an argument of; any other block,
                // or a struct literal, is as far as the literal's statement goes.
                let closure = before_space(code, i).is_some_and(|k| {
                    code[k] == '|' || (code[k] == '>' && k > 0 && code[k - 1] == '-')
                });
                if !closure {
                    break;
                }
                direct = false;
                typed = false;
            }
            ';' if level => break,
            ',' if level => {
                direct = false;
                typed = false;
            }
            '=' if level && direct && name.is_none() && is_assignment(code, i) => {
                name = ident_before(code, i);
                direct = false;
                typed = rust;
            }
            ':' if level
                && rust
                && (direct && name.is_none() || typed)
                && code.get(i + 1) != Some(&':')
                && (i == 0 || code[i - 1] != ':') =>
            {
                name = ident_before(code, i);
                direct = false;
                typed = false;
            }
            _ => {}
        }
    }
    (calls, name, false)
}

/// Whether the `=` at `at` assigns, rather than compares or arrows.
fn is_assignment(code: &[char], at: usize) -> bool {
    let before = at.checked_sub(1).map(|k| code[k]);
    let after = code.get(at + 1).copied();
    !matches!(before, Some('=' | '!' | '<' | '>'))
        && !matches!(after, Some('=' | '>'))
        && !matches!(before, Some('+' | '-' | '*' | '/' | '%' | '&' | '|' | '^'))
}

/// The index of the last character before `at` that isn't white space.
fn before_space(code: &[char], at: usize) -> Option<usize> {
    (0..at).rev().find(|k| !code[*k].is_whitespace())
}

/// The identifier just before `at`, past white space, or `None`.
fn ident_before(code: &[char], at: usize) -> Option<String> {
    let end = before_space(code, at)? + 1;
    let start = (0..end)
        .rev()
        .find(|k| !is_word_char(code[*k]))
        .map_or(0, |k| k + 1);
    (start < end).then(|| code[start..end].iter().collect())
}

/// The name of the call whose `(` stands at `open`: a path such as `view.setText`, `Log.w` or
/// `self.vm.env`, a macro's name with its `!`, or `new` and a class.
fn callee(code: &[char], open: usize) -> String {
    let Some(mut end) = before_space(code, open).map(|k| k + 1) else {
        return String::new();
    };
    let bang = code[end - 1] == '!';
    if bang {
        end -= 1;
    }
    let start = (0..end)
        .rev()
        .find(|k| !(is_word_char(code[*k]) || matches!(code[*k], '.' | ':')))
        .map_or(0, |k| k + 1);
    let mut name: String = code[start..end]
        .iter()
        .collect::<String>()
        .trim_start_matches(['.', ':'])
        .to_owned();
    if bang {
        name.push('!');
    }
    let before: String = code[..start].iter().collect();
    if before.trim_end().ends_with("new") && !before.trim_end().ends_with("_new") {
        let keyword = before.trim_end();
        if keyword.len() == 3 || !keyword[..keyword.len() - 3].ends_with(is_word_char) {
            return format!("new {name}");
        }
    }
    name
}

/// `raw` with backslash escapes resolved, as Java and Rust both write them: `\n`, `\t`, `\\`,
/// `\"`, `\'`, `\uXXXX` and `\u{…}`. Anything else is kept as written.
fn unescape(raw: &str) -> String {
    let mut text = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => text.push('\n'),
            Some('t') => text.push('\t'),
            Some('r') => text.push('\r'),
            Some('s') => text.push(' '),
            Some('u') => {
                let braced = chars.peek() == Some(&'{');
                if braced {
                    chars.next();
                }
                let mut digits = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_hexdigit() && (braced || digits.len() < 4) {
                        digits.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if braced && chars.peek() == Some(&'}') {
                    chars.next();
                }
                match u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    Some(decoded) => text.push(decoded),
                    None => {
                        text.push_str("\\u");
                        text.push_str(&digits);
                    }
                }
            }
            Some(other) if matches!(other, '\\' | '"' | '\'') => text.push(other),
            Some(other) => {
                // A regular expression's escape, such as `\p{Mn}`, kept for the reader.
                text.push('\\');
                text.push(other);
            }
            None => text.push('\\'),
        }
    }
    text
}

/// Where `file`'s Java `source` writes text for the user (L10N-01) or turns a number into text
/// in ASCII digits for a sink (L10N-03), as [`java_violations`] checks every file. `used`
/// collects the entries of `allowed` that matched.
fn java_file_violations<'a>(
    file: &str,
    source: &str,
    allowed: &'a [(&'a str, &'a str, &'a str)],
    used: &mut BTreeSet<(&'a str, &'a str)>,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    for literal in java_literals(source) {
        if !literal.text.chars().any(char::is_alphabetic) {
            continue;
        }
        let line = literal.line;
        let text = shown(&literal.text);
        if let Some(sink) = literal
            .calls
            .iter()
            .find(|call| TEXT_SINKS.contains(&last_part(call)))
        {
            violations.push(Violation(format!(
                "`{file}:{line}` gives `{sink}` the text \"{text}\"; take it from a string \
                 resource so it can be translated (L10N-01)"
            )));
            continue;
        }
        if !reads_as_words(&literal.text) || for_developers(&literal) {
            continue;
        }
        if let Some((path, what, _)) = allowed
            .iter()
            .find(|(path, what, _)| *path == file && literal.is_in(what))
        {
            used.insert((*path, *what));
            continue;
        }
        violations.push(Violation(format!(
            "`{file}:{line}` writes \"{text}\", which reads as text for the user; take it from a \
             string resource, or add where it is to TEXT_ALLOWED with why it is never shown \
             (L10N-01)"
        )));
    }
    violations.extend(number_violations(file, source));
    violations
}

/// Whether `literal` is for developers: in a log, a thread's name, an exception, a panic or an
/// assertion, or a log's tag.
fn for_developers(literal: &Literal) -> bool {
    literal.name.as_deref() == Some("TAG")
        || literal.calls.iter().any(|call| {
            let part = last_part(call);
            call.starts_with("Log.")
                || FOR_DEVELOPERS.contains(&part)
                || FOR_DEVELOPERS.contains(&call.as_str())
                || (call.starts_with("new ")
                    && (part.ends_with("Exception") || part.ends_with("Error")))
        })
}

/// Where `file`'s Java `source` puts a number into a text sink in ASCII digits (L10N-03).
pub fn number_violations(file: &str, source: &str) -> Vec<Violation> {
    let code = java_code_only(source);
    let mut violations = Vec::new();
    for sink in TEXT_SINKS {
        for (at, _) in code.match_indices(sink) {
            if word_before(&code, at)
                || code[at + sink.len()..]
                    .chars()
                    .next()
                    .is_some_and(is_word_char)
            {
                continue;
            }
            let Some(arguments) = code[at + sink.len()..]
                .trim_start()
                .strip_prefix('(')
                .map(|inside| &inside[..closing_paren(inside)])
            else {
                continue;
            };
            let mut found: Vec<&str> = NUMBER_TO_TEXT
                .iter()
                .copied()
                .filter(|call| {
                    arguments
                        .match_indices(call)
                        .any(|(k, _)| !word_before(arguments, k))
                })
                .collect();
            for (k, _) in arguments.match_indices("String.format") {
                let rest = &arguments[k + "String.format".len()..];
                let inner = rest
                    .trim_start()
                    .strip_prefix('(')
                    .map_or("", |inside| &inside[..closing_paren(inside)]);
                if !word_before(arguments, k) && !inner.contains("Locale") {
                    found.push("String.format without a Locale");
                }
            }
            for call in found {
                violations.push(Violation(format!(
                    "`{file}:{}` gives `{sink}` a number through `{call}`, which writes ASCII \
                     digits whatever the language; use `NumberFormat` or a string resource's \
                     `%d` (L10N-03)",
                    line_of(&code, at)
                )));
            }
        }
    }
    violations
}

/// Where the Java `sources`, as `(file, text)`, write text for the user or a number in ASCII
/// digits (L10N-01, L10N-03), and which entries of `allowed` match nothing. Only the source
/// sets that ship are checked ([`SHIPPED_SOURCE_SETS`]).
pub fn java_violations(
    sources: &[(String, String)],
    allowed: &[(&str, &str, &str)],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut used = BTreeSet::new();
    for (file, source) in sources {
        if !ships(file) {
            continue;
        }
        violations.extend(java_file_violations(file, source, allowed, &mut used));
    }
    for (file, what, _) in allowed {
        if !used.contains(&(*file, *what)) {
            violations.push(Violation(format!(
                "TEXT_ALLOWED lets `{file}` write words in `{what}`, but it doesn't any more; \
                 remove the entry"
            )));
        }
    }
    violations
}

/// Whether the Java `file` is in a source set that ships.
fn ships(file: &str) -> bool {
    SHIPPED_SOURCE_SETS
        .iter()
        .any(|set| file.starts_with(&format!("{ANDROID_SOURCES}/{set}/")))
}

/// Where the Rust `sources`, as `(file, text)`, write prose outside tests and fakes (L10N-01),
/// and which entries of `allowed` match nothing.
pub fn rust_violations(
    sources: &[(String, String)],
    allowed: &[(&str, &str, &str)],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut used = BTreeSet::new();
    for (file, source) in sources {
        if is_test_or_fake(file) {
            continue;
        }
        for literal in rust_literals(source) {
            if literal.in_attribute || !reads_as_prose(&literal.text) || for_developers(&literal) {
                continue;
            }
            if let Some((path, what, _)) = allowed
                .iter()
                .find(|(path, what, _)| *path == file && literal.is_in(what))
            {
                used.insert((*path, *what));
                continue;
            }
            violations.push(Violation(format!(
                "`{file}:{}` writes \"{}\", which reads as words; the Rust app writes no text for \
                 the user: give it a `Problem`, or let the host word it from a string resource, \
                 or add where it is to PROSE_ALLOWED with why it is never shown (L10N-01)",
                literal.line,
                shown(&literal.text)
            )));
        }
    }
    for (file, what, _) in allowed {
        if !used.contains(&(*file, *what)) {
            violations.push(Violation(format!(
                "PROSE_ALLOWED lets `{file}` write prose in `{what}`, but it doesn't any more; \
                 remove the entry"
            )));
        }
    }
    violations
}

/// Whether the Rust `file` holds only tests or fakes.
fn is_test_or_fake(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    name == "tests.rs" || name == "fakes.rs" || file.split('/').any(|part| part == "tests")
}

/// `text` on one line and at most 60 characters, for a message.
fn shown(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > 60 {
        format!("{}…", one_line.chars().take(59).collect::<String>())
    } else {
        one_line
    }
}

/// The attributes of an XML layout or of the manifest that hold text for the user.
const TEXT_ATTRIBUTES: [&str; 8] = [
    "android:label",
    "android:description",
    "android:text",
    "android:hint",
    "android:contentDescription",
    "android:title",
    "android:summary",
    "android:tooltipText",
];

/// Where `file`'s XML, a layout or the manifest, gives an attribute of [`TEXT_ATTRIBUTES`] text
/// rather than a resource (L10N-01).
pub fn xml_violations(file: &str, xml: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    for attribute in TEXT_ATTRIBUTES {
        let key = format!("{attribute}=\"");
        for (at, _) in xml.match_indices(&key) {
            let value = &xml[at + key.len()..];
            let value = &value[..value.find('"').unwrap_or(value.len())];
            if !value.is_empty() && !value.starts_with(['@', '?']) {
                violations.push(Violation(format!(
                    "`{file}:{}` sets `{attribute}` to \"{}\"; use a string resource, \
                     `@string/…` (L10N-01)",
                    line_of(xml, at),
                    shown(value)
                )));
            }
        }
    }
    violations
}

/// Every XML file the app ships other than its text resources, relative to `root`: each
/// source set's `AndroidManifest.xml`, and its `res/*/*.xml` outside `values*`.
pub fn xml_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = Vec::new();
    for source_set in directories(&root.join(ANDROID_SOURCES))? {
        let manifest = source_set.join("AndroidManifest.xml");
        if manifest.is_file() {
            files.push(manifest);
        }
        for resources in directories(&source_set.join("res"))? {
            let name = resources.file_name().unwrap_or_default().to_string_lossy();
            if name.split('-').next() != Some("values") {
                files_under(&resources, &mut files)?;
            }
        }
    }
    Ok(files
        .iter()
        .filter(|file| file.extension().is_some_and(|extension| extension == "xml"))
        .map(|file| relative(root, file))
        .collect())
}

/// Every Java file of the source sets that ship, relative to `root`.
pub fn shipped_java_files(root: &Path) -> Result<BTreeSet<String>, String> {
    Ok(java_files(root)?
        .into_iter()
        .filter(|file| ships(file))
        .collect())
}

/// Every Rust file of the app, in `crates/` and the root package's `src/`, relative to `root`.
pub fn app_rust_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = Vec::new();
    for dir in ["crates", "src"] {
        rust_files(&root.join(dir), &mut files)?;
    }
    Ok(files.iter().map(|file| relative(root, file)).collect())
}

/// Each code `problem.rs` gives a `Problem`, from its `code` arms: `Self::NoConnection =>
/// "no_connection",`.
pub fn rust_problem_codes(problem_rs: &str) -> BTreeSet<String> {
    problem_rs
        .lines()
        .filter_map(|line| {
            let (_, code) = line.trim().strip_prefix("Self::")?.split_once(" => \"")?;
            Some(code.strip_suffix("\",")?.to_owned())
        })
        .collect()
}

/// `Problems.java`'s constants, as `NAME → code`, and its cases, as `NAME → resource`.
pub fn java_problems(problems_java: &str) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let mut constants = BTreeMap::new();
    let mut cases = BTreeMap::new();
    let mut case: Option<String> = None;
    for line in problems_java.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("static final String ") {
            if let Some((name, value)) = rest.split_once(" = \"") {
                if let Some(code) = value.strip_suffix("\";") {
                    constants.insert(name.to_owned(), code.to_owned());
                }
            }
        } else if let Some(name) = line.strip_prefix("case ").and_then(|l| l.strip_suffix(':')) {
            case = Some(name.to_owned());
        } else if let Some(resource) = line
            .strip_prefix("return R.string.")
            .and_then(|l| l.strip_suffix(';'))
        {
            if let Some(name) = case.take() {
                cases.insert(name, resource.to_owned());
            }
        }
    }
    (constants, cases)
}

/// The names of the `<string>` resources in `xml` that start with `prefix`.
pub fn string_names(xml: &str, prefix: &str) -> BTreeSet<String> {
    xml.split("<string name=\"")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(name, _)| name))
        .filter(|name| name.starts_with(prefix))
        .map(str::to_owned)
        .collect()
}

/// Where the `Problem` codes in Rust, `Problems.java`'s constants and cases, and the
/// `problem_` string resources disagree (L10N-01).
pub fn problem_violations(problem_rs: &str, problems_java: &str, strings: &str) -> Vec<Violation> {
    let rust = rust_problem_codes(problem_rs);
    let (constants, cases) = java_problems(problems_java);
    let resources: BTreeSet<String> = string_names(strings, "problem_")
        .into_iter()
        .map(|name| name["problem_".len()..].to_owned())
        .collect();
    let java: BTreeSet<String> = constants.values().cloned().collect();
    let mut violations = Vec::new();
    for code in rust.difference(&java) {
        violations.push(Violation(format!(
            "the Problem `{code}` has no constant in Problems.java; add it, with its case"
        )));
    }
    for code in java.difference(&rust) {
        violations.push(Violation(format!(
            "Problems.java has `{code}`, which no Problem in problem.rs has; remove it, or add \
             the Problem"
        )));
    }
    for code in rust.difference(&resources) {
        violations.push(Violation(format!(
            "the Problem `{code}` has no string resource `problem_{code}`; add one, with a \
             comment for translators"
        )));
    }
    for code in resources.difference(&rust) {
        violations.push(Violation(format!(
            "the string resource `problem_{code}` is no Problem's; remove it, or add the Problem"
        )));
    }
    for (name, code) in &constants {
        match cases.get(name) {
            None => violations.push(Violation(format!(
                "Problems.java has no case for `{name}`, so `{code}` reads as a system failure"
            ))),
            Some(resource) if *resource != format!("problem_{code}") => {
                violations.push(Violation(format!(
                    "Problems.java words `{name}` (`{code}`) with `{resource}`, not \
                     `problem_{code}`"
                )));
            }
            Some(_) => {}
        }
    }
    violations
}

/// Where `file`'s text resources can't be translated well: a `<string>` or `<plurals>` that no
/// comment for translators leads its group, a string with several arguments that doesn't number
/// them, or one other than `app_name` marked untranslatable (L10N-01).
pub fn resource_violations(file: &str, xml: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut commented = false;
    let mut in_comment = false;
    for (index, line) in xml.lines().enumerate() {
        let number = index + 1;
        let trimmed = line.trim();
        if in_comment || trimmed.starts_with("<!--") {
            in_comment = !trimmed.contains("-->");
            commented = true;
            continue;
        }
        if trimmed.is_empty() {
            commented = false;
            continue;
        }
        let element = if trimmed.starts_with("<string ") {
            "string"
        } else if trimmed.starts_with("<plurals ") {
            "plurals"
        } else {
            ""
        };
        if !element.is_empty() {
            let name = trimmed
                .split("name=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .unwrap_or("?");
            if !commented {
                violations.push(Violation(format!(
                    "`{file}:{number}`: the {element} `{name}` has no comment for translators \
                     saying where it shows and what its arguments are; add one before it or its \
                     group (L10N-01)"
                )));
            }
            if trimmed.contains("translatable=\"false\"") && name != "app_name" {
                violations.push(Violation(format!(
                    "`{file}:{number}`: `{name}` is marked untranslatable; only the product's \
                     name is (L10N-01, LEGAL-01)"
                )));
            }
        }
        if trimmed.starts_with("<string ") || trimmed.starts_with("<item ") {
            let arguments = format_arguments(trimmed);
            if arguments.len() > 1 && arguments.iter().any(|numbered| !numbered) {
                violations.push(Violation(format!(
                    "`{file}:{number}` has {} arguments but doesn't number each, as `%1$s`, so a \
                     translation can't reorder them (L10N-01)",
                    arguments.len()
                )));
            }
        }
    }
    violations
}

/// The format arguments in `text`, each as whether it is numbered (`%1$s`) or not (`%s`).
fn format_arguments(text: &str) -> Vec<bool> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            if chars.get(i + 1) == Some(&'%') {
                i += 2;
                continue;
            }
            let mut k = i + 1;
            while k < chars.len() && chars[k].is_ascii_digit() {
                k += 1;
            }
            let numbered = k > i + 1 && chars.get(k) == Some(&'$');
            if numbered {
                k += 1;
            }
            while k < chars.len() && (chars[k].is_ascii_digit() || matches!(chars[k], '.' | '-')) {
                k += 1;
            }
            if chars
                .get(k)
                .is_some_and(|c| matches!(c, 's' | 'd' | 'f' | 'x' | 'c'))
            {
                found.push(numbered);
                i = k + 1;
                continue;
            }
        }
        i += 1;
    }
    found
}

/// Reads `path` under `root`.
pub fn read(root: &Path, path: &str) -> String {
    fs::read_to_string(root.join(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(files: &[(&str, &str)]) -> Vec<(String, String)> {
        files
            .iter()
            .map(|(file, text)| ((*file).to_owned(), (*text).to_owned()))
            .collect()
    }

    const MAIN: &str = "android/app/src/main/java/A.java";
    const DEBUG: &str = "android/app/src/debug/java/A.java";

    fn messages(violations: &[Violation]) -> Vec<&str> {
        violations.iter().map(|v| v.0.as_str()).collect()
    }

    #[test]
    fn words_and_identifiers_are_told_apart() {
        for words in [
            "Hello",
            "MeeGone",
            "Mail can't open",
            "two words",
            "Hm",
            "Ehm",
        ] {
            assert!(reads_as_words(words), "{words}");
        }
        for not in [
            "android.appInfo",
            "alertTitle",
            "io.github.twonet.meegone.appsScroll",
            "android.settings.WALLPAPER_SETTINGS",
            "EEEE",
            "MMMMdHm",
            "\\p{Cf}+",
            "%1$s · %2$s",
            "ok",
            "no_connection",
            "\u{200B}",
        ] {
            assert!(!reads_as_words(not), "{not}");
        }
        assert!(reads_as_prose("The system couldn't do that"));
        assert!(!reads_as_prose("no_connection"));
        assert!(!reads_as_prose("MeeGone"));
        assert!(!reads_as_prose("{error}: {other}"));
    }

    #[test]
    fn java_literals_know_their_calls_and_names() {
        let source = "class A {\n\
                      static final String TAG = \"MeeGone\";\n\
                      void f() {\n\
                      view.setText(res.getString(R.string.x, \"Hello\"));\n\
                      Log.w(TAG, \"Couldn't do it: \" + e);\n\
                      skeleton = hour24 ? \"Hm\" : \"hm\";\n\
                      Thread t = new Thread(task, \"MeeGone icons\");\n\
                      char q = '\"'; String s = \"\"\"\n  Block\n  \"\"\";\n\
                      }\n}\n";

        let found = java_literals(source);

        let summary: Vec<(usize, &str, Vec<&str>, Option<&str>)> = found
            .iter()
            .map(|l| {
                (
                    l.line,
                    l.text.as_str(),
                    l.calls.iter().map(String::as_str).collect(),
                    l.name.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (2, "MeeGone", vec![], Some("TAG")),
                (4, "Hello", vec!["res.getString", "view.setText"], None),
                (5, "Couldn't do it: ", vec!["Log.w"], None),
                (6, "Hm", vec![], Some("skeleton")),
                (6, "hm", vec![], Some("skeleton")),
                (7, "MeeGone icons", vec!["new Thread"], None),
                (8, "\n  Block\n  ", vec![], Some("s")),
            ]
        );
    }

    #[test]
    fn text_in_a_sink_is_found_with_no_way_to_allow_it() {
        let source = "void f() {\n\
                      label.setText(\"Hello\");\n\
                      node.addAction(new AccessibilityAction(ID, \"Open\"));\n\
                      label.setText(\"\");\n\
                      label.setText(R.string.hello);\n}\n";
        let allowed = [(MAIN, "setText", "no")];

        let violations = java_violations(&sources(&[(MAIN, source)]), &allowed);

        let found = messages(&violations);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].contains("`android/app/src/main/java/A.java:2` gives `label.setText`"));
        assert!(found[1].contains(":3` gives `new AccessibilityAction`"));
        assert!(found[2].contains("TEXT_ALLOWED lets"));
    }

    #[test]
    fn words_elsewhere_are_found_unless_for_developers_or_allowed() {
        let source = "class A {\n\
                      static final String TAG = \"MeeGone\";\n\
                      static final String GREETING = \"Hello\";\n\
                      static final String AGENT = \"MeeGone\";\n\
                      void f() {\n\
                      Log.i(TAG, \"Asked for the home role\");\n\
                      throw new IllegalStateException(\"Not started\");\n\
                      String key = \"android.appInfo\";\n\
                      }\n}\n";
        let allowed = [(MAIN, "AGENT", "a header")];

        let violations = java_violations(&sources(&[(MAIN, source), (DEBUG, source)]), &allowed);

        let found = messages(&violations);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains(":3` writes \"Hello\""));
        assert!(found[0].contains("L10N-01"));
    }

    #[test]
    fn numbers_in_ascii_digits_for_a_sink_are_found() {
        let source = "void f() {\n\
                      count.setText(Integer.toString(n));\n\
                      count.setText(String.format(\"%d\", n));\n\
                      count.setText(String.format(Locale.getDefault(), \"%d\", n));\n\
                      count.setText(NumberFormat.getIntegerInstance().format(n));\n\
                      String key = Integer.toString(n);\n}\n";

        let violations = number_violations("A.java", source);

        let found = messages(&violations);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("`A.java:2`") && found[0].contains("Integer.toString"));
        assert!(found[1].contains("`A.java:3`") && found[1].contains("without a Locale"));
    }

    #[test]
    fn rust_literals_skip_tests_byte_strings_and_comments() {
        let source = "//! \"Docs with words\"\n\
                      fn f() -> Result<(), String> {\n\
                      \x20   let a = r#\"Raw words\"#;\n\
                      \x20   let b = b\"bytes here\";\n\
                      \x20   let c = 'x'; let d = '\\'';\n\
                      \x20   #[cfg(test)]\n\
                      \x20   if crash { return Err(\"crashed while writing\".into()); }\n\
                      \x20   Err(format!(\"Couldn't pass it: {e}\"))\n\
                      }\n\
                      #[allow(dead_code, reason = \"kept for later use\")]\n\
                      const X: &str = r\"Plain\\swords\";\n\
                      #[cfg(test)]\n\
                      mod tests {\n\
                      \x20   const Y: &str = \"Test words\";\n\
                      }\n";

        let found = rust_literals(source);

        type Summary<'a> = (
            usize,
            &'a str,
            Vec<&'a str>,
            Option<&'a str>,
            bool,
            Option<&'a str>,
        );
        let summary: Vec<Summary> = found
            .iter()
            .map(|l| {
                (
                    l.line,
                    l.text.as_str(),
                    l.calls.iter().map(String::as_str).collect(),
                    l.name.as_deref(),
                    l.in_attribute,
                    l.function.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (3, "Raw words", vec![], Some("a"), false, Some("f")),
                (
                    8,
                    "Couldn't pass it: {e}",
                    vec!["format!", "Err"],
                    None,
                    false,
                    Some("f")
                ),
                (
                    10,
                    "kept for later use",
                    vec!["allow"],
                    Some("reason"),
                    true,
                    None
                ),
                (11, r"Plain\swords", vec![], Some("X"), false, None),
            ]
        );
    }

    #[test]
    fn rust_prose_is_found_unless_for_developers_or_allowed() {
        let source = "fn f() {\n\
                      \x20   let x = y.expect(\"the layout is ready\");\n\
                      \x20   assert_eq!(a, b, \"two things differ\");\n\
                      \x20   let p = Pairing { what: \"Body text on the background\" };\n\
                      \x20   problem = \"No connection\";\n\
                      \x20   let code = \"no_connection\";\n\
                      }\n";
        let files = sources(&[
            ("crates/a/src/lib.rs", source),
            ("crates/a/src/fakes.rs", source),
            ("crates/a/src/tests.rs", source),
            ("tests/acceptance.rs", source),
        ]);
        let allowed = [
            ("crates/a/src/lib.rs", "what", "documents a pairing"),
            ("crates/a/src/lib.rs", "never", "matches nothing"),
        ];

        let violations = rust_violations(&files, &allowed);

        let found = messages(&violations);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("`crates/a/src/lib.rs:5` writes \"No connection\""));
        assert!(
            found[1].contains("PROSE_ALLOWED lets `crates/a/src/lib.rs` write prose in `never`")
        );
    }

    #[test]
    fn layout_and_manifest_text_must_be_a_resource() {
        let xml = "<application\n    android:label=\"@string/app_name\"\n    \
                   android:description=\"An app\">\n<TextView android:text=\"Hello\" \
                   android:hint=\"?attr/hint\" />\n";

        let found = xml_violations("AndroidManifest.xml", xml);

        let found = messages(&found);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("`AndroidManifest.xml:3` sets `android:description`"));
        assert!(found[1].contains("`AndroidManifest.xml:4` sets `android:text`"));
    }

    #[test]
    fn problem_codes_must_agree_everywhere() {
        let rust = "    pub const fn code(self) -> &'static str {\n        match self {\n\
                    \x20           Self::NoConnection => \"no_connection\",\n\
                    \x20           Self::SystemFailed => \"system_failed\",\n\
                    \x20           Self::Fresh => \"fresh\",\n";
        let java = "    static final String NO_CONNECTION = \"no_connection\";\n\
                    \x20   static final String SYSTEM_FAILED = \"system_failed\";\n\
                    \x20   static final String OLD = \"old\";\n\
                    \x20           case NO_CONNECTION:\n\
                    \x20               return R.string.problem_no_connection;\n\
                    \x20           case SYSTEM_FAILED:\n\
                    \x20               return R.string.problem_no_connection;\n";
        let strings = "<string name=\"problem_no_connection\">No connection</string>\n\
                       <string name=\"problem_system_failed\">Failed</string>\n\
                       <string name=\"problem_stale\">Stale</string>\n\
                       <string name=\"other\">Other</string>\n";

        let violations = problem_violations(rust, java, strings);

        let found = messages(&violations);
        assert_eq!(found.len(), 6, "{found:?}");
        assert!(found[0].contains("`fresh` has no constant"));
        assert!(found[1].contains("Problems.java has `old`"));
        assert!(found[2].contains("`fresh` has no string resource"));
        assert!(found[3].contains("`problem_stale` is no Problem's"));
        assert!(found[4].contains("no case for `OLD`"));
        assert!(
            found[5]
                .contains("words `SYSTEM_FAILED` (`system_failed`) with `problem_no_connection`")
        );
    }

    #[test]
    fn resources_need_comments_numbered_arguments_and_to_be_translatable() {
        let xml = "<resources>\n\
                   \x20   <!-- The product's name. -->\n\
                   \x20   <string name=\"app_name\" translatable=\"false\">MeeGone</string>\n\
                   \n\
                   \x20   <!-- Two views,\n\
                   \x20        one comment. -->\n\
                   \x20   <string name=\"a\">A</string>\n\
                   \x20   <plurals name=\"b\">\n\
                   \x20       <item quantity=\"other\">%d of %d</item>\n\
                   \x20   </plurals>\n\
                   \n\
                   \x20   <string name=\"c\" translatable=\"false\">%1$s, %2$s 100%%</string>\n\
                   </resources>\n";

        let found = resource_violations("strings.xml", xml);

        let found = messages(&found);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].contains("`strings.xml:9` has 2 arguments but doesn't number each"));
        assert!(found[1].contains("`strings.xml:12`: the string `c` has no comment"));
        assert!(found[2].contains("`c` is marked untranslatable"));
    }
}
