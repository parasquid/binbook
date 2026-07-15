use binbook_document::{resolve_resource_path, Node, Resource};
use markup5ever_rcdom::{Handle, NodeData};

pub(crate) fn is_figure_container(tag: &str, attrs: &[html5ever::Attribute]) -> bool {
    let classes = attr(attrs, "class").unwrap_or_default();
    tag == "figure"
        || attr(attrs, "role").is_some_and(|role| role.eq_ignore_ascii_case("figure"))
        || classes
            .split_ascii_whitespace()
            .any(|class| matches!(class.to_ascii_lowercase().as_str(), "figcenter" | "cover"))
}

pub(crate) fn figure_image(handle: &Handle, path: &str, resources: &[Resource]) -> Option<Node> {
    let image_handles = descendant_images(handle);
    let [image_handle] = image_handles.as_slice() else {
        return None;
    };
    let NodeData::Element { attrs, .. } = &image_handle.data else {
        return None;
    };
    let attrs = attrs.borrow();
    let reference = image_reference(&attrs)?;
    let resolved = resolve_resource_path(path, &reference).ok()?;
    let resource = resources
        .iter()
        .find(|resource| resource.path == resolved)?;
    let alt = attr(&attrs, "alt").unwrap_or_default();
    match figure_caption(handle) {
        Some(caption) => Some(Node::captioned_image(resource.id, alt, caption)),
        None => Some(Node::image(resource.id, alt)),
    }
}

pub(crate) fn image_reference(attrs: &[html5ever::Attribute]) -> Option<String> {
    attr(attrs, "src").or_else(|| attr(attrs, "href"))
}

fn descendant_images(handle: &Handle) -> Vec<Handle> {
    let mut images = Vec::new();
    for child in handle.children.borrow().iter() {
        if let NodeData::Element { name, .. } = &child.data {
            let tag = name.local.as_ref();
            if tag.eq_ignore_ascii_case("img") || tag.eq_ignore_ascii_case("image") {
                images.push(child.clone());
            }
        }
        images.extend(descendant_images(child));
    }
    images
}

fn figure_caption(handle: &Handle) -> Option<String> {
    for child in handle.children.borrow().iter() {
        if let NodeData::Element { name, attrs, .. } = &child.data {
            let tag = name.local.as_ref();
            let attrs = attrs.borrow();
            let classes = attr(&attrs, "class").unwrap_or_default();
            if tag.eq_ignore_ascii_case("figcaption")
                || classes
                    .split_ascii_whitespace()
                    .any(|class| class.eq_ignore_ascii_case("caption"))
            {
                let caption = normalized_text(child);
                if !caption.is_empty() {
                    return Some(caption);
                }
            }
        }
        if let Some(caption) = figure_caption(child) {
            return Some(caption);
        }
    }
    None
}

fn normalized_text(handle: &Handle) -> String {
    let mut text = String::new();
    if let NodeData::Text { contents } = &handle.data {
        text.push_str(&contents.borrow());
    }
    for child in handle.children.borrow().iter() {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&normalized_text(child));
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn attr(attrs: &[html5ever::Attribute], name: &str) -> Option<String> {
    attrs
        .iter()
        .find(|attr| attr.name.local.as_ref().eq_ignore_ascii_case(name))
        .map(|attr| attr.value.to_string())
}
