#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub id: ResourceId,
    pub path: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourcePathError {
    Empty,
    EscapesRoot,
}

pub fn resolve_resource_path(base: &str, reference: &str) -> Result<String, ResourcePathError> {
    let reference = reference.split('#').next().unwrap_or_default();
    if reference.is_empty() {
        return Err(ResourcePathError::Empty);
    }
    let joined = if reference.starts_with('/') {
        reference.trim_start_matches('/').to_owned()
    } else {
        let directory = base.rsplit_once('/').map_or("", |(directory, _)| directory);
        if directory.is_empty() {
            reference.to_owned()
        } else {
            format!("{directory}/{reference}")
        }
    };
    let mut parts = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(ResourcePathError::EscapesRoot);
                }
            }
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        Err(ResourcePathError::Empty)
    } else {
        Ok(parts.join("/"))
    }
}
