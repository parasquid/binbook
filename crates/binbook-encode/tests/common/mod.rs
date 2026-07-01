use binbook_core::CompressionMethod;
use binbook_encode::{
    BookBuilder, BookConfig, BookMetadata, CompiledChunk, CompiledPage, CompiledPlane, FontPolicy,
    NavigationEntry, SourceIdentity, UsedFont,
};

pub fn builder() -> BookBuilder {
    let mut builder = BookBuilder::new(BookConfig::xteink_x4());
    builder.set_metadata(BookMetadata {
        title: "Repeated".into(),
        author: "Repeated".into(),
        language: "en".into(),
        ..BookMetadata::default()
    });
    builder.set_source(SourceIdentity::from_bytes(
        1,
        "fixture.epub",
        "urn:test",
        b"deterministic source bytes",
    ));
    builder.set_font_policy(FontPolicy::preserve());
    builder.add_font(UsedFont::epub_from_bytes(
        "Test Sans",
        "fonts/test.woff2",
        b"decoded font bytes",
        0,
        400,
    ));
    builder.add_page(page(0x11));
    builder.add_page(page(0x22));
    builder.add_navigation(NavigationEntry::chapter("Repeated", 0));
    builder
}

fn page(seed: u8) -> CompiledPage {
    let planes = (0_u8..3)
        .map(|slot| {
            let chunks = (0_u8..30)
                .map(|index| CompiledChunk {
                    compressed: vec![0x80, seed.wrapping_add(slot).wrapping_add(index)],
                    row_start: u16::from(index) * 16,
                    row_count: 16,
                    uncompressed_size: 1_600,
                })
                .collect();
            CompiledPlane {
                slot,
                compression: CompressionMethod::RlePackBits,
                chunks,
            }
        })
        .collect();
    CompiledPage::new_gray2(800, 480, planes)
}
