//! Typed view over the parsed KDL config.

use crate::kdl::{Document, PathSeg};

/// High-level accessors over a parsed [`Document`].
///
/// P0 keeps this thin; P1 turns it into the typed model (Output, Input, ...)
/// that the GUI pages bind to.
#[derive(Debug, Clone)]
pub struct NiriConfig {
    doc: Document,
}

impl NiriConfig {
    pub fn parse(source: &str) -> Result<Self, crate::kdl::ParseError> {
        Ok(Self {
            doc: crate::kdl::parse_document(source)?,
        })
    }

    pub const fn document(&self) -> &Document {
        &self.doc
    }

    /// Names of configured outputs, in document order.
    pub fn output_names(&self) -> Vec<String> {
        self.doc
            .find_all("output")
            .iter()
            .filter_map(|n| n.args.first())
            .map(|v| self.doc.value_text(v))
            .collect()
    }

    /// The n-th (0-based) output block, for GUI editing.
    pub fn output(&self, index: usize) -> Option<&crate::kdl::Node> {
        self.doc
            .find(&[PathSeg::Name("output"), PathSeg::Index(index)])
    }

    /// Whether the config contains a `layer-rule` with a backdrop placement
    /// (used by wallpaper tools like awww/swaybg).
    pub fn has_backdrop_layer_rule(&self) -> bool {
        self.doc
            .find_all("layer-rule")
            .iter()
            .any(|n| n.children.iter().any(|c| c.name == "place-within-backdrop"))
    }
}
