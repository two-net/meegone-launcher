//! Bing's image of the day (SRS 8.2): the one implementation of [`ImageFeed`], and the only code
//! that knows the feed's format (ARCH-04, ADR 0027).
//!
//! The feed is public but undocumented, and may change without notice (A2). Its responses are
//! untrusted (SEC-02): bounded in size, parsed strictly and checked field by field. Anything
//! unexpected is [`Problem::FeedFormatChanged`], a failure of its own for the status line, never a crash.
//!
//! A request asks for today's image, or the past week's (WALL-17), for one region, and carries
//! nothing else (PRIV-02). Each call makes exactly one request and never retries, so the
//! updater can keep to the call budget (REL-04).

use std::sync::Arc;

use meegone_core_data::{DataError, NetworkAdapter, Problem};

use crate::feed::{FeedImage, ImageFeed, ImageVersion, PAST_WEEK_IMAGES};
use crate::json::{self, Value};

/// Where the feed and its images are. Every URL MeeGone requests starts with it (PRIV-02).
const ORIGIN: &str = "https://www.bing.com";

/// The regions that have an image of their own; any other gets one shared image. The first in
/// each language is the one Automatic picks for that language's other regions (WALL-05).
const REGIONS: [&str; 12] = [
    "en-US", "en-GB", "en-CA", "en-IN", "de-DE", "fr-FR", "fr-CA", "it-IT", "es-ES", "pt-BR",
    "ja-JP", "zh-CN",
];

/// A details response is about 1 KB, and the past week's about 5 KB; a larger one isn't read
/// (SEC-02).
const MAX_DETAILS_BYTES: usize = 64 * 1024;
/// A title or an attribution any longer isn't one. Text is rejected rather than cut short, so
/// the attribution is always shown whole (LEGAL-03).
const MAX_TEXT_CHARS: usize = 1000;
const MAX_LINK_CHARS: usize = 2048;
const MAX_ID_CHARS: usize = 200;

/// The sizes every image is offered in, as `<image>_<suffix>`, with their width and height. UHD
/// is the full image, 3840×2160 as the feed publishes it today.
const VERSIONS: [(&str, u32, u32); 3] = [
    ("UHD.jpg", 3840, 2160),
    ("1920x1080.jpg", 1920, 1080),
    ("1080x1920.jpg", 1080, 1920),
];

/// The size browsing the past week shows, about 35 KB (WALL-17, PWR-03).
const PREVIEW: (&str, u32, u32) = ("800x480.jpg", 800, 480);

/// Bing's image-of-the-day feed, fetched through `network`.
pub struct BingFeed {
    network: Arc<dyn NetworkAdapter>,
}

impl BingFeed {
    /// Makes no request until asked.
    pub fn new(network: Arc<dyn NetworkAdapter>) -> Self {
        Self { network }
    }
}

impl ImageFeed for BingFeed {
    fn regions(&self) -> Vec<String> {
        REGIONS.map(str::to_owned).to_vec()
    }

    /// A region the feed doesn't list is refused without a request.
    fn today(&self, region: &str) -> Result<FeedImage, DataError> {
        let body = self.details(region, 1)?;
        parse(&body).ok_or(DataError::Invalid(Problem::FeedFormatChanged))
    }

    /// A region the feed doesn't list is refused without a request.
    fn past_week(&self, region: &str) -> Result<Vec<FeedImage>, DataError> {
        let body = self.details(region, PAST_WEEK_IMAGES)?;
        parse_week(&body).ok_or(DataError::Invalid(Problem::FeedFormatChanged))
    }
}

impl BingFeed {
    /// The one request for `count` images, refused without a request for a region the feed
    /// doesn't list.
    fn details(&self, region: &str, count: usize) -> Result<Vec<u8>, DataError> {
        if !REGIONS.contains(&region) {
            return Err(DataError::Invalid(Problem::NoSuchRegion));
        }
        self.network
            .get(&details_url(region, count), MAX_DETAILS_BYTES)
    }
}

/// The details of `count` images (`n`) from today's (`idx=0`) back, for `region` (`mkt`), as
/// JSON (`format=js`): the request inputs of SRS 8.2, and nothing else.
fn details_url(region: &str, count: usize) -> String {
    format!("{ORIGIN}/HPImageArchive.aspx?format=js&idx=0&n={count}&mkt={region}")
}

/// A details response, if it is within bounds and valid JSON.
fn response(body: &[u8]) -> Option<Value> {
    (body.len() <= MAX_DETAILS_BYTES)
        .then(|| json::parse(body))
        .flatten()
}

/// The first image of a details response, if every field it needs is there and as expected.
fn parse(body: &[u8]) -> Option<FeedImage> {
    parse_image(response(body)?.get("images")?.as_array()?.first()?)
}

/// Every image of a past week's response, newest first (WALL-17). One image the feed can't
/// describe means its format has changed, so the whole response is refused, as is one with
/// none, more than [`PAST_WEEK_IMAGES`], the same image twice, or days out of order.
fn parse_week(body: &[u8]) -> Option<Vec<FeedImage>> {
    let response = response(body)?;
    let images = response.get("images")?.as_array()?;
    if images.is_empty() || images.len() > PAST_WEEK_IMAGES {
        return None;
    }
    let week = images.iter().map(parse_image).collect::<Option<Vec<_>>>()?;
    let in_order = week.windows(2).all(|pair| pair[0].date > pair[1].date);
    let distinct = week
        .iter()
        .enumerate()
        .all(|(index, image)| week[..index].iter().all(|other| other.id != image.id));
    (in_order && distinct).then_some(week)
}

/// One image, if every field it needs is there and as expected.
fn parse_image(image: &Value) -> Option<FeedImage> {
    let id = image.get("urlbase")?.as_str().and_then(image_id)?;
    let date = image
        .get("startdate")?
        .as_str()
        .filter(|date| is_date(date))?;
    let version = |&(suffix, width, height): &(&str, u32, u32)| ImageVersion {
        width,
        height,
        url: format!("{ORIGIN}/th?id={id}_{suffix}"),
    };
    Some(FeedImage {
        id: id.to_owned(),
        date: date.to_owned(),
        title: text(image.get("title")?)?,
        // The whole credit line, as the source provides it (LEGAL-03).
        attribution: text(image.get("copyright")?)?,
        // A link that isn't plain HTTPS is left out; the image is still fine (SEC-02).
        info_link: image.get("copyrightlink").and_then(link),
        offered_as_wallpaper: image.get("wp")?.as_bool()?,
        versions: VERSIONS.iter().map(version).collect(),
        preview: version(&PREVIEW),
    })
}

/// The image's name in `/th?id=<name>`, such as `OHR.BarnOwlsUnite_EN-US9661992435`. It tells
/// images apart, and every size of the image is derived from it. Only letters, digits, `.`, `_`
/// and `-` are allowed, so no URL built from it can lead anywhere else.
fn image_id(urlbase: &str) -> Option<&str> {
    let id = urlbase.strip_prefix("/th?id=")?;
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
    (!id.is_empty() && id.len() <= MAX_ID_CHARS && id.chars().all(allowed)).then_some(id)
}

/// `YYYYMMDD`, with a month and a day that can exist.
fn is_date(date: &str) -> bool {
    let number = |range: std::ops::Range<usize>| date.get(range)?.parse::<u8>().ok();
    date.len() == 8
        && date.bytes().all(|byte| byte.is_ascii_digit())
        && number(4..6).is_some_and(|month| (1..=12).contains(&month))
        && number(6..8).is_some_and(|day| (1..=31).contains(&day))
}

/// Plain text, shown as it is and never interpreted (SEC-02): tabs and line breaks become
/// spaces, and other control characters are dropped. Empty or overlong text is rejected.
fn text(value: &Value) -> Option<String> {
    let cleaned: String = value
        .as_str()?
        .chars()
        .filter_map(|c| match c {
            '\t' | '\n' | '\r' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect();
    let trimmed = cleaned.trim();
    (!trimmed.is_empty() && trimmed.chars().count() <= MAX_TEXT_CHARS).then(|| trimmed.to_owned())
}

/// An `https://` link with nothing in it that could change what it means (SEC-02, WALL-15).
fn link(value: &Value) -> Option<String> {
    let link = value.as_str()?;
    (link.len() > "https://".len()
        && link.len() <= MAX_LINK_CHARS
        && link.starts_with("https://")
        && !link.chars().any(|c| c.is_whitespace() || c.is_control()))
    .then(|| link.to_owned())
}

#[cfg(test)]
mod tests {
    use meegone_core_data::NetworkState;
    use meegone_core_data::fakes::FakeNetwork;

    use super::*;
    use crate::region;

    const EN_US: &str = include_str!("../fixtures/bing-en-US.json");
    const DE_DE: &str = include_str!("../fixtures/bing-de-DE.json");
    const JA_JP: &str = include_str!("../fixtures/bing-ja-JP.json");
    /// The past week in en-US, recorded on 19 November 2026 (WALL-17).
    const EN_US_WEEK: &str = include_str!("../fixtures/bing-en-US-week.json");

    /// An online feed that answers `region` with `body`.
    fn online(region: &str, body: &[u8]) -> (Arc<FakeNetwork>, BingFeed) {
        online_for(1, region, body)
    }

    /// An online feed that answers the past week's request for `region` with `body`.
    fn online_week(region: &str, body: &[u8]) -> (Arc<FakeNetwork>, BingFeed) {
        online_for(PAST_WEEK_IMAGES, region, body)
    }

    fn online_for(count: usize, region: &str, body: &[u8]) -> (Arc<FakeNetwork>, BingFeed) {
        let network = Arc::new(FakeNetwork::default());
        *network.state.lock().unwrap() = NetworkState::Unmetered;
        network
            .responses
            .lock()
            .unwrap()
            .insert(details_url(region, count), body.to_vec());
        (network.clone(), BingFeed::new(network))
    }

    fn preview(id: &str) -> ImageVersion {
        ImageVersion {
            width: 800,
            height: 480,
            url: format!("https://www.bing.com/th?id={id}_800x480.jpg"),
        }
    }

    fn versions(id: &str) -> Vec<ImageVersion> {
        let version = |suffix: &str, width, height| ImageVersion {
            width,
            height,
            url: format!("https://www.bing.com/th?id={id}_{suffix}"),
        };
        vec![
            version("UHD.jpg", 3840, 2160),
            version("1920x1080.jpg", 1920, 1080),
            version("1080x1920.jpg", 1080, 1920),
        ]
    }

    /// The en-US sample with `from` replaced by `to`, which must be in it.
    fn en_us_with(from: &str, to: &str) -> String {
        assert!(EN_US.contains(from), "{from}");
        EN_US.replacen(from, to, 1)
    }

    fn format_changed() -> Result<FeedImage, DataError> {
        Err(DataError::Invalid(Problem::FeedFormatChanged))
    }

    #[test]
    fn a_recorded_response_gives_every_detail() {
        // SRS 8.2, issue #26: parsing is tested against recorded sample responses.
        let (network, feed) = online("en-US", EN_US.as_bytes());

        assert_eq!(
            feed.today("en-US"),
            Ok(FeedImage {
                id: "OHR.BarnOwlsUnite_EN-US9661992435".into(),
                date: "20261118".into(),
                title: "Hunting from dusk till dawn".into(),
                attribution: "Pair of barn owls (© Zahoor Salmi/Getty Images)".into(),
                info_link: Some(
                    "https://www.bing.com/search?q=Barn+owl&form=hpcapt&filters=HpDate%3a%2220261118_0800%22"
                        .into()
                ),
                offered_as_wallpaper: true,
                versions: versions("OHR.BarnOwlsUnite_EN-US9661992435"),
                preview: preview("OHR.BarnOwlsUnite_EN-US9661992435"),
            })
        );
        // One request, carrying only what it asks for (PRIV-02, REL-04).
        assert_eq!(
            *network.requests.lock().unwrap(),
            ["https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1&mkt=en-US"]
        );
    }

    #[test]
    fn text_in_any_language_is_kept_as_written() {
        // LEGAL-03, SEC-02
        let (_, feed) = online("ja-JP", JA_JP.as_bytes());
        let image = feed.today("ja-JP").unwrap();
        assert_eq!(image.id, "OHR.BarnOwlsUnite_JA-JP5990292316");
        assert_eq!(image.date, "20261117");
        assert_eq!(image.title, "夕暮れから始まる狩り");
        assert_eq!(
            image.attribution,
            "メンフクロウのつがい (© Zahoor Salmi/Getty Images)"
        );

        let (_, feed) = online("de-DE", DE_DE.as_bytes());
        let image = feed.today("de-DE").unwrap();
        assert_eq!(image.title, "Jäger der Nacht");
        assert_eq!(
            image.attribution,
            "Ein Schleiereulenpaar (© Zahoor Salmi/Getty Images)"
        );
    }

    #[test]
    fn every_region_is_offered_and_none_needs_a_request() {
        // WALL-05
        let (network, feed) = online("en-US", EN_US.as_bytes());
        let regions = feed.regions();

        assert_eq!(regions.len(), 12);
        assert_eq!(regions[0], region::FALLBACK);
        // Each is a language-REGION tag that Automatic picks for itself.
        for region in &regions {
            assert_eq!(&region::automatic(region, &regions), region);
        }
        assert!(network.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn a_region_the_feed_doesnt_list_is_refused_without_a_request() {
        // PRIV-02: nothing but a listed region goes into a request.
        let (network, feed) = online("en-US", EN_US.as_bytes());

        for region in ["fi-FI", "en-US&n=8", "", "EN-US"] {
            assert!(
                matches!(feed.today(region), Err(DataError::Invalid(_))),
                "{region}"
            );
        }
        assert!(network.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn no_connection_is_a_failure_of_its_own() {
        // WALL-16: told apart from a changed format.
        let (network, feed) = online("en-US", EN_US.as_bytes());
        *network.state.lock().unwrap() = NetworkState::Offline;

        assert_eq!(feed.today("en-US"), Err(DataError::Offline));
        assert_eq!(network.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_truncated_response_is_a_changed_format_never_a_crash() {
        // SRS 8.2, issue #26: truncated responses.
        for sample in [EN_US, DE_DE, JA_JP] {
            let bytes = sample.as_bytes();
            for length in 0..bytes.len() {
                assert_eq!(parse(&bytes[..length]), None, "{length} bytes");
            }
            assert!(parse(bytes).is_some());
        }
        let (_, feed) = online("en-US", &EN_US.as_bytes()[..400]);
        assert_eq!(feed.today("en-US"), format_changed());
    }

    #[test]
    fn an_oversized_response_is_refused() {
        // SEC-02, issue #26: oversized responses.
        let oversized = format!("{EN_US}{}", " ".repeat(MAX_DETAILS_BYTES));
        let (_, feed) = online("en-US", oversized.as_bytes());

        assert!(matches!(feed.today("en-US"), Err(DataError::Invalid(_))));
        // Even if a network adapter let it through.
        assert_eq!(parse(oversized.as_bytes()), None);
        assert!(parse(format!("{EN_US}{}", " ".repeat(1000)).as_bytes()).is_some());
    }

    #[test]
    fn a_malformed_response_is_a_changed_format() {
        // SRS 8.2, SEC-02, issue #26: malformed responses.
        let long = "x".repeat(MAX_TEXT_CHARS + 1);
        let malformed = [
            // Not the feed at all.
            String::new(),
            "<!DOCTYPE html><html><body>Service unavailable</body></html>".into(),
            "{}".into(),
            r#"{"images":[]}"#.into(),
            r#"{"images":{}}"#.into(),
            r#"{"images":[1]}"#.into(),
            format!("\u{feff}{EN_US}"),
            // A field missing or of another type.
            en_us_with(r#""startdate":"20261118","#, ""),
            en_us_with(
                r#""urlbase":"/th?id=OHR.BarnOwlsUnite_EN-US9661992435","#,
                "",
            ),
            en_us_with(r#""title":"Hunting from dusk till dawn","#, ""),
            en_us_with(
                r#""copyright":"Pair of barn owls (© Zahoor Salmi/Getty Images)","#,
                "",
            ),
            en_us_with(r#""wp":true,"#, ""),
            en_us_with(r#""wp":true"#, r#""wp":"true""#),
            en_us_with(r#""wp":true"#, r#""wp":1"#),
            en_us_with(r#""startdate":"20261118""#, r#""startdate":20261118"#),
            en_us_with(
                r#""title":"Hunting from dusk till dawn""#,
                r#""title":null"#,
            ),
            en_us_with(
                r#""title":"Hunting from dusk till dawn""#,
                r#""title":["x"]"#,
            ),
            // A date that can't be.
            en_us_with("\"20261118\"", "\"2026111\""),
            en_us_with("\"20261118\"", "\"202611180\""),
            en_us_with("\"20261118\"", "\"20261318\""),
            en_us_with("\"20261118\"", "\"20261100\""),
            en_us_with("\"20261118\"", "\"2026-1-18\""),
            // An image that would lead anywhere but the feed's own images.
            en_us_with(
                "\"/th?id=OHR.BarnOwlsUnite_EN-US9661992435\"",
                "\"https://evil.example/th?id=OHR.X\"",
            ),
            en_us_with(
                "\"/th?id=OHR.BarnOwlsUnite_EN-US9661992435\"",
                "\"//evil.example/th?id=OHR.X\"",
            ),
            en_us_with(
                "\"/th?id=OHR.BarnOwlsUnite_EN-US9661992435\"",
                "\"/th?id=OHR.X&rf=x\"",
            ),
            en_us_with(
                "\"/th?id=OHR.BarnOwlsUnite_EN-US9661992435\"",
                "\"/th?id=../../x\"",
            ),
            en_us_with(
                "\"/th?id=OHR.BarnOwlsUnite_EN-US9661992435\"",
                "\"/th?id=\"",
            ),
            en_us_with(
                "\"/th?id=OHR.BarnOwlsUnite_EN-US9661992435\"",
                &format!("\"/th?id={}\"", "a".repeat(MAX_ID_CHARS + 1)),
            ),
            // Text that is empty or far too long.
            en_us_with("\"Hunting from dusk till dawn\"", "\" \\n \""),
            en_us_with("\"Hunting from dusk till dawn\"", &format!("\"{long}\"")),
            // Not JSON any more.
            en_us_with("\"wp\":true,", "\"wp\":true,,"),
            en_us_with("\"wp\":true,", "\"wp\":true,\"wp\":false,"),
        ];

        for body in malformed {
            let (_, feed) = online("en-US", body.as_bytes());
            assert_eq!(feed.today("en-US"), format_changed(), "{body}");
        }
        let (_, feed) = online("en-US", &[0xFF, 0xFE, b'{', b'}']);
        assert_eq!(feed.today("en-US"), format_changed());
    }

    #[test]
    fn text_is_kept_as_plain_text() {
        // SEC-02: control characters are no part of a title.
        let body = en_us_with(
            "\"Hunting from dusk till dawn\"",
            r#""\tHunting\nfrom\u0007 dusk<b>&amp;</b> ""#,
        );
        let (_, feed) = online("en-US", body.as_bytes());

        assert_eq!(
            feed.today("en-US").unwrap().title,
            "Hunting from dusk<b>&amp;</b>"
        );
    }

    #[test]
    fn a_link_that_isnt_plain_https_is_left_out() {
        // SEC-02, WALL-15
        let link = "\"https://www.bing.com/search?q=Barn+owl&form=hpcapt&filters=HpDate%3a%2220261118_0800%22\"";
        for other in [
            "\"http://www.bing.com/search?q=Barn+owl\"",
            "\"javascript:alert(1)\"",
            "\"https://\"",
            "\"https://www.bing.com/search?q=Barn owl\"",
            "\"https://www.bing.com/\\u0000\"",
            "\"/search?q=Barn+owl\"",
            "null",
            "1",
        ] {
            let (_, feed) = online("en-US", en_us_with(link, other).as_bytes());
            assert_eq!(feed.today("en-US").unwrap().info_link, None, "{other}");
        }
        let (_, feed) = online(
            "en-US",
            en_us_with(&format!("\"copyrightlink\":{link},"), "").as_bytes(),
        );
        assert_eq!(feed.today("en-US").unwrap().info_link, None);
    }

    #[test]
    fn an_image_not_offered_as_a_wallpaper_says_so() {
        // LEGAL-04: the updater decides what to do with it (ADR 0027).
        let (_, feed) = online(
            "en-US",
            en_us_with("\"wp\":true", "\"wp\":false").as_bytes(),
        );

        assert!(!feed.today("en-US").unwrap().offered_as_wallpaper);
    }
    #[test]
    fn the_past_week_is_one_request_for_seven_images() {
        // WALL-17, issue #31: browsing asks once, for today's image and the six before it.
        let (network, feed) = online_week("en-US", EN_US_WEEK.as_bytes());
        let week = feed.past_week("en-US").unwrap();

        assert_eq!(
            week.iter()
                .map(|image| image.date.as_str())
                .collect::<Vec<_>>(),
            [
                "20261119", "20261118", "20261117", "20261116", "20261115", "20261114", "20261113"
            ]
        );
        assert_eq!(
            *network.requests.lock().unwrap(),
            ["https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=7&mkt=en-US"]
        );
        // Each image is read as today's is, with a preview built from its checked name.
        assert_eq!(week[0].id, "OHR.KnowledgeSculpture_EN-US9746703110");
        assert_eq!(week[0].title, "Where ideas take shape");
        assert!(!week[0].offered_as_wallpaper);
        let (_, today) = online("en-US", EN_US.as_bytes());
        assert_eq!(week[1], today.today("en-US").unwrap());
        for image in &week {
            assert_eq!(image.preview, preview(&image.id));
            assert_eq!(image.versions, versions(&image.id));
            assert!(!image.versions.contains(&image.preview));
        }
    }

    #[test]
    fn the_past_week_is_refused_without_a_request_for_an_unlisted_region() {
        // PRIV-02
        let (network, feed) = online_week("en-US", EN_US_WEEK.as_bytes());

        assert!(matches!(
            feed.past_week("fi-FI"),
            Err(DataError::Invalid(_))
        ));
        assert!(network.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn a_truncated_week_is_a_changed_format_never_a_crash() {
        // SEC-02: every prefix of the recorded response.
        let bytes = EN_US_WEEK.as_bytes();
        for length in 0..bytes.len() {
            assert_eq!(parse_week(&bytes[..length]), None, "{length} bytes");
        }
        assert_eq!(parse_week(bytes).map(|week| week.len()), Some(7));
        // A single day's response is a week of one.
        assert_eq!(parse_week(EN_US.as_bytes()).map(|week| week.len()), Some(1));
    }

    #[test]
    fn a_week_that_isnt_one_is_a_changed_format() {
        // SEC-02, WALL-17: one bad image, too many, repeats or days out of order.
        let barn_owls = r#""startdate":"20261118""#;
        let eight = EN_US_WEEK.replacen(
            r#""images":["#,
            &format!(r#""images":[{},"#, image_json("OHR.Extra", "20261120")),
            1,
        );
        let malformed = [
            r#"{"images":[]}"#.to_owned(),
            eight,
            // One image without a title.
            EN_US_WEEK.replacen(r#""title":"Built to last","#, "", 1),
            // The same image twice.
            EN_US_WEEK.replacen(
                r#""urlbase":"/th?id=OHR.TrailCedars_EN-US2240500417""#,
                r#""urlbase":"/th?id=OHR.BarnOwlsUnite_EN-US9661992435""#,
                1,
            ),
            // Two images on one day, and days out of order.
            EN_US_WEEK.replacen(barn_owls, r#""startdate":"20261119""#, 1),
            EN_US_WEEK.replacen(barn_owls, r#""startdate":"20261120""#, 1),
        ];

        for (case, body) in malformed.iter().enumerate() {
            assert_ne!(body, EN_US_WEEK, "case {case} changes nothing");
            let (_, feed) = online_week("en-US", body.as_bytes());
            assert_eq!(
                feed.past_week("en-US"),
                Err(DataError::Invalid(Problem::FeedFormatChanged)),
                "case {case}"
            );
        }
    }

    /// A whole image as the feed writes one.
    fn image_json(id: &str, date: &str) -> String {
        format!(
            r#"{{"startdate":"{date}","urlbase":"/th?id={id}","copyright":"© Someone","title":"Extra","wp":true}}"#
        )
    }
}
