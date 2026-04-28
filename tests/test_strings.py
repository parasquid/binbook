from binbook.strings import StringTableBuilder, read_string


def test_string_table_deduplicates_and_reads_utf8():
    builder = StringTableBuilder()

    empty = builder.add("")
    first = builder.add("BinBook")
    second = builder.add("BinBook")
    other = builder.add("xteink-x4-portrait")
    table = builder.to_bytes()

    assert empty.offset == 0
    assert empty.length == 0
    assert first == second
    assert read_string(table, first) == "BinBook"
    assert read_string(table, other) == "xteink-x4-portrait"
