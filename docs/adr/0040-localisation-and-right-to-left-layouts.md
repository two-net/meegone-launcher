# ADR 0040: Localisation and right-to-left layouts

**Status:** Accepted · **Date:** 2026-11-23 · **Issue:** #39

## Context

Issue #39 (Must) asks that MeeGone is ready for translation from day one:
- **L10N-01:** all user-visible text is translatable. v1 ships in English.
- **L10N-02:** right-to-left languages mirror the layouts, including the carousel order and swipe directions.
- **L10N-03:** dates, times and numbers follow the device locale and its 12- or 24-hour setting.
- **L10N-04:** sorting and search follow the locale's alphabetical order and ignore accents.
- **Acceptance:** a check in CI fails on hard-coded user-visible strings. Under a right-to-left pseudo-locale, the carousel order and swipe directions are mirrored and no layout breaks. Under an expanded-text pseudo-locale, no text overlaps.

Much was in place:
- **Text:** the Java host takes every string it draws from its resources, and counts from `<plurals>` (ADR 0006, ADR 0038).
- **Dates and times:** ICU writes them from skeletons, in the device's language and 12- or 24-hour setting (`SystemConfigAdapter::format_time`, ADR 0019).
- **Sorting:** each app carries the platform's collation key at primary strength (ADR 0010, ADR 0012).
- **Search:** the platform folds labels and queries alike (ADR 0016).
- **Right to left:** every `Layout` says whether the views are laid out right to left (ADR 0006). The carousel's order, swipes and flings, the grid, Recent's cards, the badges, the menus and the keys all mirror.
- **The audit** fails on text that overlaps, is cut off or cut short, and on stops out of reading order, right to left too (ADR 0038, ADR 0039).

An audit of the code found these gaps:
- **The Rust app wrote words.**
  - About 30 problems were English text: `DataError`'s `Unsupported`, `Invalid` and `Failed`, and each feature's `problem: Option<String>`. Examples are "No connection", "Only HTTPS is allowed" and "The image isn't cached".
  - They reached toasts, the image's status line and the past week's problem.
  - The image cache saved the last problem in English in its check log (ADR 0029).
  - The platform's own failures came back as words in the language of the moment, which the check log kept after the language changed.
- **Nothing checked for hard-coded text.**
- **Numbers and dates:**
  - Events wrote a group's count with `Integer.toString`, in ASCII digits even in Arabic, Persian or Hindi.
  - Recent's time-window choices were the fixed strings "12 hours", "24 hours" and "3 days".
  - A time inside a sentence, as in "Alarm ‹time›" or "Last updated ‹time›", was capitalised as at the start of a line.
- **Regions** in Settings were named with `Locale.getDisplayName`, capitalised as on their own wherever they stood, and listed in the feed's order.
- **Search's folding:**
  - It removed every nonspacing mark. That includes kana's voicing marks and Indic and Thai vowel signs, which make another letter, so バス matched ハス.
  - It didn't fold the Latin letters that have no accent to remove: ø, ł, đ, ħ, æ, œ, ð, þ and ı. So "lodz" didn't find "Łódź".
- **Two paddings were left and right,** not start and end: the home-app banner's, and the past week's problem row.
- **Two strings were reused for other meanings:**
  - a Recent card's "‹app›, ‹age›" for "‹description›, Unavailable";
  - the details screen's "‹date› · ‹region›" for "‹date› · On the lock screen".

  Each would translate wrongly in one of its places.
- **No pseudo-locales:** debug builds had none, and the audit never ran in one.
- **Right to left had one carousel test.**

## Decision

1. **Problems are codes, and the host words them** (L10N-01).
   - **`Problem`,** in `meegone-core-data` (`problem.rs`), lists the 45 things that can go wrong. Each has a stable code, such as `no_connection` or `image_not_cached`.
   - **`DataError`:**
     - `Unsupported`, `Invalid` and `Failed` carry a `Problem`, not text.
     - `DataError::problem()` gives every error's problem; `AccessDenied` is `access_denied`, and `Offline` is `no_connection`.
   - **What carries a problem:** every feature's `problem`, the check status (`CheckStatus::Retrying`, `Unavailable`), the image's `ImageStatus::Unavailable`, the past week's problem, and Settings' written instructions. Text in an error no longer compiles.
   - **The platform's failures are codes too.** Each `Bridge` method that fails returns a constant of `Problems.java` instead of words, and Rust reads it with `Problem::from_host`. A code this build doesn't know, which only a mismatched host could send, is `system_failed`.
   - **The host words a problem when it shows it.** JNI passes `problem.code()`, and `Problems.text` turns it into `problem_<code>` from the resources, in the language of that moment. Every toast goes through `HomeViews.toast` or `AppsGrid`'s. The status line, the past week, the image's unavailable reason and the written instructions word theirs the same way.
   - **The image cache saves the code** (`IMAGE_CACHE_VERSION` 6).
     - The migration reads version 5's English back as its code.
     - Anything else, such as the platform's words in whatever language, becomes `system_failed`, shown until the next check.
   - **A problem is a phrase without a full stop of its own,** unless it is written instructions, so "‹problem›. Will try again later." reads right. Three platform strings lost theirs.
2. **CI fails on hard-coded text** (`tools/arch-check/src/l10n.rs`, `tests/l10n.rs`). The rules read code, not meaning, so they catch accidents rather than determined workarounds.
   - **Java, in the source sets that ship** (`main`, `release`):
     - **Text sinks:** a literal with a letter in a text sink's arguments fails, with no way to allow it. The sinks are `setText`, `setContentDescription`, `makeText`, `new AccessibilityAction`, a dialog's title, message and buttons, and the like (`TEXT_SINKS`).
     - **Other literals:** any other literal that reads as words fails unless `TEXT_ALLOWED` names where it is and why it is never shown. Reading as words means a capitalised word, or two words with a space between.
       - A log's arguments, a thread's name, an exception's message and `TAG` are for developers, and pass.
       - `TEXT_ALLOWED` has five entries: the User-Agent, two HTTP headers, the transliterator's id and the date skeletons.
     - **Numbers:** a number given to a text sink through `Integer.toString`, `String.valueOf` or `String.format` without a `Locale` fails (L10N-03).
   - **XML:** text attributes of the manifest and of any layout must be `@string/…`.
   - **Rust,** in `crates/` and `src/`, outside tests, fakes and `#[cfg(test)]` items:
     - A literal that reads as prose fails, outside panics, assertions and `PROSE_ALLOWED`.
     - `PROSE_ALLOWED` holds the contrast tests' descriptions in `tokens.rs`, the panic messages kept for logs, and the JNI layer's exceptions for logcat.
     - A raw string is read as written, so the migration's escaped English passes.
   - **Problems:** the codes in `problem.rs`, `Problems.java`'s constants and cases, and the `problem_` resources are the same set, and each case words its own code.
   - **String resources:**
     - Every `<string>` and `<plurals>` follows a comment for translators, in its group.
     - A string with several arguments numbers them (`%1$s`), so a translation can reorder them.
     - Only `app_name` is marked untranslatable.
   - **Allow-lists stay true:** an entry of either list that matches nothing fails, as `COLOUR_ALLOWED`'s do (ADR 0038).
3. **Numbers and dates follow the locale** (L10N-03).
   - **Events' count** is written by `NumberFormat`, in the locale's digits.
   - **Recent's time-window choices** are the plurals `recent_window_hours` and `recent_window_days`, with `%d`. `ChoiceSettings` takes labels written out, besides resource ids.
   - **Capitalisation of dates and times** (`Bridge.formatTime`):
     - A weekday or a date, which starts a line, is capitalised for standing alone.
     - A time, alone or with a weekday or a date, sits inside a sentence and is capitalised for the middle of one. This matters in languages that capitalise a month or a weekday only at the start of a line.
4. **Sorting and search follow the locale** (L10N-04).
   - **Regions** (`Regions.java`) are named by ICU's `LocaleDisplayNames` in the device's language:
     - capitalised for a list in the dialog and in Settings' row;
     - capitalised for the middle of a sentence after the details' date and inside "Automatic (…)".
     - In the dialog, Automatic comes first, then the rest in the language's collation order. Each choice keeps its place for `nativeRegion`.
   - **`Bridge.fold`** still folds labels and queries alike:
     1. NFKD.
     2. It removes only accents: the combining diacritical blocks, Hebrew points and Arabic vowel marks. Marks that make another letter, such as kana's voicing marks and Indic and Thai vowel signs, stay.
     3. ICU's `Latin-ASCII` transliterator spells ø, ł, đ, ħ, æ, œ, ð, þ and ı plainly. It is public since API 29, MeeGone's minimum.
     4. Format characters, such as a soft hyphen or a zero-width space, are dropped.
     5. Full case folding, and runs of spaces collapsed to one.

     "lodz" finds "Łódź", and "aero" finds "Ærø".
   - **The fake's fold** learns the same Latin letters and Polish accents, so the acceptance tests can show the rule.
5. **Right to left stays where it was** (L10N-02).
   - **The direction comes from the host's layout,** not the locale: Android's "Force RTL layout direction" can differ from the language, and the views are drawn as laid out (ADR 0006). The frame already carries mirrored positions.
   - **The banner's and the past week's paddings** are start and end.
   - **The two reused strings** get their own: `described_state` for an unavailable card, and `image_date_on_lock_screen`.
   - **Tests:**
     - The carousel's tests are mirrored right to left: following the finger, a fling, a flick back, wrapping around from the left, wrap-around off, catching the moving row, a drag from anywhere, and the indicator.
     - The acceptance scenarios mirror AC-02, with the pages' places, and AC-07's edit-mode drag to the first cell, now the top right one.
6. **The pseudo-locales on a device** (the acceptance criteria).
   - **Debug builds** have Android's pseudo-locales (`isPseudoLocalesEnabled`):
     - en-XA, accented and about a third longer;
     - ar-XB, right to left.
   - **`tools/a11y-audit.sh` adds two passes,** en-XA and ar-XB.
     - Each sets MeeGone's language, as Android 13's per-app languages do, with MeeGone started afresh, and puts it back as it was found.
     - Below Android 13 they are skipped, and the script says so.
   - **Every screen's checks** run in them, in both themes: overlap, text cut off or cut short, and reading order.
   - **New checks, in every pass, with a finger's drags injected into the home views:**
     - `ORDER`: on Applications, Events is drawn on the left and Recent on the right, or the other way round right to left.
     - `SWIPE`: a drag to the right shows the view on the left, and a drag to the left the one on the right.
   - **`L10nCheck`** checks search's folding (`FOLD`) and the collation sorting relies on (`SORT`) on the device's own ICU. Swedish sorts Ö after Z, and accents count for nothing at primary strength.
   - **They run before the keyboard's checks,** so the tour still ends out of touch mode.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0016, decision 1 | The fold keeps marks that make another letter, spells Latin letters with strokes plainly, and drops format characters (decision 4). |
| 0019, decision 1 | A time inside a sentence is capitalised for the middle of one (decision 3). |
| 0026, decision 5 | The choices are plurals in the locale's digits (decision 3). |
| 0029, decisions 1 and 3 | The check log keeps the last problem as its code, not in plain words; the image cache is at version 6 (decision 1). |
| 0031, decisions 4 and 5 | The status line's problem is a code the host words (decision 1). |
| 0035, decisions 3 and 4 | Regions are named by `LocaleDisplayNames` and listed in the language's order. The written instructions are a problem's code (decisions 1 and 4). |
| 0038, decisions 8 and 9 | arch-check also runs the localisation rules, and the audit two pseudo-locale passes (decisions 2 and 6). |

## Consequences

- **What changes:**
  - **No language is fixed in the Rust app or in its saved files.** A second language needs only its `values-‹lang›/strings.xml`.
  - **The type system keeps errors free of words.** CI keeps the rest free of them.
  - **A problem shown after a language change** is in the new language, even the one the check log kept.
- **Checked:**
  - By CI: `cargo test -p arch-check`, every Rust test with the mirrored carousel and acceptance scenarios, and the version 6 migration from every fixture.
  - Each new rule was shown to fail on a hard-coded `setText`, a number through `Integer.toString`, a Rust constant in words, and a problem without its string.
- **Costs:**
  - **A new problem needs three entries,** in `problem.rs`, `Problems.java` and `strings.xml`, which arch-check holds together.
  - **`TEXT_ALLOWED` and `PROSE_ALLOWED`** need an entry, with its reason, for any new developer-facing text that reads as words.
  - **The audit takes about two passes longer.**
- **Not done:**
  - **No `localeConfig` and no in-app language choice** until a second language ships. Android lists only the languages an app declares, and v1 has one.
  - **Hiragana and katakana aren't unified** for search.
  - **Numeric collation is off,** so "App 10" sorts before "App 9".
- **Not checked yet:** the pseudo-locale passes on a device, and Arabic and Hebrew as system languages for the keys and TalkBack's page actions (`RELEASING.md`). The device matrix (#43) covers a tablet.
