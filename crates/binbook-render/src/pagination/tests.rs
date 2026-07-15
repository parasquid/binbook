use super::*;

#[test]
fn split_decision_returns_none_when_next_scalar_does_not_fit() {
    // Given: one fewer byte than each UTF-8 scalar requires.
    let cases = [("é", 1), ("—", 2), ("🦀", 3)];

    // When/Then: no complete scalar can be consumed.
    for (value, remaining) in cases {
        assert_eq!(split_decision(value, remaining), None);
    }
}

#[test]
fn push_text_preserves_utf8_when_scalar_crosses_page_boundary() {
    // Given: punctuation whose first scalar cannot fit in the final byte.
    let value = format!("{}—‘tail", "a".repeat(CHARS_PER_PAGE - 1));
    let style = ComputedStyle::default();

    // When: the text is paginated twice from identical state.
    let render = || {
        let mut pages = Pages {
            pages: vec![Page::default()],
            ..Pages::default()
        };
        push_text(&value, &style, &mut pages);
        pages
    };
    let first = render();
    let second = render();

    // Then: pagination progresses without empty spans or byte loss.
    assert_eq!(first.pages.len(), 2);
    assert!(first.pages.iter().all(|page| page.len <= CHARS_PER_PAGE));
    assert!(first
        .pages
        .iter()
        .flat_map(|page| &page.spans)
        .all(|span| !span.text.is_empty()));
    let first_text = first
        .pages
        .iter()
        .flat_map(|page| &page.spans)
        .map(|span| span.text.as_str())
        .collect::<String>();
    let second_text = second
        .pages
        .iter()
        .flat_map(|page| &page.spans)
        .map(|span| span.text.as_str())
        .collect::<String>();
    assert_eq!(first_text, value);
    assert_eq!(second_text, first_text);
}
