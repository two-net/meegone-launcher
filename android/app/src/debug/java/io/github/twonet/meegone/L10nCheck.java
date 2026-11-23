package io.github.twonet.meegone;

import android.icu.text.Collator;
import android.icu.text.Transliterator;
import android.icu.util.ULocale;

/**
 * Debug builds only: checks, on the device's own ICU, what search and sorting rely on (L10N-04,
 * ADR 0040). {@link Bridge#fold} ignores case, accents and invisible characters, spells Latin
 * letters with strokes plainly, and keeps the marks that make another letter, such as kana's
 * voicing marks. And the collation the catalogue sorts by follows the language, ignoring accents.
 * The tour runs it in each pass and logs a failure as {@code FAIL Search … FOLD} or {@code FAIL
 * Sorting … SORT}.
 */
final class L10nCheck {
    /** Each text, and what search makes of it. */
    private static final String[][] FOLDED = {
        {"Café", "cafe"},
        {"  CAFE  crème ", "cafe creme"},
        {"ﬁle", "file"},
        {"Straße", "strasse"},
        {"Łódź", "lodz"},
        {"Ærø", "aero"},
        {"İstanbul", "istanbul"},
        {"ısparta", "isparta"},
        {"soft­hyphen", "softhyphen"},
        {"zero​width", "zerowidth"},
        {"Ελλάδα", "ελλαδα"},
    };
    /** Texts that must not fold alike: the mark makes another letter, not an accent. */
    private static final String[][] APART = {
        {"バス", "ハス"},
        {"हि", "ह"},
    };

    private L10nCheck() {}

    static void check(A11yAudit audit, String theme) {
        Transliterator latin = Transliterator.getInstance(Bridge.LATIN_LETTERS);
        for (String[] each : FOLDED) {
            String folded = Bridge.fold(each[0], latin);
            audit.expect(each[1].equals(folded), "Search", theme, "FOLD",
                    "\"" + each[0] + "\" folds to \"" + folded + "\", not \"" + each[1] + "\"");
        }
        for (String[] pair : APART) {
            boolean apart = !Bridge.fold(pair[0], latin).equals(Bridge.fold(pair[1], latin));
            audit.expect(apart, "Search", theme, "FOLD",
                    "\"" + pair[0] + "\" and \"" + pair[1] + "\" fold alike");
        }
        Collator swedish = Collator.getInstance(ULocale.forLanguageTag("sv"));
        swedish.setStrength(Collator.PRIMARY);
        audit.expect(swedish.compare("Örter", "Zoom") > 0, "Sorting", theme, "SORT",
                "Swedish doesn't sort Ö after Z");
        Collator english = Collator.getInstance(ULocale.ENGLISH);
        english.setStrength(Collator.PRIMARY);
        audit.expect(english.compare("Él", "el") == 0, "Sorting", theme, "SORT",
                "English tells É from e at primary strength");
    }
}
