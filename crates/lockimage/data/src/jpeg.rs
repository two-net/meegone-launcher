//! A small, strict reader of JPEG headers, for downloaded images, which are untrusted (SEC-02,
//! ADR 0028).
//!
//! It checks the file's shape and reads the image's size from its frame header, without
//! decoding a pixel. The platform decodes the image only after these checks pass (WALL-07):
//! - The file starts with SOI and ends with EOI.
//! - Every segment before the frame header has a length that stays inside the file. Fill bytes
//!   may precede a marker. Nothing else may sit between segments, and no scan may come first.
//! - The frame is baseline, extended sequential or progressive, all Huffman-coded, so every
//!   platform's decoder takes it. It has 8-bit samples, a width and a height, and 1 or 3
//!   components (greyscale or YCbCr), each with sampling factors from 1 to 4. Lossless,
//!   arithmetic-coded, hierarchical, 12-bit and CMYK files are refused, as is a height left for
//!   later (DNL).
//!
//! It reads only as far as the frame header and never panics: anything else is `None`.

use meegone_core_data::Size;

/// Start of image.
const SOI: u8 = 0xD8;
/// End of image.
const EOI: u8 = 0xD9;
/// Start of scan: the compressed pixels follow.
const SOS: u8 = 0xDA;
/// The frames read here: baseline, extended sequential and progressive, all Huffman-coded.
const FRAMES: [u8; 3] = [0xC0, 0xC1, 0xC2];

/// The size of the JPEG image in `bytes`, if the file is as the module describes.
pub(crate) fn size(bytes: &[u8]) -> Option<Size> {
    let mut rest = bytes
        .strip_prefix(&[0xFF, SOI])?
        .strip_suffix(&[0xFF, EOI])?;
    loop {
        let (code, after) = marker(rest)?;
        let (segment, after) = segment(after)?;
        match code {
            code if FRAMES.contains(&code) => return frame(segment),
            // Another frame, a scan before the frame, or a marker that stands alone here.
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF | SOS | 0x00 | 0x01 | 0xD0..=EOI => {
                return None;
            }
            // Tables, application data and comments.
            _ => rest = after,
        }
    }
}

/// The code of the marker at the start of `bytes`, after its `0xFF` and any fill bytes, and
/// what follows it.
fn marker(bytes: &[u8]) -> Option<(u8, &[u8])> {
    let after = bytes.strip_prefix(&[0xFF])?;
    let start = after.iter().position(|&byte| byte != 0xFF)?;
    let (&code, after) = after[start..].split_first()?;
    Some((code, after))
}

/// The contents of the segment at the start of `bytes`, whose 2-byte length counts itself, and
/// what follows it.
fn segment(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let length = usize::from(u16::from_be_bytes([*bytes.first()?, *bytes.get(1)?]));
    let contents = bytes.get(2..length)?;
    Some((contents, &bytes[length..]))
}

/// The size a frame header gives: its precision, height, width and components.
fn frame(header: &[u8]) -> Option<Size> {
    let [
        precision,
        height_high,
        height_low,
        width_high,
        width_low,
        count,
        components @ ..,
    ] = header
    else {
        return None;
    };
    let height = u16::from_be_bytes([*height_high, *height_low]);
    let width = u16::from_be_bytes([*width_high, *width_low]);
    let component_ok = |component: &[u8]| {
        let (sampling, table) = (component[1], component[2]);
        (1..=4).contains(&(sampling >> 4)) && (1..=4).contains(&(sampling & 0x0F)) && table <= 3
    };
    let ok = *precision == 8
        && height > 0
        && width > 0
        && matches!(count, 1 | 3)
        && components.len() == 3 * usize::from(*count)
        && components.chunks_exact(3).all(component_ok);
    ok.then(|| Size {
        width: u32::from(width),
        height: u32::from(height),
    })
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes;

    use super::*;

    /// Real files, each with its size: made by libjpeg-turbo's `cjpeg` and macOS's `sips` from
    /// plain colours, so they hold no photograph.
    const FIXTURES: [(&[u8], u32, u32); 4] = [
        (
            include_bytes!("../fixtures/jpeg-baseline-720x1280.jpg"),
            720,
            1280,
        ),
        (
            include_bytes!("../fixtures/jpeg-progressive-1280x720.jpg"),
            1280,
            720,
        ),
        (
            include_bytes!("../fixtures/jpeg-greyscale-720x720.jpg"),
            720,
            720,
        ),
        // With Exif, IPTC, a restart interval, and tables after the frame header.
        (
            include_bytes!("../fixtures/jpeg-exif-800x1000.jpg"),
            800,
            1000,
        ),
    ];

    const fn px(width: u32, height: u32) -> Size {
        Size { width, height }
    }

    /// A frame header segment: `code`, 8-bit, `width` × `height`, with three YCbCr components.
    fn frame_segment(code: u8, width: u16, height: u16) -> Vec<u8> {
        let mut segment = vec![0xFF, code, 0x00, 0x11, 0x08];
        segment.extend(height.to_be_bytes());
        segment.extend(width.to_be_bytes());
        segment.extend([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        segment
    }

    /// SOI, the segments, and EOI.
    fn file(segments: &[&[u8]]) -> Vec<u8> {
        let mut bytes = vec![0xFF, SOI];
        for segment in segments {
            bytes.extend_from_slice(segment);
        }
        bytes.extend([0xFF, EOI]);
        bytes
    }

    #[test]
    fn real_files_give_their_size() {
        for (bytes, width, height) in FIXTURES {
            assert_eq!(size(bytes), Some(px(width, height)), "{width}×{height}");
        }
    }

    #[test]
    fn the_fake_wallpapers_crops_read_as_their_size() {
        let crop = px(1080, 2400);

        assert_eq!(size(&fakes::jpeg(crop)), Some(crop));
    }

    #[test]
    fn a_truncated_file_is_refused_never_a_crash() {
        // SEC-02. Ended early, a file has no EOI. With one added, its frame header reads
        // whole or not at all.
        for (bytes, width, height) in FIXTURES {
            for end in 0..bytes.len() {
                assert_eq!(size(&bytes[..end]), None, "{width}×{height}, {end} bytes");
                let mut ended = bytes[..end].to_vec();
                ended.extend([0xFF, EOI]);
                assert!(
                    size(&ended).is_none_or(|read| read == px(width, height)),
                    "{width}×{height}, {end} bytes and EOI"
                );
            }
        }
    }

    #[test]
    fn a_damaged_file_never_crashes_the_reader() {
        // SEC-02: every byte changed in turn, in the headers and the start of the scan.
        for (bytes, ..) in FIXTURES {
            for at in 0..bytes.len().min(1024) {
                let mut damaged = bytes.to_vec();
                damaged[at] ^= 0xFF;
                let _ = size(&damaged);
            }
        }
    }

    #[test]
    fn content_that_isnt_a_jpeg_is_refused() {
        // WALL-07, SEC-02
        let mut noise = Vec::new();
        let mut seed: u32 = 0x2545_F491;
        for _ in 0..4096 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            noise.push(seed.to_le_bytes()[0]);
        }
        for bytes in [
            &b""[..],
            b"\xFF\xD8",
            b"\xFF\xD8\xFF\xD9",
            b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x05\0\0\0\x02\xd0\x08\x02\0\0\0",
            b"GIF89a\x00\x05\xd0\x02",
            b"<!DOCTYPE html><html><body>Not found</body></html>",
            b"{\"images\":[]}",
            &noise,
        ] {
            assert_eq!(size(bytes), None, "{:?}", &bytes[..bytes.len().min(16)]);
        }
    }

    #[test]
    fn only_huffman_coded_8_bit_greyscale_or_colour_frames_are_read() {
        // SEC-02: what every platform's decoder takes.
        for code in FRAMES {
            assert_eq!(
                size(&file(&[&frame_segment(code, 1920, 1080)])),
                Some(px(1920, 1080)),
                "{code:02X}"
            );
        }
        // Lossless, hierarchical and arithmetic-coded frames.
        for code in [0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF] {
            assert_eq!(
                size(&file(&[&frame_segment(code, 1920, 1080)])),
                None,
                "{code:02X}"
            );
        }

        let refused = |change: &dyn Fn(&mut Vec<u8>)| {
            let mut segment = frame_segment(0xC0, 1920, 1080);
            change(&mut segment);
            size(&file(&[&segment]))
        };
        // 12-bit samples.
        assert_eq!(refused(&|segment| segment[4] = 12), None);
        // No height yet (DNL), or no width.
        assert_eq!(refused(&|segment| segment[5..7].fill(0)), None);
        assert_eq!(refused(&|segment| segment[7..9].fill(0)), None);
        // CMYK: four components.
        assert_eq!(
            refused(&|segment| {
                segment[3] = 0x14;
                segment[9] = 4;
                segment.extend([4, 0x11, 1]);
            }),
            None
        );
        // Two components, a sampling factor of 0 or 5, or a fifth table.
        assert_eq!(
            refused(&|segment| {
                segment[3] = 0x0E;
                segment[9] = 2;
                segment.truncate(16);
            }),
            None
        );
        assert_eq!(refused(&|segment| segment[11] = 0x02), None);
        assert_eq!(refused(&|segment| segment[11] = 0x52), None);
        assert_eq!(refused(&|segment| segment[12] = 4), None);
        // A length that disagrees with its components.
        assert_eq!(refused(&|segment| segment[3] = 0x12), None);
    }

    #[test]
    fn a_greyscale_frame_is_read() {
        let segment = [
            0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x02, 0xD0, 0x05, 0x00, 0x01, 0x01, 0x11, 0x00,
        ];

        assert_eq!(size(&file(&[&segment])), Some(px(1280, 720)));
    }

    #[test]
    fn segments_before_the_frame_are_skipped_by_their_length() {
        let comment = [0xFF, 0xFE, 0x00, 0x06, b'h', b'e', b'l', b'o'];
        let application = [0xFF, 0xE1, 0x00, 0x02];
        // Fill bytes may come before any marker.
        let filled = [0xFF, 0xFF, 0xFF, 0xFE, 0x00, 0x02];

        assert_eq!(
            size(&file(&[
                &comment,
                &application,
                &filled,
                &frame_segment(0xC2, 720, 720)
            ])),
            Some(px(720, 720))
        );
    }

    #[test]
    fn a_file_out_of_shape_is_refused() {
        let frame = frame_segment(0xC0, 1920, 1080);
        let scan = [0xFF, SOS, 0x00, 0x02];
        let refused = [
            // A segment that runs past the end, or whose length doesn't count itself.
            file(&[&[0xFF, 0xFE, 0x10, 0x00, 0x00], &frame]),
            file(&[&[0xFF, 0xFE, 0x00, 0x01], &frame]),
            file(&[&[0xFF, 0xFE, 0x00, 0x00], &frame]),
            // A scan before the frame.
            file(&[&scan, &frame]),
            // Another image's start, an end, a restart or stuffing before the frame.
            file(&[&[0xFF, SOI], &frame]),
            file(&[&[0xFF, EOI], &frame]),
            file(&[&[0xFF, 0xD0], &frame]),
            file(&[&[0xFF, 0x00], &frame]),
            // A byte between segments that isn't a marker.
            file(&[&[0x00], &frame]),
            // No frame at all.
            file(&[&[0xFF, 0xFE, 0x00, 0x02]]),
        ];
        for bytes in refused {
            assert_eq!(size(&bytes), None, "{bytes:02X?}");
        }

        // Missing its end, or with anything after it.
        let mut whole = file(&[&frame]);
        assert_eq!(size(&whole), Some(px(1920, 1080)));
        whole.push(0);
        assert_eq!(size(&whole), None);
        whole.truncate(whole.len() - 3);
        assert_eq!(size(&whole), None);
    }
}
