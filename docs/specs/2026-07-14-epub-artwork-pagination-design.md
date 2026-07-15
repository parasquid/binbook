# EPUB Artwork Pagination

## Scope

The EPUB compiler renders referenced raster artwork as standalone X4 pages. HTML parsing, typed document structure, page composition, and final X4 plane compilation remain separate crate responsibilities.

## Discovery and captions

`binbook-epub` recognizes HTML `img` sources and SVG `image` references using either `href` or namespaced `xlink:href`. Paths resolve relative to the containing XHTML resource.

A `figure`, `role="figure"`, `figcenter`, or cover container is collapsed when it contains exactly one descendant image reference. Text from a descendant `figcaption` or `caption` class is whitespace-normalized and stored as the image caption. The consumed caption subtree does not also enter ordinary text flow. Alt text remains distinct and is used only by accessibility data and deterministic fallback rendering.

Missing resources produce a `MissingResource` diagnostic and visible fallback text. Decoding failures produce an `UnsupportedImage` warning and an alt-text fallback page without aborting the remaining document.

## Page expansion

Decoded width and height determine orientation. Portrait and square images produce one page. Landscape images produce two consecutive pages in left-to-right source order.

The landscape split is an exact vertical partition. For odd widths, the left panel owns the center column. The panel rectangles are contiguous, non-overlapping, and their widths sum to the complete source width.

Each complete portrait image or landscape panel is aspect-contained with Lanczos filtering. No source area is cropped or distorted. White letterboxing is intentional when source and page aspect ratios differ.

Captionless artwork uses the complete logical 480×800 viewport. Captioned artwork uses the 24-pixel logical outer margin and a bottom band with 18-pixel text, 22-pixel line height, and at most four lines. Landscape captions repeat on both panels. Overflow is truncated at a UTF-8 scalar boundary with a visible ellipsis and an `UnsupportedContent` warning.

## Raster and memory behavior

Composition occurs at 2× resolution on one 960×1600 RGBA canvas at a time. The renderer decodes one resource, emits its one or two compiled pages, and releases the decoded image and canvas before processing the next layout page. The renderer passes the selected GRAY1/GRAY2 storage format and dithering option to every text and artwork page.

The page-expansion start index is recorded for every private layout page. Navigation anchors are remapped through this table after rasterization, so anchors after a landscape image advance by two output pages.

## Determinism and warnings

Identical source bytes, fonts, and compile options produce identical page ordering and bytes. Render warnings are sorted by resource, spine index, offset, and code. The broader streaming compiler described in `docs/ROADMAP.md` remains future work; this design bounds image-local temporaries but does not remove whole-book output accumulation.
