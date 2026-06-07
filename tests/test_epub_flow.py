from binbook.epub_flow import flow_items, resolve_image_path


def test_flow_items_preserve_text_image_text_order():
    html = '<html><body><h1>Chapter</h1><p>Before image.</p><img src="../Images/pic.png"/><p>After image.</p></body></html>'

    items = flow_items(html, 2, "OEBPS/Text/chapter.xhtml")

    assert [item.kind for item in items] == ["text", "image", "text"]
    assert items[0].value == "Chapter Before image."
    assert items[1].value == "../Images/pic.png"
    assert items[1].source_spine_index == 2
    assert items[1].source_full_path == "OEBPS/Text/chapter.xhtml"
    assert items[2].value == "After image."


def test_resolve_image_path_handles_relative_paths_and_fragments():
    assert resolve_image_path("OEBPS/Text/chapter.xhtml", "../Images/pic.png#cover") == "OEBPS/Images/pic.png"
