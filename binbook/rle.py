from __future__ import annotations


def encode_packbits(data: bytes) -> bytes:
    out = bytearray()
    i = 0
    n = len(data)
    while i < n:
        run_len = 1
        while i + run_len < n and run_len < 128 and data[i + run_len] == data[i]:
            run_len += 1
        if run_len >= 2:
            out.append(0x80 | (run_len - 1))
            out.append(data[i])
            i += run_len
            continue

        literal_start = i
        i += 1
        while i < n and i - literal_start < 128:
            lookahead = 1
            while i + lookahead < n and lookahead < 128 and data[i + lookahead] == data[i]:
                lookahead += 1
            if lookahead >= 2:
                break
            i += 1
        literal = data[literal_start:i]
        out.append(len(literal) - 1)
        out.extend(literal)
    return bytes(out)


def decode_packbits(data: bytes) -> bytes:
    out = bytearray()
    i = 0
    while i < len(data):
        control = data[i]
        i += 1
        if control <= 127:
            count = control + 1
            if i + count > len(data):
                raise ValueError("truncated literal RLE run")
            out.extend(data[i : i + count])
            i += count
        else:
            count = (control & 0x7F) + 1
            if i >= len(data):
                raise ValueError("truncated repeat RLE run")
            out.extend(bytes([data[i]]) * count)
            i += 1
    return bytes(out)
