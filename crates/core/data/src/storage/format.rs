//! The format every store saves in (REL-03, MAINT-04; ADR 0004).
//!
//! A saved file is UTF-8 text:
//!
//! ```text
//! meegone-settings 1
//! theme dark
//! hidden 0 com.example.mail com.example.mail.Main
//! end 9c1f0e2a7b3d4f51
//! ```
//!
//! - **Header**: the format's kind and the version it was written in. The kind keeps a file from
//!   being read by the wrong store.
//! - **Body**: one record per line, a key and then fields separated by single spaces. Strings
//!   are escaped, so a field never holds a space or a line break.
//! - **Trailer**: `end` and a checksum of everything before it. A file that was cut short has no
//!   trailer, and a damaged one fails the checksum. Either is unreadable.
//!
//! A body saved by an older version is brought up to date by the format's [`Migration`]s, one
//! version at a time, before it is parsed, so only the current version needs a parser. A file
//! that is unreadable or comes from a newer version of MeeGone loads as the defaults (MAINT-04);
//! after a downgrade, the newer data is replaced the next time the store saves.

use std::borrow::Cow;
use std::fmt::Display;
use std::iter::Peekable;
use std::str::{FromStr, Split, SplitTerminator};

use super::{StorageAdapter, StoredFile};
use crate::model::{AppId, DataError, ProfileId};
use crate::problem::Problem;

/// Turns a body saved at one version into the body of the next version, or `None` if it can't.
pub type Migration = fn(body: &str) -> Option<String>;

/// A value saved in a file of its own.
pub trait Format: Default {
    /// Names the format in the header, such as `settings`. A plain word.
    const KIND: &'static str;
    /// The version [`Format::write`] writes, starting at 1. The body changes only together with
    /// a new version, its migration and its fixture (ARCHITECTURE.md, "Persistence").
    const VERSION: u32;
    /// `MIGRATIONS[i]` turns a version `i + 1` body into a version `i + 2` one. Declare it as
    /// `[Migration; (VERSION - 1) as usize]`, so a version without its migration doesn't compile.
    const MIGRATIONS: &'static [Migration];

    /// Writes the body at [`Format::VERSION`].
    fn write(&self, out: &mut Writer);
    /// Reads a body at [`Format::VERSION`], or `None` if it isn't valid.
    fn read(input: &mut Reader<'_>) -> Option<Self>;
}

/// The file contents that save `value`.
pub fn encode<F: Format>(value: &F) -> Vec<u8> {
    let mut out = Writer::default();
    value.write(&mut out);
    seal(F::KIND, F::VERSION, &out.into_body())
}

/// The value saved in `bytes`, brought up to the current version. `None` if `bytes` are
/// damaged, cut short, of another kind, from a newer version, or not a valid body.
pub fn decode<F: Format>(bytes: &[u8]) -> Option<F> {
    let (version, body) = open(F::KIND, bytes)?;
    let body = migrate(version, F::VERSION, F::MIGRATIONS, body)?;
    let mut input = Reader::new(&body);
    let value = F::read(&mut input)?;
    input.is_done().then_some(value)
}

/// What a store's file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored<F> {
    /// Nothing has been saved yet.
    Missing,
    Read(F),
    /// The file is damaged, cut short, of another kind or from a newer version. Its bytes are
    /// kept, so a store can [`salvage`] what it can (REL-02).
    Unreadable(Vec<u8>),
}

/// What is saved in `file`. Fails only when the storage itself can't be read.
pub fn read<F: Format>(
    storage: &dyn StorageAdapter,
    file: StoredFile,
) -> Result<Stored<F>, DataError> {
    Ok(match storage.read(file)? {
        None => Stored::Missing,
        Some(bytes) => match decode(&bytes) {
            Some(value) => Stored::Read(value),
            None => Stored::Unreadable(bytes),
        },
    })
}

/// The saved value, or the defaults if there is none or it can't be read (MAINT-04).
pub fn load<F: Format>(storage: &dyn StorageAdapter, file: StoredFile) -> F {
    match read(storage, file) {
        Ok(Stored::Read(value)) => value,
        Ok(Stored::Missing | Stored::Unreadable(_)) | Err(_) => F::default(),
    }
}

/// The records of an unreadable file that still read on their own, each as a value of its own,
/// in the order they were saved (REL-02). It suits formats whose records stand alone, such as the
/// layout's cells; for others it finds nothing.
///
/// The header must be whole: a file of another kind, or from a newer version, gives nothing, as
/// [`decode`] would (MAINT-04). The checksum is ignored, and a last line without its line break
/// was cut short, so it is dropped. Each record is brought up to date by the format's migrations
/// on its own.
pub fn salvage<F: Format>(bytes: &[u8]) -> Vec<F> {
    let text = String::from_utf8_lossy(bytes);
    let Some((header, body)) = text.split_once('\n') else {
        return Vec::new();
    };
    let version = header
        .strip_prefix(MAGIC)
        .and_then(|rest| rest.strip_prefix(F::KIND))
        .and_then(|rest| rest.strip_prefix(' '))
        .and_then(canonical::<u32>);
    let Some(version) = version.filter(|version| (1..=F::VERSION).contains(version)) else {
        return Vec::new();
    };
    let whole = body.rfind('\n').map_or("", |end| &body[..=end]);
    whole
        .split_terminator('\n')
        .filter(|record| record.split(' ').next() != Some(TRAILER_KEY))
        .filter_map(|record| {
            let record = format!("{record}\n");
            let record = migrate(version, F::VERSION, F::MIGRATIONS, &record)?;
            let mut input = Reader::new(&record);
            let value = F::read(&mut input)?;
            input.is_done().then_some(value)
        })
        .collect()
}

/// Saves `value` atomically (REL-03).
pub fn save<F: Format>(
    storage: &dyn StorageAdapter,
    file: StoredFile,
    value: &F,
) -> Result<(), DataError> {
    storage.write(file, &encode(value))
}

/// Wraps `body` in the header and trailer. Stores use [`encode`]; this is for tests that need a
/// file in another version.
pub fn seal(kind: &str, version: u32, body: &str) -> Vec<u8> {
    let mut text = format!("{MAGIC}{kind} {version}\n{body}");
    let trailer = trailer(text.as_bytes());
    text.push_str(&trailer);
    text.push('\n');
    text.into_bytes()
}

const MAGIC: &str = "meegone-";
/// The key of the trailer, which no record uses.
const TRAILER_KEY: &str = "end";
/// Written for `None`, and never for any other value.
const NONE: &str = "-";

/// The version and body of a sealed file of `kind`, if it is whole and undamaged.
fn open<'a>(kind: &str, bytes: &'a [u8]) -> Option<(u32, &'a str)> {
    let text = std::str::from_utf8(bytes).ok()?.strip_suffix('\n')?;
    let (content, last_line) = text.split_at(text.rfind('\n')? + 1);
    if last_line != trailer(content.as_bytes()) {
        return None;
    }
    let (header, body) = content.split_once('\n')?;
    let version = header
        .strip_prefix(MAGIC)?
        .strip_prefix(kind)?
        .strip_prefix(' ')?;
    Some((canonical(version)?, body))
}

fn trailer(content: &[u8]) -> String {
    format!("{TRAILER_KEY} {:016x}", checksum(content))
}

/// 64-bit FNV-1a. It catches any single changed byte and nearly every other accident; it is not
/// meant to resist tampering.
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Runs the migrations from `version` up to `current`, one step at a time.
fn migrate<'a>(
    version: u32,
    current: u32,
    migrations: &[Migration],
    body: &'a str,
) -> Option<Cow<'a, str>> {
    if version == current {
        return Some(Cow::Borrowed(body));
    }
    if version == 0 || version > current {
        return None;
    }
    let steps = migrations.get(version as usize - 1..current as usize - 1)?;
    steps.iter().try_fold(Cow::Borrowed(body), |body, step| {
        step(&body).map(Cow::Owned)
    })
}

/// `text` as a `T`, if `text` is exactly how `T` writes it.
fn canonical<T: FromStr + Display>(text: &str) -> Option<T> {
    let value: T = text.parse().ok()?;
    (value.to_string() == text).then_some(value)
}

/// Builds a body, one record per line.
#[derive(Debug, Default)]
pub struct Writer {
    body: String,
}

impl Writer {
    /// Adds a record: `key`, a plain word, followed by the fields of `value`.
    pub fn line(&mut self, key: &str, value: impl Encode) -> &mut Self {
        debug_assert!(is_plain(key), "a key is a plain word: {key:?}");
        self.body.push_str(key);
        value.encode(&mut Line(&mut self.body));
        self.body.push('\n');
        self
    }

    /// The body written so far. Migrations use it to write the next version's body.
    pub fn into_body(self) -> String {
        self.body
    }
}

/// The record being written.
pub struct Line<'a>(&'a mut String);

impl Line<'_> {
    /// Adds one field as it is. Only for plain words and numbers; strings are [`Encode`]d.
    pub fn token(&mut self, token: &str) {
        debug_assert!(is_plain(token), "a token is a plain word: {token:?}");
        self.push(token);
    }

    fn push(&mut self, field: &str) {
        self.0.push(' ');
        self.0.push_str(field);
    }
}

/// Non-empty, not [`NONE`], and free of separators and escapes.
fn is_plain(word: &str) -> bool {
    !word.is_empty() && word != NONE && !word.contains(['\\', ' ', '\n', '\r'])
}

/// Reads a body, one record at a time, in the order it was written. Every read is strict: a
/// record with another key, a missing or extra field, or a malformed one gives `None`.
pub struct Reader<'a> {
    records: Peekable<SplitTerminator<'a, char>>,
}

impl<'a> Reader<'a> {
    pub fn new(body: &'a str) -> Self {
        Self {
            records: body.split_terminator('\n').peekable(),
        }
    }

    /// The key of the next record, if there is one.
    pub fn peek_key(&mut self) -> Option<&'a str> {
        let record = self.records.peek()?;
        record.split(' ').next()
    }

    /// The next record, which must have `key` and hold exactly one `T`.
    pub fn line<T: Decode>(&mut self, key: &str) -> Option<T> {
        if self.peek_key()? != key {
            return None;
        }
        let mut tokens = self.records.next()?.split(' ').peekable();
        tokens.next(); // The key.
        let mut fields = Fields { tokens };
        let value = T::decode(&mut fields)?;
        fields.tokens.next().is_none().then_some(value)
    }

    /// Every record from here on that has `key`, each holding exactly one `T`.
    pub fn lines<T: Decode>(&mut self, key: &str) -> Option<Vec<T>> {
        let mut values = Vec::new();
        while self.peek_key() == Some(key) {
            values.push(self.line(key)?);
        }
        Some(values)
    }

    /// Whether every record has been read.
    pub fn is_done(&mut self) -> bool {
        self.records.peek().is_none()
    }
}

/// The fields of the record being read.
pub struct Fields<'a> {
    tokens: Peekable<Split<'a, char>>,
}

impl<'a> Fields<'a> {
    /// The next field as it was written. Only for plain words and numbers; strings are
    /// [`Decode`]d.
    pub fn token(&mut self) -> Option<&'a str> {
        self.tokens.next().filter(|token| !token.is_empty())
    }
}

/// A value written as one or more fields of a record.
pub trait Encode {
    fn encode(&self, line: &mut Line<'_>);
}

/// A value read back from the fields its [`Encode`] wrote.
pub trait Decode: Sized {
    fn decode(fields: &mut Fields<'_>) -> Option<Self>;
}

impl<T: Encode + ?Sized> Encode for &T {
    fn encode(&self, line: &mut Line<'_>) {
        (**self).encode(line);
    }
}

impl Encode for str {
    fn encode(&self, line: &mut Line<'_>) {
        line.push(&escape(self));
    }
}

impl Encode for String {
    fn encode(&self, line: &mut Line<'_>) {
        self.as_str().encode(line);
    }
}

impl Decode for String {
    fn decode(fields: &mut Fields<'_>) -> Option<Self> {
        unescape(fields.token()?)
    }
}

/// Booleans and numbers, written as Rust writes them.
macro_rules! plain {
    ($($t:ty),*) => {$(
        impl Encode for $t {
            fn encode(&self, line: &mut Line<'_>) {
                line.token(&self.to_string());
            }
        }

        impl Decode for $t {
            fn decode(fields: &mut Fields<'_>) -> Option<Self> {
                canonical(fields.token()?)
            }
        }
    )*};
}

plain!(bool, u32, u64);

impl<T: Encode> Encode for Option<T> {
    fn encode(&self, line: &mut Line<'_>) {
        match self {
            None => line.push(NONE),
            Some(value) => value.encode(line),
        }
    }
}

impl<T: Decode> Decode for Option<T> {
    fn decode(fields: &mut Fields<'_>) -> Option<Self> {
        if fields.tokens.peek() == Some(&NONE) {
            fields.tokens.next();
            Some(None)
        } else {
            T::decode(fields).map(Some)
        }
    }
}

/// A list is its length, then its items.
impl<T: Encode> Encode for [T] {
    fn encode(&self, line: &mut Line<'_>) {
        (self.len() as u64).encode(line);
        for item in self {
            item.encode(line);
        }
    }
}

impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, line: &mut Line<'_>) {
        self.as_slice().encode(line);
    }
}

impl<T: Decode> Decode for Vec<T> {
    fn decode(fields: &mut Fields<'_>) -> Option<Self> {
        let len = u64::decode(fields)?;
        (0..len).map(|_| T::decode(fields)).collect()
    }
}

macro_rules! tuple {
    ($($name:ident),+) => {
        impl<$($name: Encode),+> Encode for ($($name,)+) {
            #[allow(non_snake_case)]
            fn encode(&self, line: &mut Line<'_>) {
                let ($($name,)+) = self;
                $($name.encode(line);)+
            }
        }

        impl<$($name: Decode),+> Decode for ($($name,)+) {
            fn decode(fields: &mut Fields<'_>) -> Option<Self> {
                Some(($($name::decode(fields)?,)+))
            }
        }
    };
}

tuple!(A, B);
tuple!(A, B, C);

/// An app is its profile, package and activity.
impl Encode for AppId {
    fn encode(&self, line: &mut Line<'_>) {
        self.profile.0.encode(line);
        self.package.encode(line);
        self.activity.encode(line);
    }
}

impl Decode for AppId {
    fn decode(fields: &mut Fields<'_>) -> Option<Self> {
        Some(AppId {
            profile: ProfileId(u32::decode(fields)?),
            package: String::decode(fields)?,
            activity: String::decode(fields)?,
        })
    }
}

/// A problem is its code (L10N-01, ADR 0040), so it is shown in the language of the day it is
/// read. A code this version doesn't know makes the record unreadable, like any other field.
impl Encode for Problem {
    fn encode(&self, line: &mut Line<'_>) {
        line.token(self.code());
    }
}

impl Decode for Problem {
    fn decode(fields: &mut Fields<'_>) -> Option<Self> {
        Problem::from_code(fields.token()?)
    }
}

/// `text` as one field: never empty, never [`NONE`], and without spaces or line breaks.
fn escape(text: &str) -> Cow<'_, str> {
    match text {
        "" => Cow::Borrowed(r"\e"),
        NONE => Cow::Borrowed(r"\-"),
        _ if is_plain(text) => Cow::Borrowed(text),
        _ => {
            let mut escaped = String::with_capacity(text.len() + 8);
            for c in text.chars() {
                match c {
                    '\\' => escaped.push_str(r"\\"),
                    ' ' => escaped.push_str(r"\s"),
                    '\n' => escaped.push_str(r"\n"),
                    '\r' => escaped.push_str(r"\r"),
                    c => escaped.push(c),
                }
            }
            Cow::Owned(escaped)
        }
    }
}

fn unescape(field: &str) -> Option<String> {
    match field {
        r"\e" => return Some(String::new()),
        r"\-" => return Some(NONE.to_owned()),
        NONE => return None,
        _ => {}
    }
    let mut text = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        text.push(match c {
            '\\' => match chars.next()? {
                '\\' => '\\',
                's' => ' ',
                'n' => '\n',
                'r' => '\r',
                _ => return None,
            },
            c => c,
        });
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakes::{FakeStorage, app_id};

    /// A format with every kind of field.
    #[derive(Debug, Clone, PartialEq, Default)]
    struct Sample {
        name: String,
        note: Option<String>,
        count: u64,
        on: bool,
        apps: Vec<AppId>,
        pairs: Vec<(String, u32)>,
    }

    impl Format for Sample {
        const KIND: &'static str = "sample";
        const VERSION: u32 = 1;
        const MIGRATIONS: &'static [Migration] = &[];

        fn write(&self, out: &mut Writer) {
            out.line("name", &self.name)
                .line("note", &self.note)
                .line("count", self.count)
                .line("on", self.on)
                .line("apps", &self.apps);
            for pair in &self.pairs {
                out.line("pair", pair);
            }
        }

        fn read(input: &mut Reader<'_>) -> Option<Self> {
            Some(Self {
                name: input.line("name")?,
                note: input.line("note")?,
                count: input.line("count")?,
                on: input.line("on")?,
                apps: input.line("apps")?,
                pairs: input.lines("pair")?,
            })
        }
    }

    fn sample() -> Sample {
        Sample {
            name: "Mail and more".into(),
            note: Some("-".into()),
            count: u64::MAX,
            on: true,
            apps: vec![app_id("mail"), app_id("maps")],
            pairs: vec![("".into(), 0), ("two\nlines".into(), 7)],
        }
    }

    fn sealed(body: &str) -> Vec<u8> {
        seal(Sample::KIND, Sample::VERSION, body)
    }

    #[test]
    fn values_come_back_as_they_were_saved() {
        let bytes = encode(&sample());

        assert_eq!(decode::<Sample>(&bytes), Some(sample()));
    }

    #[test]
    fn a_problem_is_saved_as_its_code() {
        // L10N-01: what is saved is the code, never words.
        let mut out = Writer::default();
        out.line("problem", Some(Problem::NoConnection))
            .line("problem", None::<Problem>);
        let body = out.into_body();
        assert_eq!(body, "problem no_connection\nproblem -\n");

        let mut input = Reader::new(&body);
        assert_eq!(input.line("problem"), Some(Some(Problem::NoConnection)));
        assert_eq!(input.line::<Option<Problem>>("problem"), Some(None));
        assert!(input.is_done());

        let mut unknown = Reader::new("problem no_such_problem\n");
        assert_eq!(unknown.line::<Problem>("problem"), None);
    }

    #[test]
    fn awkward_strings_come_back_as_they_were_saved() {
        for name in [
            "",
            "-",
            "--",
            " ",
            "\\",
            r"\e",
            r"\-",
            r"\s",
            "a b",
            "line\nbreak",
            "cr\r",
            "tab\t",
            "ünïcode ✕",
            "end 0000000000000000",
        ] {
            let saved = Sample {
                name: name.into(),
                note: Some(name.into()),
                ..Sample::default()
            };

            assert_eq!(decode::<Sample>(&encode(&saved)), Some(saved), "{name:?}");
        }
    }

    #[test]
    fn a_file_is_a_header_a_body_and_a_checksum() {
        let text = String::from_utf8(encode(&Sample::default())).unwrap();

        assert_eq!(
            text,
            "meegone-sample 1\nname \\e\nnote -\ncount 0\non false\napps 0\n\
             end 34a7f95b69edc6bd\n"
        );
    }

    #[test]
    fn the_checksum_is_64_bit_fnv_1a() {
        // Test vectors from the FNV reference.
        assert_eq!(checksum(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(checksum(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(checksum(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn every_shortened_file_is_unreadable() {
        // REL-03: a file cut short never loads as partial data.
        let bytes = encode(&sample());

        for len in 0..bytes.len() {
            assert_eq!(decode::<Sample>(&bytes[..len]), None, "cut at {len}");
        }
    }

    #[test]
    fn every_changed_bit_makes_the_file_unreadable() {
        let bytes = encode(&sample());

        for i in 0..bytes.len() {
            for bit in 0..8 {
                let mut damaged = bytes.clone();
                damaged[i] ^= 1 << bit;
                assert_eq!(decode::<Sample>(&damaged), None, "byte {i}, bit {bit}");
            }
        }
    }

    #[test]
    fn files_of_another_kind_or_version_are_unreadable() {
        // MAINT-04
        let body = "name a\nnote -\ncount 0\non false\napps 0\n";
        assert!(decode::<Sample>(&sealed(body)).is_some());

        assert_eq!(decode::<Sample>(&seal("other", 1, body)), None);
        assert_eq!(decode::<Sample>(&seal("sample", 2, body)), None);
        assert_eq!(decode::<Sample>(&seal("sample", 0, body)), None);
        assert_eq!(decode::<Sample>(&seal("sample", u32::MAX, body)), None);
        let padded = format!("meegone-sample 01\n{body}");
        let padded = format!("{padded}{}\n", trailer(padded.as_bytes()));
        assert_eq!(decode::<Sample>(padded.as_bytes()), None);
    }

    #[test]
    fn malformed_files_and_bodies_are_unreadable() {
        for bytes in [
            &b""[..],
            b"\n",
            b"meegone-sample 1\n",
            b"\xff\xfe",
            b"end 5e7ce0ad0d81e8ef\n",
        ] {
            assert_eq!(decode::<Sample>(bytes), None, "{bytes:?}");
        }
        for body in [
            "",
            "name\nnote -\ncount 0\non false\napps 0\n",
            "name a b\nnote -\ncount 0\non false\napps 0\n",
            "name a \nnote -\ncount 0\non false\napps 0\n",
            "name -\nnote -\ncount 0\non false\napps 0\n",
            "name a\\q\nnote -\ncount 0\non false\napps 0\n",
            "name a\\\nnote -\ncount 0\non false\napps 0\n",
            "name a\nnote -\ncount +1\non false\napps 0\n",
            "name a\nnote -\ncount 0\non yes\napps 0\n",
            "name a\nnote -\ncount 0\non false\napps 1\n",
            "name a\nnote -\ncount 0\non false\napps 2 0 mail mail.Main\n",
            "name a\nnote -\ncount 0\non false\napps 0\nextra 1\n",
            "name a\nnote -\ncount 0\non false\napps 0\npair x\n",
            "name a\nnote -\ncount 0\non false\napps 0\n\n",
            "note -\nname a\ncount 0\non false\napps 0\n",
        ] {
            assert_eq!(decode::<Sample>(&sealed(body)), None, "{body:?}");
        }
    }

    /// A format at version 3: version 2 renamed `name` to `who`, and version 3 added `times`.
    #[derive(Debug, PartialEq, Default)]
    struct Greeting {
        who: String,
        times: u32,
    }

    const GREETING_MIGRATIONS: [Migration; 2] = [rename_name_to_who, add_times];

    fn rename_name_to_who(body: &str) -> Option<String> {
        let mut input = Reader::new(body);
        let name: String = input.line("name")?;
        let mut out = Writer::default();
        out.line("who", &name);
        input.is_done().then(|| out.into_body())
    }

    fn add_times(body: &str) -> Option<String> {
        Some(format!("{body}times 1\n"))
    }

    impl Format for Greeting {
        const KIND: &'static str = "greeting";
        const VERSION: u32 = 3;
        const MIGRATIONS: &'static [Migration] = &GREETING_MIGRATIONS;

        fn write(&self, out: &mut Writer) {
            out.line("who", &self.who).line("times", self.times);
        }

        fn read(input: &mut Reader<'_>) -> Option<Self> {
            Some(Self {
                who: input.line("who")?,
                times: input.line("times")?,
            })
        }
    }

    #[test]
    fn every_older_version_is_migrated_step_by_step() {
        // MAINT-04
        let ann = |times| Greeting {
            who: "Ann Lee".into(),
            times,
        };

        let v1 = seal("greeting", 1, "name Ann\\sLee\n");
        let v2 = seal("greeting", 2, "who Ann\\sLee\n");
        let v3 = seal("greeting", 3, "who Ann\\sLee\ntimes 5\n");

        assert_eq!(decode::<Greeting>(&v1), Some(ann(1)));
        assert_eq!(decode::<Greeting>(&v2), Some(ann(1)));
        assert_eq!(decode::<Greeting>(&v3), Some(ann(5)));
        assert_eq!(encode(&ann(5)), v3);
        assert_eq!(
            decode::<Greeting>(&seal("greeting", 4, "who Ann\ntimes 5\n")),
            None
        );
    }

    #[test]
    fn a_failed_migration_makes_the_file_unreadable() {
        let v1 = seal("greeting", 1, "name Ann\nextra 1\n");

        assert_eq!(decode::<Greeting>(&v1), None);
    }

    #[test]
    fn a_missing_migration_makes_the_file_unreadable() {
        #[derive(Default)]
        struct Unfinished;

        impl Format for Unfinished {
            const KIND: &'static str = "unfinished";
            const VERSION: u32 = 2;
            const MIGRATIONS: &'static [Migration] = &[];

            fn write(&self, _out: &mut Writer) {}

            fn read(_input: &mut Reader<'_>) -> Option<Self> {
                Some(Self)
            }
        }

        assert!(decode::<Unfinished>(&seal("unfinished", 2, "")).is_some());
        assert!(decode::<Unfinished>(&seal("unfinished", 1, "")).is_none());
    }

    #[test]
    fn saved_values_load_back() {
        let storage = FakeStorage::default();

        save(&storage, StoredFile::Settings, &sample()).unwrap();

        assert_eq!(load::<Sample>(&storage, StoredFile::Settings), sample());
        assert_eq!(*storage.writes.lock().unwrap(), [StoredFile::Settings]);
    }

    #[test]
    fn missing_damaged_or_unreachable_files_load_as_the_defaults() {
        // MAINT-04
        let storage = FakeStorage::default();
        assert_eq!(
            load::<Sample>(&storage, StoredFile::Layout),
            Sample::default()
        );

        storage
            .files
            .lock()
            .unwrap()
            .insert(StoredFile::Layout, b"\x00garbage".to_vec());
        assert_eq!(
            load::<Sample>(&storage, StoredFile::Layout),
            Sample::default()
        );

        save(&storage, StoredFile::Layout, &sample()).unwrap();
        *storage.error.lock().unwrap() = Some(DataError::Failed(Problem::SavedDataUnusable));
        assert_eq!(
            load::<Sample>(&storage, StoredFile::Layout),
            Sample::default()
        );
    }

    #[test]
    fn a_read_tells_missing_read_and_unreadable_files_apart() {
        // APPS-04, REL-02: a first start and a damaged file are handled differently.
        let storage = FakeStorage::default();
        assert_eq!(
            read::<Sample>(&storage, StoredFile::Layout),
            Ok(Stored::Missing)
        );

        save(&storage, StoredFile::Layout, &sample()).unwrap();
        assert_eq!(
            read::<Sample>(&storage, StoredFile::Layout),
            Ok(Stored::Read(sample()))
        );

        let garbage = b"\x00garbage".to_vec();
        storage
            .files
            .lock()
            .unwrap()
            .insert(StoredFile::Layout, garbage.clone());
        assert_eq!(
            read::<Sample>(&storage, StoredFile::Layout),
            Ok(Stored::Unreadable(garbage))
        );

        let error = DataError::Failed(Problem::SavedDataUnusable);
        *storage.error.lock().unwrap() = Some(error.clone());
        assert_eq!(read::<Sample>(&storage, StoredFile::Layout), Err(error));
    }

    /// A format of standalone records, at version 2: version 2 renamed `thing` to `item`.
    #[derive(Debug, PartialEq, Default)]
    struct Shelf {
        items: Vec<String>,
    }

    const SHELF_MIGRATIONS: [Migration; 1] = [|body| Some(body.replace("thing ", "item "))];

    impl Format for Shelf {
        const KIND: &'static str = "shelf";
        const VERSION: u32 = 2;
        const MIGRATIONS: &'static [Migration] = &SHELF_MIGRATIONS;

        fn write(&self, out: &mut Writer) {
            for item in &self.items {
                out.line("item", item);
            }
        }

        fn read(input: &mut Reader<'_>) -> Option<Self> {
            Some(Self {
                items: input.lines("item")?,
            })
        }
    }

    fn items(found: Vec<Shelf>) -> Vec<String> {
        found.into_iter().flat_map(|shelf| shelf.items).collect()
    }

    fn shelf(items: &[&str]) -> Vec<u8> {
        encode(&Shelf {
            items: items.iter().map(|item| (*item).to_owned()).collect(),
        })
    }

    #[test]
    fn a_damaged_file_gives_back_the_records_that_still_read() {
        // REL-02
        let mut damaged = shelf(&["mail", "maps", "music"]);
        let text = String::from_utf8(damaged.clone()).unwrap();
        // "maps" becomes "m\x01ps": the checksum fails, and the record still reads, changed.
        let maps = text.find("maps").unwrap();
        damaged[maps + 1] = 0x01;
        assert_eq!(decode::<Shelf>(&damaged), None);
        // A record whose key was damaged reads as nothing.
        let mut odd = damaged.clone();
        let music = text.find("item music").unwrap();
        odd[music] = b'?';

        assert_eq!(items(salvage(&damaged)), ["mail", "m\u{1}ps", "music"]);
        assert_eq!(items(salvage(&odd)), ["mail", "m\u{1}ps"]);
    }

    #[test]
    fn a_file_cut_short_gives_back_its_whole_records() {
        // REL-02
        let bytes = shelf(&["mail", "maps"]);
        let text = String::from_utf8(bytes.clone()).unwrap();
        let second = text.find("item maps").unwrap();

        assert_eq!(items(salvage(&bytes[..second + 6])), ["mail"]);
        assert_eq!(items(salvage(&bytes[..second + 10])), ["mail", "maps"]);
        assert_eq!(items(salvage(&bytes)), ["mail", "maps"]);
        assert_eq!(items(salvage(&bytes[..second])), ["mail"]);
    }

    #[test]
    fn records_of_an_older_version_are_migrated_one_by_one() {
        // MAINT-04, REL-02
        let mut v1 = seal("shelf", 1, "thing mail\nthing maps\n");
        let last = v1.len() - 2;
        v1[last] ^= 1;

        assert_eq!(items(salvage(&v1)), ["mail", "maps"]);
    }

    #[test]
    fn nothing_is_recovered_from_another_kind_a_newer_version_or_a_damaged_header() {
        // MAINT-04
        let body = "item mail\n";
        for bytes in [
            seal("other", 2, body),
            seal("shelf", 3, body),
            seal("shelf", 0, body),
            seal("shelf", 2, body)[1..].to_vec(),
            format!("meegone-shelf 02\n{body}").into_bytes(),
            b"item mail\n".to_vec(),
            Vec::new(),
            b"\xff\xfe".to_vec(),
        ] {
            assert!(salvage::<Shelf>(&bytes).is_empty(), "{bytes:?}");
        }
    }

    #[test]
    fn every_damaged_or_shortened_file_is_salvaged_without_a_panic() {
        // REL-02: whatever happened to the file, recovering it never crashes.
        let bytes = shelf(&["mail", "two words", "ünïcode ✕", ""]);
        for len in 0..bytes.len() {
            salvage::<Shelf>(&bytes[..len]);
        }
        for i in 0..bytes.len() {
            for bit in 0..8 {
                let mut damaged = bytes.clone();
                damaged[i] ^= 1 << bit;
                salvage::<Shelf>(&damaged);
            }
        }
    }
}
