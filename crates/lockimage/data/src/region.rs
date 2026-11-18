//! The image feed's region (WALL-05, ADR 0027): Automatic, or the one the user chose.
//!
//! Regions are BCP 47 `language-REGION` tags, as [`ImageFeed::regions`](crate::ImageFeed)
//! lists them. Automatic is derived from the device's language and region. Of the regions the
//! feed supports, it is:
//! 1. the one with the device's language and region: `de-DE` for `de-DE`;
//! 2. otherwise the first with the device's language: `de-DE` for `de-AT`. Chinese in
//!    Traditional characters (`Hant`, or Taiwan, Hong Kong and Macao) has no such match, as the
//!    feed's Chinese region writes Simplified;
//! 3. otherwise [`FALLBACK`].

/// United States English, the region when nothing else matches (WALL-05).
pub const FALLBACK: &str = "en-US";

/// The Automatic region for a device whose language is `locale`, a BCP 47 tag such as `de-AT`.
pub fn automatic(locale: &str, regions: &[String]) -> String {
    let Some(device) = Tag::parse(locale) else {
        return FALLBACK.to_owned();
    };
    let tagged: Vec<(&String, Tag)> = regions
        .iter()
        .filter_map(|region| Some((region, Tag::parse(region)?)))
        .collect();
    let exact = tagged
        .iter()
        .find(|(_, tag)| tag.same_language(&device) && tag.region == device.region);
    let same_language = || tagged.iter().find(|(_, tag)| tag.same_language(&device));
    exact
        .or_else(same_language)
        .map_or_else(|| FALLBACK.to_owned(), |(region, _)| (*region).clone())
}

/// The region to ask the feed for: the user's choice while the feed still supports it,
/// otherwise Automatic.
pub fn in_use(choice: Option<&str>, locale: &str, regions: &[String]) -> String {
    match choice {
        Some(chosen) if regions.iter().any(|region| region == chosen) => chosen.to_owned(),
        _ => automatic(locale, regions),
    }
}

/// The parts of a language tag that matter here.
#[derive(Debug, PartialEq, Eq)]
struct Tag {
    /// Lowercase, such as `de`.
    language: String,
    /// Lowercase, such as `hant`, when the tag names one.
    script: Option<String>,
    /// Uppercase, such as `AT`, or three digits, such as `419`.
    region: Option<String>,
}

impl Tag {
    /// Reads `language[-script][-region]`, with `-` or `_` between them, in any case. Variants,
    /// extensions and private use that follow are ignored. `None` for an empty or undetermined
    /// (`und`) language.
    fn parse(tag: &str) -> Option<Self> {
        let mut subtags = tag.split(['-', '_']);
        let language = subtags.next()?.to_ascii_lowercase();
        if !(2..=3).contains(&language.len())
            || !language.bytes().all(|byte| byte.is_ascii_alphabetic())
            || language == "und"
        {
            return None;
        }
        let alphabetic = |subtag: &str| subtag.bytes().all(|byte| byte.is_ascii_alphabetic());
        let mut script = None;
        let mut region = None;
        for subtag in subtags {
            match subtag.len() {
                4 if script.is_none() && region.is_none() && alphabetic(subtag) => {
                    script = Some(subtag.to_ascii_lowercase());
                }
                2 if region.is_none() && alphabetic(subtag) => {
                    region = Some(subtag.to_ascii_uppercase());
                }
                3 if region.is_none() && subtag.bytes().all(|byte| byte.is_ascii_digit()) => {
                    region = Some(subtag.to_owned());
                }
                _ => break,
            }
        }
        Some(Self {
            language,
            script,
            region,
        })
    }

    /// The same language, in the same script where that tells regions apart.
    fn same_language(&self, other: &Self) -> bool {
        self.language == other.language && self.chinese_script() == other.chinese_script()
    }

    /// For Chinese, the script it is written in: Simplified unless the tag says Traditional or
    /// names Taiwan, Hong Kong or Macao. `None` for other languages.
    fn chinese_script(&self) -> Option<&str> {
        if self.language != "zh" {
            return None;
        }
        Some(match (self.script.as_deref(), self.region.as_deref()) {
            (Some(script), _) => script,
            (None, Some("TW" | "HK" | "MO")) => "hant",
            (None, _) => "hans",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regions() -> Vec<String> {
        [
            "en-US", "en-GB", "en-CA", "en-IN", "de-DE", "fr-FR", "fr-CA", "it-IT", "es-ES",
            "pt-BR", "ja-JP", "zh-CN",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[test]
    fn automatic_matches_the_language_and_region_first() {
        // WALL-05
        for (locale, region) in [
            ("en-US", "en-US"),
            ("en-GB", "en-GB"),
            ("en-IN", "en-IN"),
            ("fr-CA", "fr-CA"),
            ("ja-JP", "ja-JP"),
            ("en_GB", "en-GB"),
            ("EN-gb", "en-GB"),
            ("en-GB-u-fw-mon", "en-GB"),
            ("zh-CN", "zh-CN"),
            ("zh-Hans-CN", "zh-CN"),
        ] {
            assert_eq!(automatic(locale, &regions()), region, "{locale}");
        }
    }

    #[test]
    fn automatic_then_matches_the_language() {
        // WALL-05
        for (locale, region) in [
            ("de-AT", "de-DE"),
            ("de-CH", "de-DE"),
            ("de", "de-DE"),
            ("fr-BE", "fr-FR"),
            ("es-MX", "es-ES"),
            ("es-419", "es-ES"),
            ("pt-PT", "pt-BR"),
            ("en-AU", "en-US"),
            ("en-IE", "en-US"),
            ("zh-SG", "zh-CN"),
            ("zh", "zh-CN"),
        ] {
            assert_eq!(automatic(locale, &regions()), region, "{locale}");
        }
    }

    #[test]
    fn automatic_falls_back_to_united_states_english() {
        // WALL-05
        for locale in [
            "fi-FI",
            "ar-EG",
            "zh-TW",
            "zh-HK",
            "zh-Hant-HK",
            "zh-Hant-CN",
            "und",
            "",
            "x",
            "-US",
            "e1-US",
        ] {
            assert_eq!(automatic(locale, &regions()), FALLBACK, "{locale}");
        }
        assert_eq!(automatic("de-DE", &[]), FALLBACK);
    }

    #[test]
    fn a_choice_is_kept_while_the_feed_supports_it() {
        // WALL-05
        assert_eq!(in_use(Some("ja-JP"), "de-AT", &regions()), "ja-JP");
        assert_eq!(in_use(None, "de-AT", &regions()), "de-DE");
        // A region the feed no longer lists is Automatic again.
        assert_eq!(in_use(Some("en-NZ"), "de-AT", &regions()), "de-DE");
    }

    #[test]
    fn tags_are_read_subtag_by_subtag() {
        let tag = |language: &str, script: Option<&str>, region: Option<&str>| Tag {
            language: language.into(),
            script: script.map(Into::into),
            region: region.map(Into::into),
        };
        assert_eq!(
            Tag::parse("sr-Latn-RS"),
            Some(tag("sr", Some("latn"), Some("RS")))
        );
        assert_eq!(Tag::parse("es-419"), Some(tag("es", None, Some("419"))));
        assert_eq!(Tag::parse("de-1996"), Some(tag("de", None, None)));
        assert_eq!(
            Tag::parse("en-US-x-private"),
            Some(tag("en", None, Some("US")))
        );
        assert_eq!(Tag::parse("yue-Hant"), Some(tag("yue", Some("hant"), None)));
    }
}
