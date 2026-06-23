from __future__ import annotations

from dataclasses import dataclass
import json

from .constants import SectionId
from .reader import BinBookReader


@dataclass(frozen=True)
class InspectionResult:
    text: str
    json_text: str
    ok: bool


def inspect_book(reader: BinBookReader, validate: bool = False, *, json_output: bool = False, strict: bool = False) -> InspectionResult:
    validation_errors = collect_validation_errors(reader) if validate else []
    payload = _payload(reader, validation_errors if validate else None)
    text = _text(payload, strict=strict)
    return InspectionResult(
        text=text,
        json_text=json.dumps(payload, indent=2, sort_keys=True),
        ok=not validation_errors,
    )


def collect_validation_errors(reader: BinBookReader) -> list[str]:
    try:
        reader.validate()
    except ValueError as exc:
        errors = [str(exc)]
    else:
        errors = []
    for error in reader.profile_validation_errors():
        if error not in errors:
            errors.append(error)
    return errors


def _payload(reader: BinBookReader, validation_errors: list[str] | None) -> dict[str, object]:
    total_compressed = 0
    for page in reader.pages:
        for slot in range(4):
            if page.plane_dir.bitmap & (1 << slot):
                total_compressed += page.plane_dir.sizes[slot]
    ratio = total_compressed / reader.header.page_data_length if reader.header.page_data_length else 0
    payload: dict[str, object] = {
        "format": "BinBook",
        "file_size": reader.header.file_size,
        "section_count": len(reader.sections),
        "page_count": len(reader.pages),
        "chapter_count": len(reader.chapters),
        "page_data": {"offset": reader.header.page_data_offset, "length": reader.header.page_data_length},
        "compression": {
            "compressed_bytes": total_compressed,
            "ratio": ratio,
        },
        "sections": [
            {
                "id": int(section_id),
                "name": section_id.name if isinstance(section_id, SectionId) else str(section_id),
                "offset": section.offset,
                "length": section.length,
                "entry_size": section.entry_size,
                "record_count": section.record_count,
                "crc32": section.crc32,
            }
            for section_id, section in sorted(reader.sections.items())
        ],
        "pages": [
            {
                "page_number": page.page_number,
                "page_kind": page.page_kind,
                "pixel_format": page.pixel_format,
                "compression_method": page.compression_method,
                "plane_bitmap": page.plane_dir.bitmap,
                "stored_width": page.stored_width,
                "stored_height": page.stored_height,
                "progress_start_ppm": page.progress_start_ppm,
                "progress_end_ppm": page.progress_end_ppm,
            }
            for page in reader.pages
        ],
    }
    if validation_errors is not None:
        payload["validation"] = {"ok": not validation_errors, "errors": validation_errors}
    return payload


def _text(payload: dict[str, object], *, strict: bool) -> str:
    compression = payload["compression"]
    page_data = payload["page_data"]
    lines = [
        "BinBook",
        f"File size: {payload['file_size']}",
        f"Sections: {payload['section_count']}",
        f"Pages: {payload['page_count']}",
        f"Chapters: {payload['chapter_count']}",
        f"Page data offset: {page_data['offset']}",
        f"Page data length: {page_data['length']}",
        f"Compression: {compression['compressed_bytes']} bytes ({compression['ratio']:.2%})",
        "Section table:",
    ]
    for section in payload["sections"]:
        lines.append(
            f"  {section['id']:>2} {section['name']}: offset={section['offset']} length={section['length']} records={section['record_count']}"
        )
    validation = payload.get("validation")
    if validation:
        if validation["ok"]:
            lines.append("Validation: OK")
        else:
            lines.append("Validation: FAILED")
            errors = validation["errors"]
            for error in errors if strict else errors[:1]:
                lines.append(f"  - {error}")
    return "\n".join(lines)
