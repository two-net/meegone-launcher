//! A small, strict JSON reader (RFC 8259) for the image feed's responses, which are untrusted
//! (SEC-02, ADR 0027).
//!
//! It accepts exactly the grammar: no comments, trailing commas, single quotes, byte order mark,
//! `NaN` or leading zeros. The text must be valid UTF-8, strings may hold no raw control
//! characters, every escape must be valid, surrogate pairs included, an object may not repeat a
//! key, and nothing may follow the value. Nesting stops at [`MAX_DEPTH`], so no response can
//! exhaust the stack; the caller bounds the input's size. Numbers are kept as their text, since
//! the feed's are never read.
//!
//! It never panics: anything else is `None`.

use std::collections::HashSet;

/// How deeply arrays and objects may nest.
pub(crate) const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    /// A number, as written.
    Number(String),
    String(String),
    Array(Vec<Value>),
    /// Members in the order written, each key once.
    Object(Vec<(String, Value)>),
}

impl Value {
    /// The member `key` of an object.
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Object(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }
}

/// The JSON text in `bytes`, or `None` if it isn't exactly one valid JSON value.
pub(crate) fn parse(bytes: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = parser.value(0)?;
    parser.whitespace();
    (parser.at == parser.bytes.len()).then_some(value)
}

/// Reads valid UTF-8 byte by byte. It stops only at ASCII bytes, so every slice it takes is
/// on character boundaries.
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.at += 1;
        Some(byte)
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn literal(&mut self, word: &[u8]) -> Option<()> {
        let end = self.at.checked_add(word.len())?;
        (self.bytes.get(self.at..end)? == word).then(|| self.at = end)
    }

    fn text(&self, start: usize) -> Option<&str> {
        std::str::from_utf8(self.bytes.get(start..self.at)?).ok()
    }

    /// A value at `depth` arrays and objects deep.
    fn value(&mut self, depth: usize) -> Option<Value> {
        self.whitespace();
        match self.peek()? {
            b'{' => self.object(depth + 1),
            b'[' => self.array(depth + 1),
            b'"' => self.string().map(Value::String),
            b't' => self.literal(b"true").map(|()| Value::Bool(true)),
            b'f' => self.literal(b"false").map(|()| Value::Bool(false)),
            b'n' => self.literal(b"null").map(|()| Value::Null),
            b'-' | b'0'..=b'9' => self.number().map(Value::Number),
            _ => None,
        }
    }

    fn object(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.at += 1;
        let mut members = Vec::new();
        let mut keys = HashSet::new();
        self.whitespace();
        if self.peek()? == b'}' {
            self.at += 1;
            return Some(Value::Object(members));
        }
        loop {
            self.whitespace();
            if self.peek()? != b'"' {
                return None;
            }
            let key = self.string()?;
            if !keys.insert(key.clone()) {
                return None;
            }
            self.whitespace();
            if self.next()? != b':' {
                return None;
            }
            members.push((key, self.value(depth)?));
            self.whitespace();
            match self.next()? {
                b',' => {}
                b'}' => return Some(Value::Object(members)),
                _ => return None,
            }
        }
    }

    fn array(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.at += 1;
        let mut items = Vec::new();
        self.whitespace();
        if self.peek()? == b']' {
            self.at += 1;
            return Some(Value::Array(items));
        }
        loop {
            items.push(self.value(depth)?);
            self.whitespace();
            match self.next()? {
                b',' => {}
                b']' => return Some(Value::Array(items)),
                _ => return None,
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        self.at += 1;
        let mut text = String::new();
        loop {
            let start = self.at;
            while self
                .peek()
                .is_some_and(|byte| byte != b'"' && byte != b'\\' && byte >= 0x20)
            {
                self.at += 1;
            }
            text.push_str(self.text(start)?);
            match self.next()? {
                b'"' => return Some(text),
                b'\\' => text.push(self.escape()?),
                // A raw control character.
                _ => return None,
            }
        }
    }

    /// The character an escape stands for, after its backslash.
    fn escape(&mut self) -> Option<char> {
        Some(match self.next()? {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => match self.hex4()? {
                high @ 0xD800..=0xDBFF => {
                    self.literal(b"\\u")?;
                    let low = self.hex4()?;
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return None;
                    }
                    char::from_u32(0x1_0000 + ((high - 0xD800) << 10) + (low - 0xDC00))?
                }
                unit => char::from_u32(unit)?,
            },
            _ => return None,
        })
    }

    /// Four hexadecimal digits, as a UTF-16 code unit.
    fn hex4(&mut self) -> Option<u32> {
        let end = self.at.checked_add(4)?;
        let digits = self.bytes.get(self.at..end)?;
        if !digits.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        self.at = end;
        u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
    }

    /// `-? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?`
    fn number(&mut self) -> Option<String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.next()? {
            b'0' => {}
            b'1'..=b'9' => {
                self.digits();
            }
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return None;
            }
        }
        self.text(start).map(str::to_owned)
    }

    /// Skips decimal digits, and says how many.
    fn digits(&mut self) -> usize {
        let start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        self.at - start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(json: &str) -> Option<Value> {
        parse(json.as_bytes())
    }

    fn string(json: &str) -> Option<String> {
        text(json)?.as_str().map(str::to_owned)
    }

    #[test]
    fn every_kind_of_value_is_read() {
        let value = text(
            " {\"a\": [1, -2.5e+3, 0, 0.5, 1E-2, true, false, null, \"x\"], \"b\": {}, \"c\": []}\r\n",
        )
        .unwrap();

        assert_eq!(
            value.get("a").and_then(Value::as_array).unwrap(),
            [
                Value::Number("1".into()),
                Value::Number("-2.5e+3".into()),
                Value::Number("0".into()),
                Value::Number("0.5".into()),
                Value::Number("1E-2".into()),
                Value::Bool(true),
                Value::Bool(false),
                Value::Null,
                Value::String("x".into()),
            ]
        );
        assert_eq!(value.get("b"), Some(&Value::Object(Vec::new())));
        assert_eq!(value.get("c").and_then(Value::as_array), Some(&[][..]));
        assert_eq!(value.get("d"), None);
    }

    #[test]
    fn escapes_and_unicode_are_decoded() {
        assert_eq!(
            string(r#""\"\\\/\b\f\n\r\té© 🦉""#).as_deref(),
            Some("\"\\/\u{8}\u{c}\n\r\té© 🦉")
        );
        assert_eq!(
            string("\"メンフクロウ (© Zahoor)\"").as_deref(),
            Some("メンフクロウ (© Zahoor)")
        );
    }

    #[test]
    fn accessors_answer_only_for_their_own_kind() {
        let value = text(r#"{"s": "x", "b": true, "a": []}"#).unwrap();
        let (s, b, a) = (
            value.get("s").unwrap(),
            value.get("b").unwrap(),
            value.get("a").unwrap(),
        );

        assert_eq!(
            (s.as_str(), s.as_bool(), s.as_array()),
            (Some("x"), None, None)
        );
        assert_eq!(
            (b.as_str(), b.as_bool(), b.as_array()),
            (None, Some(true), None)
        );
        assert_eq!(a.as_str(), None);
        assert_eq!(s.get("x"), None);
    }

    #[test]
    fn anything_but_exact_json_is_rejected() {
        // SEC-02: strict parsing.
        for json in [
            "",
            " ",
            "{",
            "}",
            "[1,]",
            "[,1]",
            "[1 2]",
            r#"{"a":1,}"#,
            r#"{"a" 1}"#,
            r#"{a:1}"#,
            "{'a':1}",
            r#"{"a":1,"a":2}"#,
            "[01]",
            "[-01]",
            "[1.]",
            "[.5]",
            "[1e]",
            "[1e+]",
            "[-]",
            "[+1]",
            "[NaN]",
            "[Infinity]",
            "[tru]",
            "[nul]",
            "[True]",
            "\"\u{1}\"",
            "\"a\nb\"",
            r#""\x""#,
            r#""\u12""#,
            r#""\u12G4""#,
            r#""\u+123""#,
            r#""\ud800""#,
            r#""\udc00""#,
            r#""\ud800A""#,
            r#""\ud800x""#,
            "\"open",
            "[1] 2",
            "[] /* comment */",
            "// comment\n[]",
            "\u{feff}[]",
            "\u{a0}[]",
        ] {
            assert_eq!(text(json), None, "{json:?}");
        }
        assert_eq!(parse(&[b'"', 0xFF, b'"']), None);
        assert_eq!(parse(&[b'"', 0xC3, b'"']), None);
    }

    #[test]
    fn nesting_stops_at_the_limit_without_overflowing() {
        let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        assert!(text(&nested(MAX_DEPTH)).is_some());
        assert_eq!(text(&nested(MAX_DEPTH + 1)), None);
        assert_eq!(
            text(&format!(
                "{}1{}",
                r#"{"a":"#.repeat(MAX_DEPTH + 1),
                "}".repeat(MAX_DEPTH + 1)
            )),
            None
        );

        // Far deeper than any stack could take, if it were followed.
        assert_eq!(text(&"[".repeat(1_000_000)), None);
    }
}
