use crate::{Diagnostic, FontStyle, FontWeight, Node, Resource, ResourceId};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DocumentMetadata {
    pub title: String,
    pub author: String,
    pub language: String,
    pub identifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpineItem {
    pub resource: ResourceId,
    pub linear: bool,
    pub root: Node,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavItem {
    pub title: String,
    pub resource: ResourceId,
    pub fragment: Option<String>,
    pub level: u16,
    pub parent: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFace {
    pub family: String,
    pub resource: ResourceId,
    pub weight: FontWeight,
    pub style: FontStyle,
    pub obfuscated: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Document {
    pub metadata: DocumentMetadata,
    pub resources: Vec<Resource>,
    pub spine: Vec<SpineItem>,
    pub navigation: Vec<NavItem>,
    pub fonts: Vec<FontFace>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Document {
    pub fn sort_diagnostics(&mut self) {
        self.diagnostics.sort();
    }

    #[must_use]
    pub fn resource(&self, id: ResourceId) -> Option<&Resource> {
        self.resources.iter().find(|resource| resource.id == id)
    }
}
