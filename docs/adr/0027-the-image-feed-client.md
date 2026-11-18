# ADR 0027: The image-feed client

**Status:** Accepted · **Date:** 2026-11-18 · **Issue:** #26

## Context

Issue #26 builds a small, defensive client for MeeGone's only network service, Bing's image-of-the-day feed:
- **One place knows the format.** A single adapter knows the feed's format, so a change touches only it (SRS 8.2, A2).
- **Request and response.** A request asks for a region, today's image, and one image. A response gives:
  - the publication date, the title and the attribution;
  - an info link, which is optional;
  - the image's sizes;
  - a way to tell images apart.
- **Region (WALL-05).** It defaults to Automatic, derived from the device's language and region, with United States English as the fallback. The user can choose any region the feed supports.
- **Security and privacy.**
  - HTTPS only, with the platform's certificate validation; unencrypted traffic is disabled (SEC-01).
  - Requests carry only the region and the image size, with no device or user identifiers (PRIV-02).
  - Responses are untrusted: their size is bounded, they are parsed strictly, and their text stays plain text (SEC-02).
- **Call budget.** Normally one details request and one image download a day, and never more than 6 automatic attempts a day (REL-04).
- **Failure.** A changed or unparseable format is a failure of its own for the status line, never a crash.
- **Acceptance.**
  - Parsing is tested against recorded sample responses, including malformed, truncated and oversized ones.
  - A redirect to a plain-HTTP URL fails.

What was there already:
- **The feed's interface.** `ImageFeed`, its format-free `FeedImage` and `FakeFeed`, in `meegone-lockimage-data` (ADR 0003).
- **The network adapter.** `NetworkAdapter`, with `state` and `get(https_url, max_bytes)`, and `FakeNetwork`. On Android it was a stub that answered `Offline`.
- **The region setting.** `ImageOptions.region` is saved, and `None` means Automatic, but nothing wrote it.
- **The device's locale.** `SystemConfigAdapter::locale` answered `en-US` on Android.
- **No caller.** Nothing asks the feed for anything yet. The updater arrives with #27 and #28.

The constraints:
- **Only the standard library** (DEPENDENCIES.md). The app's crates have no JSON or HTTP crate, and Rust's standard library has no TLS.
- **No network on the main thread.** Android refuses network access there, and every call into Rust runs on the main thread today. A thread that Rust starts itself isn't attached to the JVM (ADR 0005).
- **Decisions belong in Rust.** Java can't be tested without a device (ADR 0006).
- **The feed as recorded on 2026-11-18:**
  - **Shape.** The response is `{"images":[{…}],"tooltips":{…}}`. Each image has:
    - `startdate`;
    - `urlbase`, as `/th?id=OHR.<name>_<MARKET><number>`;
    - `title`, a headline;
    - `copyright`, a description with the credit in brackets: "Pair of barn owls (© Zahoor Salmi/Getty Images)";
    - `copyrightlink`, `wp` and `hsh`.
  - **Sizes.** Every image is served at `<urlbase>_UHD.jpg` (3840×2160, about 4 MB), `_1920x1080.jpg` and `_1080x1920.jpg`, without redirects. The feed also answers over plain HTTP.
  - **`wp`.** It is false for an image that, in Bing's own words, "is not available to download as wallpaper".
  - **Markets.** Twelve markets have an image of their own, and every other gets a shared one.

## Decision

1. **`BingFeed` is the feed's one implementation** (`bing.rs` in `meegone-lockimage-data`). It is the only code that knows the format.
   - **`today(region)` makes exactly one request:** `https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1&mkt=<region>`.
     - At most 64 KiB is read.
     - It never retries; the updater counts attempts (REL-04).
     - A region the feed doesn't list is refused without a request, so nothing else ever reaches the URL.
   - **`regions()`** lists the twelve markets with an image of their own, as BCP 47 tags, and makes no request.
   - **Every field below is required, of its type, from `images[0]`.**
     - `date` is `startdate`: `YYYYMMDD`, with a month and a day that can exist.
     - `id` is the name in `urlbase`, which must be `/th?id=` followed only by letters, digits, `.`, `_` and `-`. Each size's URL is built from the name on `https://www.bing.com`. A response therefore can't send a download anywhere else, and a download carries only its size (PRIV-02).
     - `versions` are UHD (3840×2160), 1920×1080 and 1080×1920. #27 picks one (WALL-06).
     - `title` is `title`, the headline.
     - `attribution` is the whole `copyright`, unaltered (LEGAL-03). Splitting it at " (©" would depend on its wording.
     - `info_link` is `copyrightlink` while that is plain `https://`. Otherwise there is none, and the image is still good (WALL-15).
     - `offered_as_wallpaper` is `wp`. The client stays neutral: the updater will leave out an image that isn't offered, keep the current wallpaper, and not count it as a failure (LEGAL-04, #45).
   - **Text stays plain** (SEC-02). Tabs and line breaks become spaces, and other control characters are dropped. Text over 1 000 characters is refused, never cut short.
   - **Failure.** Anything else is `DataError::Invalid(FORMAT_CHANGED)`, "The feed's format has changed". The status line can tell it apart from `Offline` (WALL-16), and parsing never panics.
2. **A strict JSON reader, private to the crate** (`json.rs`). It takes RFC 8259 exactly:
   - valid UTF-8, and no comments, trailing commas, byte order mark, leading zeros or `NaN`;
   - valid escapes and surrogate pairs, and no repeated keys;
   - nothing after the value, and nesting at most 32 deep.

   It is about 250 lines, rather than the app's first third-party crate.
3. **The region.**
   - **`region::automatic(locale, regions)`** picks, in order:
     1. the region with the device's language and country;
     2. otherwise the first with its language: de-AT gets de-DE, es-MX es-ES, en-AU en-US. Traditional Chinese (`Hant`, or Taiwan, Hong Kong and Macao) has no such match, as the feed's Chinese is Simplified;
     3. otherwise en-US.
   - **`region::in_use(choice, locale, regions)`** gives the Automatic region in place of a choice the feed no longer lists. Both are in the data layer, so the updater can use them too.
   - **`SystemConfigAdapter::locale`** is real on Android: `Locale.getDefault().toLanguageTag()`, or en-US if the bridge can't answer.
   - **The lock-screen image** reads, at each refresh:
     - `regions`;
     - `region`, the choice while the feed lists it;
     - `automatic_region`.

     `region_in_use()` is the one to ask for.
   - **`LockImageIntent::SetRegion(Option<String>)`** saves a listed region, or Automatic. The lock-screen image becomes that field's one writer (ARCH-06). An unlisted region is ignored, and a failed save becomes `problem`.
   - **`ImageSettingsSection`** offers `regions()`, `region()`, `automatic_region()` and `region_chosen()`. The Settings screen, #34, draws the picker, and nothing draws it before.
4. **On Android, Java makes one request and Rust follows redirects.**
   - **`Bridge.httpsGet(url, maxBytes, outcome)`** makes one GET with `HttpsURLConnection`. The platform's default certificate and host-name checks are never overridden.
     - It follows no redirect.
     - It returns the body, or null with an outcome kind and detail: `ok`, `redirect` with an absolute URL, `offline`, `too-large`, `not-https`, `insecure` (the secure connection failed) or `failed`.
   - **`https::fetch`** in `meegone-core-data` decides what to follow, and is tested without a device. `AndroidNetwork` only passes each hop to the bridge.
     - A URL that isn't `https://` is never requested, so a redirect to plain HTTP fails as `Invalid` (SEC-01).
     - At most 3 redirects are followed.
   - **What a request carries:** the URL; `User-Agent: MeeGone`, in place of the platform's default, which names the device's model and build; no cookies, as no cookie handler is ever installed; and no cache (PRIV-02).
   - **Bounds.** A declared length over the limit is refused unread, and reading stops past the limit. The timeouts are 15 s to connect and 30 s to read.
   - **Threads.** `get` blocks, so it is for a JVM thread other than the main one, such as #28's job thread. On the main thread, `httpsGet` answers `failed` at once, without a request.
   - **`Bridge.networkState()`** answers offline, metered or unmetered from the active network's capabilities (WALL-04).
   - **The JNI binding** gains `call_int` (`CallIntMethodA`, slot 51), `call_bytes` (`GetByteArrayRegion`, slot 200) and a public `string_array`. The slots were checked against NDK 27.3's `jni.h`.
5. **The manifest.**
   - **Permissions:** `INTERNET` and `ACCESS_NETWORK_STATE`, SRS 8.1's "Network and network state". Both are granted at install, without a prompt.
   - **`network_security_config.xml`** refuses cleartext and trusts only the system's certificates, for every domain and in debug builds too (SEC-01). Android's default since API 28 already refuses cleartext. The file makes it explicit, and also leaves out certificates the user added.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0003, decision 5 | `BingFeed` is built, and `FeedImage` gains `offered_as_wallpaper`. |
| 0005, decision 4 | `NetworkAdapter::get` blocks, so it runs only on a JVM thread other than the main one. |
| 0005, decision 7 | The network adapter is built. Wallpaper and scheduling are still stubs. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| WALL-05: the user can choose any region the feed supports | The choice is saved and used, but nothing draws a picker until #34. |

## Consequences

- **Benefits:**
  - **Tested without a device:**
    - Three recorded responses (en-US, de-DE and ja-JP). Every prefix of each fails as a changed format, and none panics.
    - Over thirty malformed responses, and oversized ones.
    - Plain text, links that aren't plain HTTPS, and `wp`.
    - The exact request URL, one request per call, and none for `regions()` or an unlisted region.
    - The JSON grammar and its depth limit.
    - Every region rule, the choice saved across a restart, and a failed save.
    - Redirects: one to plain HTTP fails without being requested, HTTPS ones are followed up to three, and every outcome kind the bridge reports maps to its error.
  - **Checked on a Galaxy S22 (Android 16)**, with a temporary build that called the feed from a background thread and was not kept:
    - **The feed.** The Automatic region followed the device's en-GB. Today's en-GB, ja-JP and de-DE images parsed whole, and the UHD version downloaded (3.7 MB).
    - **Refusals without a request.** The same image with a 1 000-byte limit was too large, and an unlisted region and a plain-HTTP URL were refused before any request was made.
    - **Redirects.**
      - An HTTPS redirect to plain HTTP was refused (the issue's acceptance criterion).
      - Two relative redirects were followed. A fourth redirect was too many.
    - **What a request carried:** `User-Agent: MeeGone`, `Host` and the platform's `Accept-Encoding: gzip`, and nothing else.
    - **Certificates.** Expired, wrong-host and self-signed certificates all failed as "The connection isn't secure".
    - **Cleartext.** A raw cleartext request was refused by the network security config.
    - **The main thread.** A request there answered `failed` without a request.
    - **Permissions.** Both were granted at install, and MeeGone started and drew as before.
  - A format change touches `bing.rs` and its fixtures only.
  - #27 gets `versions` to choose from, #28 a feed that never retries, and #34 a section to draw.
- **Costs:**
  - **Kept in step by hand:** the outcome kinds and the network states, between `Bridge.java` and `https.rs` / `network.rs`.
  - **Assumed, not documented:** the twelve markets, and the UHD size, as the feed served them on 2026-11-18. A wrong guess costs image quality, never a failure: #27 checks the image it downloads.
  - **A narrow first match.** An English device outside the four English markets gets en-US, the first English market, even in Ireland.
