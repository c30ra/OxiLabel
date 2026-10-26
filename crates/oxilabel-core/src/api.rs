//! HTTP API types shared by the server and its clients. Keeping the wire names here means a
//! client and a handler cannot silently disagree about an error, a reply or a query parameter.

use serde::{Deserialize, Serialize};

/// The body of an error response, including the pre-annotation worker's `{"error": "..."}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorReply {
    pub error: String,
}

impl ErrorReply {
    pub fn new(message: impl Into<String>) -> Self {
        Self { error: message.into() }
    }
}

/// Whether a request comes from the server's own machine (ADR 0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostReply {
    pub local: bool,
}

/// The result of the host's folder dialog: the chosen absolute path, or none if cancelled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderDialogReply {
    pub path: Option<String>,
}

/// URL query pairs as produced by the shared query types below.
pub type QueryPairs = Vec<(&'static str, String)>;

/// Appends URL-encoded query pairs to a path. An empty list leaves the path untouched.
pub fn with_query(path: &str, query: &QueryPairs) -> String {
    let mut url = path.to_owned();
    for (index, (key, value)) in query.iter().enumerate() {
        url.push(if index == 0 { '?' } else { '&' });
        encode_component(key, &mut url);
        url.push('=');
        encode_component(value, &mut url);
    }
    url
}

fn encode_component(text: &str, output: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            output.push(byte as char);
        } else {
            output.push('%');
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0xf) as usize] as char);
        }
    }
}

/// A folder of the server's image root to browse, optionally marking a project's known images.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseQuery {
    #[serde(default)]
    pub path: String,
    pub project: Option<i64>,
}

impl BrowseQuery {
    pub fn to_query(&self) -> QueryPairs {
        let mut query = vec![("path", self.path.clone())];
        if let Some(project) = self.project {
            query.push(("project", project.to_string()));
        }
        query
    }
}

/// The page of images to list, in addition to the [`crate::ImageFilter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagePageQuery {
    #[serde(default)]
    pub offset: i64,
    #[serde(default = "default_page_size")]
    pub limit: i64,
}

fn default_page_size() -> i64 {
    100
}

impl Default for ImagePageQuery {
    fn default() -> Self {
        Self { offset: 0, limit: default_page_size() }
    }
}

impl ImagePageQuery {
    pub fn to_query(&self) -> QueryPairs {
        vec![("offset", self.offset.to_string()), ("limit", self.limit.to_string())]
    }
}

/// How many images to give on each side of an image in the filmstrip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighboursQuery {
    #[serde(default = "default_neighbours")]
    pub count: i64,
}

fn default_neighbours() -> i64 {
    6
}

impl Default for NeighboursQuery {
    fn default() -> Self {
        Self { count: default_neighbours() }
    }
}

impl NeighboursQuery {
    pub fn to_query(&self) -> QueryPairs {
        vec![("count", self.count.to_string())]
    }
}

/// Which enhancement filter's pyramid to read a tile from; none means the original.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileQuery {
    pub filter: Option<String>,
}

impl TileQuery {
    pub fn to_query(&self) -> QueryPairs {
        self.filter.as_ref().map(|filter| vec![("filter", filter.clone())]).unwrap_or_default()
    }
}

/// Whether deleting a label may also delete the annotations that use it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteLabelQuery {
    #[serde(default)]
    pub confirm: bool,
}

impl DeleteLabelQuery {
    pub fn to_query(&self) -> QueryPairs {
        self.confirm.then(|| ("confirm", "true".to_owned())).into_iter().collect()
    }
}

/// Whether a project archive contains copies of its image files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveExportQuery {
    #[serde(default)]
    pub images: bool,
}

impl ArchiveExportQuery {
    pub fn to_query(&self) -> QueryPairs {
        vec![("images", self.images.to_string())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_reply_keeps_the_worker_wire_format() {
        let reply = ErrorReply::new("RuntimeError: cannot read broken.pt");
        assert_eq!(serde_json::to_value(&reply).unwrap(), serde_json::json!({"error": "RuntimeError: cannot read broken.pt"}));
        assert_eq!(serde_json::from_value::<ErrorReply>(serde_json::json!({"error": "no GPU"})).unwrap(), ErrorReply::new("no GPU"));
    }

    #[test]
    fn query_components_are_percent_encoded() {
        let query = BrowseQuery { path: "a folder/one+two?".into(), project: None };
        assert_eq!(with_query("/api/browse", &query.to_query()), "/api/browse?path=a%20folder%2Fone%2Btwo%3F");
    }
}
