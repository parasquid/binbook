from __future__ import annotations

from .constants import SectionId
from .reader import BinBookReader


def inspect_book(reader: BinBookReader, validate: bool = False) -> str:
    compressed = sum(page.compressed_size for page in reader.pages)
    uncompressed = sum(page.uncompressed_size for page in reader.pages)
    ratio = compressed / uncompressed if uncompressed else 0
    lines = [
        "BinBook",
        f"Version: {reader.header.version_major}.{reader.header.version_minor}",
        f"File size: {reader.header.file_size}",
        f"Sections: {len(reader.sections)}",
        f"Pages: {len(reader.pages)}",
        f"Page data offset: {reader.header.page_data_offset}",
        f"Page data length: {reader.header.page_data_length}",
        f"Compression: {compressed}/{uncompressed} bytes ({ratio:.2%})",
        "Section table:",
    ]
    for section_id in sorted(reader.sections):
        section = reader.sections[section_id]
        name = section_id.name if isinstance(section_id, SectionId) else str(section_id)
        lines.append(
            f"  {int(section_id):>2} {name}: offset={section.offset} length={section.length} records={section.record_count}"
        )
    if validate:
        reader.validate()
        lines.append("Validation: OK")
    return "\n".join(lines)
