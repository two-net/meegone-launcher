# Third-party dependencies

Every third-party crate that a workspace crate depends on directly is listed here with its reason (ARCH-08). None may collect or transmit data.

`tools/arch-check` enforces this list in CI. It fails if a crate is used but not listed here, and also if a listed crate is no longer used.

The app's own crates (`crates/` and the root package) use only the Rust standard library.

| Crate | Used by | Reason | Collects or transmits data |
|---|---|---|---|
| `serde_json` | `tools/arch-check` only; a development tool that is never part of the app | Parses the JSON from the local `cargo metadata --no-deps` command, so the architecture rules can be checked in CI. | No. It only parses text it is given and has no network or file access. |

Crates that only a listed crate pulls in, such as `itoa`, `memchr`, `serde_core` and `zmij` under `serde_json`, are covered by that crate's entry. `Cargo.lock` pins every version.
