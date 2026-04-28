from __future__ import annotations

from .structs import StringRef


class StringTableBuilder:
    def __init__(self) -> None:
        self._data = bytearray()
        self._refs: dict[str, StringRef] = {"": StringRef(0, 0)}

    def add(self, value: str | None) -> StringRef:
        text = value or ""
        if text in self._refs:
            return self._refs[text]
        encoded = text.encode("utf-8")
        ref = StringRef(len(self._data), len(encoded))
        self._data.extend(encoded)
        self._refs[text] = ref
        return ref

    def to_bytes(self) -> bytes:
        return bytes(self._data)


def read_string(table: bytes, ref: StringRef, *, errors: str = "replace") -> str:
    if ref.length == 0:
        return ""
    end = ref.offset + ref.length
    if ref.offset < 0 or end > len(table):
        raise ValueError("StringRef is outside the string table")
    return table[ref.offset:end].decode("utf-8", errors=errors)
