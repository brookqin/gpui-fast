// Modified by Longbridge for gpui-fast.
//! Explicit SVG capability validation for compact icon-only builds.
use crate::AssetSource;
use std::sync::Arc;

pub type SvgResult<T> = anyhow::Result<T>;

pub(crate) fn render_single_frame(
    renderer: &crate::SvgRenderer,
    bytes: &[u8],
    scale: f32,
) -> SvgResult<Arc<crate::RenderImage>> {
    let parsed = renderer.parse_svg(bytes)?;
    Ok(renderer.render_parsed(&parsed, scale)?)
}

pub(crate) fn options(_assets: Arc<dyn AssetSource>) -> usvg::Options<'static> {
    #[cfg(feature = "svg-text")]
    {
        crate::fast::svg_fonts::options(_assets)
    }
    #[cfg(not(feature = "svg-text"))]
    {
        usvg::Options::default()
    }
}

pub(crate) fn parse(bytes: &[u8], options: &usvg::Options<'_>) -> anyhow::Result<usvg::Tree> {
    #[cfg(not(all(feature = "svg-text", feature = "svg-raster")))]
    {
        let expanded;
        let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
            expanded = usvg::decompress_svgz(bytes)?;
            expanded.as_slice()
        } else {
            bytes
        };
        let document = usvg::roxmltree::Document::parse(std::str::from_utf8(bytes)?)?;
        for node in document.descendants().filter(|n| n.is_element()) {
            let tag = node.tag_name().name();
            anyhow::ensure!(
                cfg!(feature = "svg-text") || !matches!(tag, "text" | "tspan" | "textPath"),
                "SVG text requires the svg-text feature"
            );
            // Embedded images may recursively contain SVG text or raster images.
            // The compact icon profile rejects the entire capability explicitly.
            anyhow::ensure!(
                cfg!(feature = "svg-raster") && cfg!(feature = "svg-text")
                    || !matches!(tag, "image" | "feImage"),
                "SVG embedded images require svg-text and svg-raster features"
            );
        }
    }
    Ok(usvg::Tree::from_data(bytes, options)?)
}

#[cfg(test)]
mod tests {
    use super::parse;
    #[test]
    fn path_icons_still_parse() {
        assert!(parse(br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><path d="M0 0h24v24z"/></svg>"#, &Default::default()).is_ok());
    }
    #[test]
    #[cfg(not(feature = "svg-text"))]
    fn text_is_an_explicit_capability_error() {
        let error = parse(br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><text>sample</text></svg>"#, &Default::default()).unwrap_err();
        assert!(error.to_string().contains("svg-text"));
    }
}
