//! Image blocks: shown as a short marker in bodies, located by reference
//! for viewers that load the bytes.

use super::model::ImageRef;
use crate::js::value::{JsObject, JsValue, defined};

/// Leading text of every image marker.
pub const IMAGE_MARKER: &str = "[image: ";

/// Raster types a viewer may show inline. SVG is markup, so it is left out.
const SHOWABLE: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Media type of a Claude (`source.media_type`) or Pi (`mimeType`) block.
fn media_type(block: &JsObject) -> String {
    let source = block.get("source").and_then(JsValue::as_obj);
    defined(source.and_then(|s| s.get("media_type")))
        .or_else(|| defined(block.get("mimeType")))
        .or_else(|| block.get("media_type"))
        .and_then(JsValue::as_str)
        .unwrap_or("image")
        .to_owned()
}

/// The block when it is an image block (`{type: "image"}`).
pub fn as_image(block: &JsValue) -> Option<&JsObject> {
    block
        .as_obj()
        .filter(|b| b.get("type").and_then(JsValue::as_str) == Some("image"))
}

/// Marker text for an image block, e.g. `[image: image/png]`.
pub fn image_marker(block: &JsObject) -> String {
    format!("{IMAGE_MARKER}{}]", media_type(block))
}

/// A tool result's content with image payloads replaced by text markers.
pub fn omit_image_data(content: Option<&JsValue>) -> Option<JsValue> {
    let content = content?;
    let Some(items) = content.as_arr() else {
        return Some(content.clone());
    };
    Some(JsValue::Arr(
        items
            .iter()
            .map(|b| match as_image(b) {
                Some(img) => JsValue::Obj(JsObject::from_entries([
                    ("type".to_owned(), JsValue::Str("text".to_owned())),
                    ("text".to_owned(), JsValue::Str(image_marker(img))),
                ])),
                None => b.clone(),
            })
            .collect(),
    ))
}

pub fn image_ref(block: &JsObject, block_index: usize, item: Option<usize>) -> ImageRef {
    ImageRef {
        block: block_index,
        media_type: media_type(block),
        item,
    }
}

/// Refs to every image inside a tool result's content, in order.
pub fn tool_result_image_refs(block_index: usize, content: Option<&JsValue>) -> Vec<ImageRef> {
    content
        .and_then(JsValue::as_arr)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .filter_map(|(item, b)| as_image(b).map(|img| image_ref(img, block_index, Some(item))))
        .collect()
}

/// `^[A-Za-z0-9+/]+={0,2}$`.
fn is_base64(data: &str) -> bool {
    let body = data.trim_end_matches('=');
    data.len() - body.len() <= 2
        && !body.is_empty()
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// One image's payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageData {
    pub data: String,
    pub media_type: String,
}

/// The base64 payload of the image a ref names in a parsed line: content
/// block `block`, and item `item` inside it for a tool result. `None` when
/// the ref names anything else or a type a viewer will not show.
pub fn image_at(line: &JsValue, block: usize, item: Option<usize>) -> Option<ImageData> {
    let content = line.get("message")?.as_obj()?.get("content")?.as_arr()?;
    let outer = content.get(block)?;
    let inner = match item {
        None => outer,
        Some(i) => outer.as_obj()?.get("content")?.as_arr()?.get(i)?,
    };
    let img = as_image(inner)?;
    let source = img.get("source").and_then(JsValue::as_obj);
    let ty = media_type(img);
    let data = source
        .and_then(|s| s.get("data"))
        .and_then(JsValue::as_str)?;
    let base64 = source.and_then(|s| s.get("type")).and_then(JsValue::as_str) == Some("base64");
    (base64 && SHOWABLE.contains(&ty.as_str()) && is_base64(data)).then(|| ImageData {
        data: data.to_owned(),
        media_type: ty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> JsValue {
        JsValue::parse(s).unwrap_or(JsValue::Null)
    }

    #[test]
    fn markers_and_refs() {
        let block = v(r#"{"type":"image","source":{"media_type":"image/png"}}"#);
        let img = as_image(&block).cloned().unwrap_or_default();
        assert_eq!(image_marker(&img), "[image: image/png]");
        let content = v(r#"[{"type":"text","text":"a"},{"type":"image","mimeType":"image/gif"}]"#);
        assert_eq!(
            tool_result_image_refs(2, Some(&content)),
            [ImageRef {
                block: 2,
                media_type: "image/gif".to_owned(),
                item: Some(1)
            }]
        );
        assert_eq!(
            omit_image_data(Some(&content)).map(|c| c.stringify(false)),
            Some(
                r#"[{"type":"text","text":"a"},{"type":"text","text":"[image: image/gif]"}]"#
                    .to_owned()
            )
        );
    }

    #[test]
    fn image_bytes() {
        let line = v(
            r#"{"message":{"content":[{"type":"tool_result","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw=="}}]},{"type":"image","source":{"type":"base64","media_type":"image/svg+xml","data":"PHN2Zz4="}}]}}"#,
        );
        assert_eq!(
            image_at(&line, 0, Some(0)),
            Some(ImageData {
                data: "iVBORw==".to_owned(),
                media_type: "image/png".to_owned()
            })
        );
        assert_eq!(image_at(&line, 1, None), None);
        assert_eq!(image_at(&line, 0, None), None);
        assert!(!is_base64("ab==="));
        assert!(!is_base64("a b"));
    }
}
