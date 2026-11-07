# CIF 2.0 Unicode dependencies

The optional `cbf` feature uses the existing Unicode libraries for canonical
caseless name matching. Original input bytes and spellings remain unchanged.
These are Rust library dependencies; no Python or new detector codec is added.

| Package | Version | License | Retained notices |
|---|---|---|---|
| [caseless](https://github.com/unicode-rs/rust-caseless) | 0.2.2 | MIT | `caseless-0.2.2-LICENSE.txt` |
| [unicode-normalization](https://github.com/unicode-rs/unicode-normalization) | 0.1.24 | MIT OR Apache-2.0 | `unicode-normalization-0.1.24-COPYRIGHT.txt`, `-LICENSE-MIT.txt`, `-LICENSE-APACHE.txt` |
| [tinyvec](https://github.com/Lokathor/tinyvec) (transitive) | 1.13.3 | Zlib OR Apache-2.0 OR MIT | `tinyvec-1.13.3-LICENSE-{ZLIB,APACHE,MIT}.md` |

Versions and registry archive SHA256 values are pinned in `Cargo.lock`. The
notices above are copied unchanged from the corresponding downloaded archives.
Default features remain empty; these dependencies are activated only with CBF.
