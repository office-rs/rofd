# rofd-io

OFD (GB/T 33190) ZIP/XML parser and writer for the rofd editor: a
**preserving save** that keeps the original package and rewrites only the
annotation entries (signed documents keep their signatures), and a
**full write** that generates a package from the model. A document parsed
here carries the package it came from (`OfdDocument.package`), which is
what lets a save return the original entries byte for byte and a signature
be appended in place.

**Keywords**: `ofd`, `gbt-33190`, `zip`, `xml`

---

Part of [rofd](https://github.com/office-rs/rofd) — OFD (GB/T 33190) view + annotation editor library.

## License

Apache-2.0. See [LICENSE](https://github.com/office-rs/rofd/blob/main/LICENSE).
