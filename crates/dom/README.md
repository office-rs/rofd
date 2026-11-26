# rofd-dom

OFD (GB/T 33190) document model — page body + annotation data structures for the rofd editor. A document parsed by rofd-io carries the package it came from (`OfdDocument.package`: `PackageHandle`, plain data), so a save can return the original entries byte for byte and a signature can be appended in place.

**Keywords**: `ofd`, `gbt-33190`, `document-model`

---

Part of [rofd](https://github.com/office-rs/rofd) — OFD (GB/T 33190) view + annotation editor library.

## License

Apache-2.0. See [LICENSE](https://github.com/office-rs/rofd/blob/main/LICENSE).
