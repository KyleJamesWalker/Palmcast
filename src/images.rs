//! Pictures a room holds for as long as it holds anything else.
//!
//! Nothing is stored as it arrived. An upload is decoded, shrunk and
//! re-encoded before it reaches memory a viewer can ask for.

use std::io::Cursor;

use image::{ImageEncoder, ImageFormat, ImageReader, Limits};

/// What one upload may weigh before it is shrunk.
pub const MAX_UPLOAD_BYTES: usize = 8 * 1024 * 1024;
/// The longest edge an image is kept at. A stage view is a television across a
/// bar and every other viewer is holding a phone.
pub const MAX_EDGE: u32 = 1600;
const JPEG_QUALITY: u8 = 80;
/// Refuses a picture whose header claims more pixels than any deck needs,
/// before anything allocates room for them.
const MAX_PIXELS_PER_EDGE: u32 = 12_000;

#[derive(Debug, PartialEq, Eq)]
pub enum ImageError {
    /// Not a picture, or not one in a format this build can read.
    Unreadable,
    TooLarge,
}

impl ImageError {
    pub fn message(&self) -> &'static str {
        match self {
            ImageError::Unreadable => "that file is not an image this server can read",
            ImageError::TooLarge => "that image is too large",
        }
    }
}

/// One stored picture: what to serve it as, and the bytes to serve.
pub struct Stored {
    pub id: String,
    pub kind: &'static str,
    pub bytes: Vec<u8>,
}

impl Stored {
    pub fn extension(&self) -> &'static str {
        match self.kind {
            "image/png" => "png",
            _ => "jpg",
        }
    }
}

/// Decodes, shrinks and re-encodes an upload. Returns the bytes to keep and the
/// type to serve them as.
///
/// A picture carrying transparency comes back as png and everything else as
/// jpeg, so a cut-out stays a cut-out.
pub fn shrink(raw: &[u8]) -> Result<(Vec<u8>, &'static str), ImageError> {
    if raw.len() > MAX_UPLOAD_BYTES {
        return Err(ImageError::TooLarge);
    }

    let mut reader = ImageReader::new(Cursor::new(raw))
        .with_guessed_format()
        .map_err(|_| ImageError::Unreadable)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_PIXELS_PER_EDGE);
    limits.max_image_height = Some(MAX_PIXELS_PER_EDGE);
    reader.limits(limits);

    let decoded = reader.decode().map_err(|_| ImageError::Unreadable)?;
    let decoded = if decoded.width().max(decoded.height()) > MAX_EDGE {
        decoded.resize(MAX_EDGE, MAX_EDGE, image::imageops::FilterType::Lanczos3)
    } else {
        decoded
    };

    let mut out = Vec::new();
    if decoded.color().has_alpha() {
        let rgba = decoded.to_rgba8();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|_| ImageError::Unreadable)?;
        return Ok((out, "image/png"));
    }

    let rgb = decoded.to_rgb8();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|_| ImageError::Unreadable)?;
    Ok((out, "image/jpeg"))
}

/// True for a type this server will try to decode. The bytes decide in the end.
pub fn readable_type(content_type: &str) -> bool {
    let kind = content_type.split(';').next().unwrap_or("").trim();
    matches!(
        ImageFormat::from_mime_type(kind),
        Some(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Gif | ImageFormat::WebP)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    fn png(width: u32, height: u32, alpha: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if alpha {
            let mut img = RgbaImage::new(width, height);
            img.put_pixel(0, 0, Rgba([10, 20, 30, 128]));
            img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
                .unwrap();
        } else {
            let mut img = RgbImage::new(width, height);
            img.put_pixel(0, 0, Rgb([10, 20, 30]));
            img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
                .unwrap();
        }
        out
    }

    fn dimensions(bytes: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(bytes).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn a_photo_from_a_phone_is_shrunk_to_something_a_phone_can_draw() {
        let (bytes, kind) = shrink(&png(4032, 3024, false)).unwrap();
        assert_eq!(kind, "image/jpeg");
        let (width, height) = dimensions(&bytes);
        assert_eq!(width, MAX_EDGE, "the long edge was not brought down");
        assert_eq!(height, 1200, "the shape of the picture changed");
    }

    #[test]
    fn a_tall_picture_is_measured_on_its_long_edge_too() {
        let (bytes, _) = shrink(&png(1000, 4000, false)).unwrap();
        assert_eq!(dimensions(&bytes), (400, MAX_EDGE));
    }

    #[test]
    fn a_picture_already_small_enough_is_left_at_its_size() {
        let (bytes, _) = shrink(&png(800, 600, false)).unwrap();
        assert_eq!(dimensions(&bytes), (800, 600));
    }

    #[test]
    fn a_transparent_picture_is_kept_as_one() {
        let (bytes, kind) = shrink(&png(200, 200, true)).unwrap();
        assert_eq!(kind, "image/png");
        let back = image::load_from_memory(&bytes).unwrap();
        assert_eq!(
            back.to_rgba8().get_pixel(0, 0)[3],
            128,
            "the cut-out was filled in"
        );
    }

    /// Shaded rather than flat, and jpeg rather than png, because that is what
    /// a phone hands over.
    fn photograph(width: u32, height: u32) -> Vec<u8> {
        let mut img = RgbImage::new(width, height);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            let across = (x * 255 / width.max(1)) as u8;
            let down = (y * 255 / height.max(1)) as u8;
            *pixel = Rgb([across, down, across / 2 + down / 2]);
        }
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95)
            .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgb8)
            .unwrap();
        out
    }

    #[test]
    fn a_photograph_gets_smaller_rather_than_larger() {
        let big = photograph(4032, 3024);
        let (bytes, _) = shrink(&big).unwrap();
        assert!(
            bytes.len() < big.len(),
            "shrinking grew the file: {} -> {}",
            big.len(),
            bytes.len()
        );
    }

    /// A phone photograph carries the place it was taken. Decoding to pixels
    /// and encoding again leaves every such field behind, and a room is a room
    /// full of strangers.
    #[test]
    fn nothing_but_pixels_survives_the_re_encoding() {
        let plain = photograph(200, 150);
        let secret = b"secret place";
        // An APP1 segment right after the start marker, where a camera writes
        // Exif and, with it, GPS.
        let mut app1 = vec![0xFF, 0xE1];
        let payload = [b"Exif\0\0".as_slice(), secret].concat();
        app1.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        app1.extend_from_slice(&payload);

        let mut tagged = plain[..2].to_vec();
        tagged.extend_from_slice(&app1);
        tagged.extend_from_slice(&plain[2..]);
        assert!(
            tagged.windows(secret.len()).any(|w| w == secret),
            "the test did not manage to plant anything"
        );

        let (bytes, _) = shrink(&tagged).unwrap();
        assert!(
            !bytes.windows(secret.len()).any(|w| w == secret),
            "metadata from the upload reached the room"
        );
    }

    #[test]
    fn something_that_is_not_a_picture_is_refused() {
        assert_eq!(
            shrink(b"this is a deck, not a picture"),
            Err(ImageError::Unreadable)
        );
    }

    #[test]
    fn an_upload_over_the_limit_is_refused_before_it_is_decoded() {
        let raw = vec![0u8; MAX_UPLOAD_BYTES + 1];
        assert_eq!(shrink(&raw), Err(ImageError::TooLarge));
    }

    #[test]
    fn only_picture_types_are_taken_from_the_header() {
        assert!(readable_type("image/png"));
        assert!(readable_type("image/jpeg; charset=binary"));
        assert!(!readable_type("image/svg+xml"), "svg carries markup");
        assert!(!readable_type("video/mp4"));
        assert!(!readable_type("text/markdown"));
    }
}
