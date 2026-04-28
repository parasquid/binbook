from binbook.checksums import crc32
from binbook.hashes import sha256_digest


def test_crc32_matches_pkzip_known_vector():
    assert crc32(b"123456789") == 0xCBF43926


def test_sha256_returns_raw_digest():
    digest = sha256_digest(b"abc")
    assert len(digest) == 32
    assert digest.hex() == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
