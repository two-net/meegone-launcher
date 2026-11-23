package io.github.twonet.meegone;

import android.icu.text.Collator;
import android.icu.text.DisplayContext;
import android.icu.text.LocaleDisplayNames;
import android.icu.util.ULocale;
import java.util.Arrays;
import java.util.Comparator;
import java.util.Locale;

/**
 * The image feed's regions, such as {@code en-GB}, named and listed in the device's language
 * (WALL-05, L10N-04, ADR 0040): "English (United Kingdom)", or "anglais (Royaume-Uni)" in French,
 * capitalised as the language does where the name stands, and listed in its alphabetical order.
 */
final class Regions {
    private Regions() {}

    /** {@code tag} named on its own, as in a list or a setting's value. */
    static String name(String tag) {
        return name(tag, DisplayContext.CAPITALIZATION_FOR_UI_LIST_OR_MENU);
    }

    /** {@code tag} named inside a sentence, as in "Automatic (‹name›)" or "‹date› · ‹name›". */
    static String nameInSentence(String tag) {
        return name(tag, DisplayContext.CAPITALIZATION_FOR_MIDDLE_OF_SENTENCE);
    }

    private static String name(String tag, DisplayContext capitals) {
        if (tag == null || tag.isEmpty()) {
            return "";
        }
        String name = LocaleDisplayNames.getInstance(ULocale.getDefault(), capitals)
                .localeDisplayName(Locale.forLanguageTag(tag));
        return name == null || name.isEmpty() ? tag : name;
    }

    /**
     * The order to list {@code names} in: the first where it is, as Automatic leads, then the
     * rest in the language's alphabetical order, ignoring case and accents. Each entry is a
     * place in {@code names}.
     */
    static int[] order(String[] names) {
        Collator collator = Collator.getInstance(ULocale.getDefault());
        collator.setStrength(Collator.PRIMARY);
        Integer[] rest = new Integer[Math.max(0, names.length - 1)];
        for (int i = 0; i < rest.length; i++) {
            rest[i] = i + 1;
        }
        Arrays.sort(rest, Comparator.comparing(place -> names[place], collator));
        int[] order = new int[names.length];
        for (int i = 0; i < rest.length; i++) {
            order[i + 1] = rest[i];
        }
        return order;
    }
}
