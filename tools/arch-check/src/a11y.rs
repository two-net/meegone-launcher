//! The accessibility rules of ADR 0038 that a scan of the Java host can check:
//!
//! - every colour the host draws comes from the theme's tokens, which `meegone-core-ui`'s
//!   `PAIRINGS` hold to their contrast in both themes, so no other colour may be written in its
//!   code, except the few in [`COLOUR_ALLOWED`], each with its reason (A11Y-03, SRS 5.3);
//! - text sizes follow the system font size: no text size is set in pixels, dp or any other
//!   unit that doesn't scale with it (A11Y-04).
//!
//! The rules are pure functions over source text; [`java_files`] lists the files.
//! `tests/a11y.rs` runs them on this repository. They read code, not meaning, so they catch
//! accidents rather than determined workarounds: a colour or a size worked out at run time
//! gets past them, and the audit on a device (ADR 0038) checks what is drawn.

use std::collections::BTreeSet;
use std::path::Path;

use crate::legal::{ANDROID_SOURCES, directories};
use crate::{Violation, files_under, relative};

/// Colour literals the host may write, as `(file, literal, reason)`. The file is relative to
/// the repository root, and the literal is matched as written, anywhere in that file. An entry
/// that matches nothing is reported, so the list stays true.
pub const COLOUR_ALLOWED: [(&str, &str, &str); 14] = [
    (
        "android/app/src/main/java/io/github/twonet/meegone/SettingsRows.java",
        "0x1F000000",
        "the press highlight: the primary text colour at PRESSED_PERCENT (ADR 0038)",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/SettingsRows.java",
        "Color.WHITE",
        "a ripple's mask, which only shapes the ripple and is never drawn",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/RecentList.java",
        "Color.WHITE",
        "a ripple's mask, which only shapes the ripple and is never drawn",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/ImageCard.java",
        "Color.WHITE",
        "a ripple's mask, which only shapes the ripple and is never drawn",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/HomeViews.java",
        "Color.TRANSPARENT",
        "the 1 px status line that only screen readers read (A11Y-08)",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/FolderPanel.java",
        "Color.BLACK",
        "the scrim, black at the theme's opacity from the frame (SRS 5.3, FOLD-04)",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/AppIcons.java",
        "Color.WHITE",
        "the plate behind a legacy icon, part of the icon (APPS-15, ADR 0011)",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/Badge.java",
        "0x66000000",
        "decorative (ADR 0038): the ring around the ✕'s disc; the ✕ itself is held to 3:1",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/AppCell.java",
        "0x40000000",
        "decorative (ADR 0038): the ring around a folder's icon; its name identifies it",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/AppCell.java",
        "0x33000000",
        "decorative (ADR 0038): an empty place in a folder's preview",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/Themed.java",
        "0x66000000",
        "decorative (ADR 0038): the highlight behind selected text, which stays in the text colour",
    ),
    (
        "android/app/src/debug/java/io/github/twonet/meegone/A11yAudit.java",
        "Color.BLACK",
        "the debug build's accessibility audit, which measures colours and draws none (ADR 0038)",
    ),
    (
        "android/app/src/debug/java/io/github/twonet/meegone/A11yAudit.java",
        "0xFF000000",
        "the debug build's accessibility audit, which measures colours and draws none (ADR 0038)",
    ),
    (
        "android/app/src/debug/java/io/github/twonet/meegone/A11yAudit.java",
        "Color.rgb",
        "the debug build's accessibility audit, which measures colours and draws none (ADR 0038)",
    ),
];

/// `android.graphics.Color`'s named colours.
const NAMED_COLOURS: [&str; 12] = [
    "BLACK",
    "WHITE",
    "GRAY",
    "DKGRAY",
    "LTGRAY",
    "RED",
    "GREEN",
    "BLUE",
    "YELLOW",
    "CYAN",
    "MAGENTA",
    "TRANSPARENT",
];
/// `android.graphics.Color`'s methods that make a colour from numbers or a name.
const COLOUR_MAKERS: [&str; 4] = ["rgb", "argb", "parseColor", "valueOf"];
/// The mask that keeps a token's red, green and blue. It draws nothing itself: the alpha put
/// onto it is the colour literal the rule finds.
const RGB_MASK: u64 = 0x00FF_FFFF;

/// The one unit that follows the system font size (A11Y-04).
const SCALED_UNIT: &str = "COMPLEX_UNIT_SP";

/// Every Java file of the Android host, in all its source sets, relative to `root`.
pub fn java_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = Vec::new();
    for source_set in directories(&root.join(ANDROID_SOURCES))? {
        files_under(&source_set.join("java"), &mut files)?;
    }
    Ok(files
        .iter()
        .filter(|file| {
            file.extension()
                .is_some_and(|extension| extension == "java")
        })
        .map(|file| relative(root, file))
        .collect())
}

/// The colour literals in Java `source`, with the line each is on: `Color`'s named colours
/// (`Color.WHITE`), its colour makers (`Color.rgb`, `Color.argb`, `Color.parseColor`,
/// `Color.valueOf`), and every 32-bit hex `int` but the RGB mask, such as the alpha in
/// `| 0x66000000`. Comments and strings are ignored.
pub fn colour_literals(source: &str) -> Vec<(usize, String)> {
    let code = java_code_only(source);
    let mut found = Vec::new();
    for (at, _) in code.match_indices("Color.") {
        if word_before(&code, at) {
            continue;
        }
        let name = word_at(&code[at + "Color.".len()..]);
        let rest = code[at + "Color.".len() + name.len()..].trim_start();
        if NAMED_COLOURS.contains(&name) || (COLOUR_MAKERS.contains(&name) && rest.starts_with('('))
        {
            found.push((line_of(&code, at), format!("Color.{name}")));
        }
    }
    for (at, _) in code
        .match_indices("0x")
        .chain(code.match_indices("0X"))
        .collect::<BTreeSet<_>>()
    {
        if word_before(&code, at) {
            continue;
        }
        let literal = word_at(&code[at..]);
        if literal.ends_with(['L', 'l']) {
            // A long, such as a mask on a revision, is never a colour.
            continue;
        }
        let digits: String = literal[2..].chars().filter(|c| *c != '_').collect();
        if digits.len() == 8
            && digits.chars().all(|c| c.is_ascii_hexdigit())
            && u64::from_str_radix(&digits, 16) != Ok(RGB_MASK)
        {
            found.push((line_of(&code, at), literal.to_owned()));
        }
    }
    found.sort();
    found
}

/// Where the Java `sources`, as `(file, text)`, write a colour that isn't one of the theme's
/// tokens (A11Y-03), and which entries of `allowed` match nothing.
pub fn colour_violations(
    sources: &[(String, String)],
    allowed: &[(&str, &str, &str)],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut used = BTreeSet::new();
    for (file, source) in sources {
        for (line, literal) in colour_literals(source) {
            if allowed
                .iter()
                .any(|(path, allowed, _)| path == file && *allowed == literal)
            {
                used.insert((file.as_str(), literal));
                continue;
            }
            violations.push(Violation(format!(
                "`{file}:{line}` draws `{literal}`, a colour that isn't one of the theme's \
                 tokens; take it from the frame, or add it to COLOUR_ALLOWED with its reason \
                 (A11Y-03, SRS 5.3)"
            )));
        }
    }
    for (file, literal, _) in allowed {
        if !used.contains(&(*file, (*literal).to_owned())) {
            violations.push(Violation(format!(
                "COLOUR_ALLOWED lets `{file}` write `{literal}`, but it doesn't any more; remove \
                 the entry"
            )));
        }
    }
    violations
}

/// Where `file`'s Java `source` sets a text size in a unit that doesn't follow the system font
/// size, such as `COMPLEX_UNIT_PX` or `COMPLEX_UNIT_DIP`, in a `setTextSize` call's arguments,
/// including an `applyDimension` inside them (A11Y-04). A size in a variable worked out
/// elsewhere gets past it.
pub fn text_size_violations(file: &str, source: &str) -> Vec<Violation> {
    let code = java_code_only(source);
    let mut violations = Vec::new();
    for (at, _) in code.match_indices("setTextSize") {
        if word_before(&code, at) || word_at(&code[at..]) != "setTextSize" {
            continue;
        }
        let rest = &code[at + "setTextSize".len()..];
        let Some(arguments) = rest
            .trim_start()
            .strip_prefix('(')
            .map(|inside| &inside[..closing_paren(inside)])
        else {
            continue;
        };
        let units: BTreeSet<&str> = arguments
            .match_indices("COMPLEX_UNIT_")
            .filter(|(start, _)| !word_before(arguments, *start))
            .map(|(start, _)| word_at(&arguments[start..]))
            .filter(|unit| *unit != SCALED_UNIT)
            .collect();
        for unit in units {
            violations.push(Violation(format!(
                "`{file}:{}` sets a text size in `{unit}`, which doesn't follow the system font \
                 size; use `{SCALED_UNIT}` (A11Y-04)",
                line_of(&code, at)
            )));
        }
    }
    violations
}

/// Java `source` with every comment, string, text block and character literal blanked out,
/// keeping each line break, so positions in it are on the same lines as in `source`.
pub fn java_code_only(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let at = |i: usize, text: &str| {
        text.chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    let mut code = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        if at(i, "//") {
            while i < chars.len() && chars[i] != '\n' {
                code.push(' ');
                i += 1;
            }
        } else if at(i, "/*") {
            // Java's block comments don't nest.
            code.push_str("  ");
            i += 2;
            while i < chars.len() && !at(i, "*/") {
                code.push(blank(chars[i]));
                i += 1;
            }
            code.push_str(&"  "[..2.min(chars.len() - i)]);
            i += 2;
        } else if at(i, "\"\"\"") {
            code.push_str("\"\"\"");
            i += 3;
            while i < chars.len() && !at(i, "\"\"\"") {
                let len = if chars[i] == '\\' { 2 } else { 1 };
                for c in chars.iter().skip(i).take(len) {
                    code.push(blank(*c));
                }
                i += len;
            }
            if i < chars.len() {
                code.push_str("\"\"\"");
                i += 3;
            }
        } else if chars[i] == '"' || chars[i] == '\'' {
            let quote = chars[i];
            code.push(quote);
            i += 1;
            while i < chars.len() && chars[i] != quote && chars[i] != '\n' {
                let len = if chars[i] == '\\' { 2 } else { 1 };
                for c in chars.iter().skip(i).take(len) {
                    code.push(blank(*c));
                }
                i += len;
            }
            if chars.get(i) == Some(&quote) {
                code.push(quote);
                i += 1;
            }
        } else {
            code.push(chars[i]);
            i += 1;
        }
    }
    code
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Whether the character before byte `at` of `text` continues a word, so a match there is the
/// end of a longer name, as `MyColor.` or `a0x`.
fn word_before(text: &str, at: usize) -> bool {
    text[..at].chars().next_back().is_some_and(is_word_char)
}

/// The word that starts `text`.
fn word_at(text: &str) -> &str {
    &text[..text.find(|c: char| !is_word_char(c)).unwrap_or(text.len())]
}

/// The 1-based line of byte `at` in `text`.
fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// The byte index of the `)` that closes the call whose arguments start `text`, or its length.
fn closing_paren(text: &str) -> usize {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return i,
            ')' => depth -= 1,
            _ => {}
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn literals(source: &str) -> Vec<String> {
        colour_literals(source)
            .into_iter()
            .map(|(_, literal)| literal)
            .collect()
    }

    #[test]
    fn named_colours_colour_makers_and_32_bit_numbers_are_found() {
        let source = "int a = Color.RED;\n\
                      int b = android.graphics.Color.TRANSPARENT;\n\
                      int c = Color.argb(255, 1, 2, 3) + Color.parseColor (\"#fff\");\n\
                      int d = (dim & 0x00FFFFFF) | 0x66000000;\n\
                      int e = 0xFF_00_00_00;\n";

        assert_eq!(
            colour_literals(source),
            vec![
                (1, "Color.RED".to_owned()),
                (2, "Color.TRANSPARENT".to_owned()),
                (3, "Color.argb".to_owned()),
                (3, "Color.parseColor".to_owned()),
                (4, "0x66000000".to_owned()),
                (5, "0xFF_00_00_00".to_owned()),
            ]
        );
    }

    #[test]
    fn colour_maths_other_classes_shorter_numbers_and_the_rgb_mask_pass() {
        let source = "float l = Color.luminance(background) + Color.alpha(c);\n\
                      ColorStateList p = ColorStateList.valueOf(fg);\n\
                      int m = MyColor.WHITE + SomeColor.rgb(1, 2, 3);\n\
                      int n = (fg & 0x00ffffff) | (alpha << 24) | 0xFF | 0x7FFFFFFFFFFFFFFFL;\n\
                      long r = frame[1] & 0xFFFF_FFFFL;\n\
                      int rgb = Color.rgb;\n";

        assert!(literals(source).is_empty(), "{:?}", literals(source));
    }

    #[test]
    fn comments_strings_text_blocks_and_chars_are_ignored_and_lines_kept() {
        let source = "/* Color.RED\n   0x66000000 */\n\
                      // Color.BLACK\n\
                      String s = \"Color.WHITE \\\" 0x11223344\";\n\
                      String t = \"\"\"\n  Color.GRAY\n  \"\"\";\n\
                      char q = '\"'; int x = Color.CYAN;\n";

        assert_eq!(colour_literals(source), vec![(8, "Color.CYAN".to_owned())]);
        assert_eq!(
            java_code_only(source).lines().count(),
            source.lines().count()
        );
    }

    fn sources(files: &[(&str, &str)]) -> Vec<(String, String)> {
        files
            .iter()
            .map(|(file, text)| ((*file).to_owned(), (*text).to_owned()))
            .collect()
    }

    #[test]
    fn a_colour_is_allowed_only_in_the_file_its_entry_names() {
        let allowed = [("A.java", "Color.WHITE", "a mask")];
        let files = sources(&[
            ("A.java", "mask(Color.WHITE);\nmask(Color.WHITE);"),
            ("B.java", "\ntext(Color.WHITE);"),
        ]);

        let violations = colour_violations(&files, &allowed);

        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].0.contains("`B.java:2` draws `Color.WHITE`"));
        assert!(violations[0].0.contains("A11Y-03"));
    }

    #[test]
    fn an_entry_that_matches_nothing_is_reported() {
        let allowed = [("A.java", "0x1F000000", "pressed")];

        let violations = colour_violations(&sources(&[("A.java", "int a = 0;")]), &allowed);

        assert_eq!(violations.len(), 1);
        assert!(violations[0].0.contains("doesn't any more"));
    }

    #[test]
    fn text_sizes_in_units_that_do_not_scale_are_found() {
        let source = "a.setTextSize(TypedValue.COMPLEX_UNIT_PX, 12);\n\
                      paint.setTextSize(TypedValue.applyDimension(\n\
                      TypedValue.COMPLEX_UNIT_DIP, 12, metrics));\n\
                      b.setTextSize ( COMPLEX_UNIT_PT , f(1, 2) );\n";

        let violations = text_size_violations("F.java", source);

        let messages: Vec<&str> = violations.iter().map(|v| v.0.as_str()).collect();
        assert_eq!(messages.len(), 3, "{messages:?}");
        assert!(messages[0].contains("`F.java:1` sets a text size in `COMPLEX_UNIT_PX`"));
        assert!(messages[1].contains("`F.java:2` sets a text size in `COMPLEX_UNIT_DIP`"));
        assert!(messages[2].contains("`F.java:4`") && messages[2].contains("A11Y-04"));
    }

    #[test]
    fn text_sizes_in_sp_or_without_a_unit_and_other_units_elsewhere_pass() {
        let source = "a.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12);\n\
                      text.setTextSize(TypedValue.applyDimension(\n\
                      TypedValue.COMPLEX_UNIT_SP, LABEL_SP, metrics));\n\
                      b.setTextSize(16);\n\
                      int pad = (int) TypedValue.applyDimension(COMPLEX_UNIT_DIP, 8, m);\n\
                      // c.setTextSize(TypedValue.COMPLEX_UNIT_PX, 1);\n\
                      d.resetTextSize(TypedValue.COMPLEX_UNIT_PX, 1);\n";

        let violations = text_size_violations("F.java", source);

        assert!(violations.is_empty(), "{violations:?}");
    }
}
