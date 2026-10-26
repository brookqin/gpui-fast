// Modified by Longbridge for gpui-fast.
//! Image codec policy; PNG remains available in the compact UI profile.
#[cfg(feature = "image-formats")]
use crate::decode_static_image_from_decoder;
use crate::{ImageCacheError, decode_static_image};
#[cfg(feature = "image-formats")]
use image::{
    AnimationDecoder, Rgba,
    codecs::{gif::GifDecoder, webp::WebPDecoder},
};
use image::{Frame, ImageFormat};
use smallvec::SmallVec;
#[cfg(feature = "image-formats")]
use std::{io::Cursor, sync::Arc};
pub(crate) fn decode(
    bytes: &[u8],
    format: ImageFormat,
    _source: &impl std::fmt::Debug,
) -> Result<SmallVec<[Frame; 1]>, ImageCacheError> {
    #[cfg(feature = "image-formats")]
    let source = _source;
    Ok(match format {
        #[cfg(feature = "image-formats")]
        ImageFormat::Gif => {
            let decoder = GifDecoder::new(Cursor::new(&bytes))?;
            let mut frames = SmallVec::new();

            for frame in decoder.into_frames() {
                match frame {
                    Ok(mut frame) => {
                        // Convert from RGBA to BGRA.
                        for pixel in frame.buffer_mut().chunks_exact_mut(4) {
                            pixel.swap(0, 2);
                        }
                        frames.push(frame);
                    }
                    Err(err) => {
                        log::debug!("Skipping GIF frame in {source:?} due to decode error: {err}");
                    }
                }
            }

            if frames.is_empty() {
                return Err(ImageCacheError::Other(Arc::new(anyhow::anyhow!(
                    "GIF could not be decoded: all frames failed ({source:?})"
                ))));
            }

            frames
        }
        #[cfg(feature = "image-formats")]
        ImageFormat::WebP => {
            let mut decoder = WebPDecoder::new(Cursor::new(&bytes))?;

            if decoder.has_animation() {
                let _ = decoder.set_background_color(Rgba([0, 0, 0, 0]));
                let mut frames = SmallVec::new();

                for frame in decoder.into_frames() {
                    match frame {
                        Ok(mut frame) => {
                            // Convert from RGBA to BGRA.
                            for pixel in frame.buffer_mut().chunks_exact_mut(4) {
                                pixel.swap(0, 2);
                            }
                            frames.push(frame);
                        }
                        Err(err) => {
                            log::debug!(
                                "Skipping WebP frame in {source:?} due to decode error: {err}"
                            );
                        }
                    }
                }

                if frames.is_empty() {
                    return Err(ImageCacheError::Other(Arc::new(anyhow::anyhow!(
                        "WebP could not be decoded: all frames failed ({source:?})"
                    ))));
                }

                frames
            } else {
                decode_static_image_from_decoder(decoder)?
            }
        }
        _ => decode_static_image(&bytes, format)?,
    })
}
