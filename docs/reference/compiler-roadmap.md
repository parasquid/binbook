# Compiler Roadmap

The native Rust image and EPUB compiler is the current supported implementation. The items below are aspirational follow-on work and are not completion requirements for BinBook 0.1.

## Browser and WASM

- Add a `binbook-wasm` adapter crate over the existing path-free compiler libraries.
- Accept browser `Blob`, `ArrayBuffer`, and stream-backed source adapters without adding filesystem assumptions to compiler crates.
- Bind typed progress phases and stable warning records for JavaScript consumers.
- Build a browser UI only after deterministic native/WASM parity, cancellation, memory limits, and downloadable output behavior are tested.

The current compiler crates already pass `wasm32-unknown-unknown` compile/no-run gates. They intentionally expose no `wasm-bindgen` API or browser UI yet.

## Additional source backends

- Add PDF as a source backend with an explicit rasterization/runtime policy and deterministic font/image behavior.
- Add CBZ and image-archive ingestion with bounded streaming and lexical page ordering.
- Add other archive, document, and image-sequence backends behind the same source-dispatch boundary.
- Keep every backend isolated from the BinBook binary writer so EPUB, PDF, CBZ, standalone images, and future formats converge on shared rendered-page and metadata models.

Each backend requires path-free library APIs, typed errors, deterministic warnings, native and WASM evidence where applicable, and end-to-end strict validation of produced BinBook bytes.
