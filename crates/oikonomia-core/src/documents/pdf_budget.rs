//! Size budget for a parsed PDF, measured on what its streams decode to.
//!
//! pdf-extract decompresses content streams, form and image `XObject`s, font
//! files and `CMap`s through lopdf, which reads each one to its end with no
//! limit. A few kilobytes of Flate or LZW data can expand to gigabytes, so
//! the size of the file says nothing about the memory a document needs.
//!
//! The budget here decodes every stream the way lopdf would, but stops
//! decoding a stream as soon as the budget is spent. The check holds two
//! stages of one stream at a time (the input of a filter and its output),
//! each cut off within one read of [`DECODE_CHUNK_BYTES`] past the budget,
//! whatever the file holds.
//!
//! One gap remains. lopdf decompresses object streams and cross-reference
//! streams while it loads a file, before this check can run, and that step
//! has no limit either.

use std::borrow::Cow;
use std::io::Read;

// The same lopdf as pdf-extract uses; see `analyze`.
use pdf_extract as lopdf;

/// Most pages a PDF may have. Bills, receipts and statements are far
/// shorter; every page costs a pass of text extraction.
pub(super) const MAX_PDF_PAGES: usize = 50;

/// Most bytes the streams of one PDF may decode to, all streams together.
/// Four times the upload cap: room for the text and fonts of a long
/// statement, and a bound on what decoding any one stream can make
/// pdf-extract allocate. A stream that is used several times is decoded
/// again each time, one at a time.
pub(super) const MAX_PDF_DECODED_BYTES: usize = 32 * 1024 * 1024;

/// How much is read from a decoder at a time while counting.
const DECODE_CHUNK_BYTES: usize = 64 * 1024;

/// Whether `document` has at most [`MAX_PDF_PAGES`] pages and its streams
/// decode to at most [`MAX_PDF_DECODED_BYTES`].
pub(super) fn within_budget(document: &lopdf::Document) -> bool {
    if document.get_pages().len() > MAX_PDF_PAGES {
        return false;
    }

    let mut remaining = MAX_PDF_DECODED_BYTES;
    for object in document.objects.values() {
        let lopdf::Object::Stream(stream) = object else {
            continue;
        };
        let Some(decoded) = decoded_len(stream, remaining) else {
            return false;
        };
        remaining -= decoded;
    }
    true
}

/// The most bytes `stream` occupies at any stage of decoding, or `None`
/// when a stage is larger than `cap`.
///
/// Follows `lopdf::Stream::decompressed_content`: the filters run in order,
/// and the first one lopdf does not implement ends the chain, leaving the
/// caller with the stored bytes.
fn decoded_len(stream: &lopdf::Stream, cap: usize) -> Option<usize> {
    let mut stage = Cow::Borrowed(stream.content.as_slice());
    let mut largest = stage.len();

    for filter in stream.filters().unwrap_or_default() {
        if largest > cap {
            return None;
        }

        let decoded = match filter {
            b"FlateDecode" => inflate(&stage, cap)?,
            b"LZWDecode" => unpack_lzw(&stage, early_change(stream), cap)?,
            b"ASCII85Decode" => decode_ascii85(&stage, cap)?,
            _ => break,
        };
        largest = largest.max(decoded.len());
        stage = Cow::Owned(decoded);
    }

    (largest <= cap).then_some(largest)
}

/// Inflates zlib data, or `None` when the output is longer than `cap`.
///
/// Like lopdf, keeps what was read before an error, and retries as raw
/// deflate without the two-byte zlib header when nothing was read.
fn inflate(input: &[u8], cap: usize) -> Option<Vec<u8>> {
    let (output, failed) = read_capped(flate2::read::ZlibDecoder::new(input), cap)?;
    if !failed || !output.is_empty() {
        return Some(output);
    }

    let raw = input.get(2..).unwrap_or_default();
    read_capped(flate2::read::DeflateDecoder::new(raw), cap).map(|(output, _)| output)
}

/// Reads `decoder` to its end or its first error, whichever comes first.
///
/// Returns the bytes read and whether an error ended the read, or `None`
/// as soon as more than `cap` bytes have been produced.
fn read_capped(mut decoder: impl Read, cap: usize) -> Option<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    let mut chunk = vec![0; DECODE_CHUNK_BYTES];

    loop {
        match decoder.read(&mut chunk) {
            Ok(0) => return Some((output, false)),
            Ok(read) => {
                output.extend_from_slice(chunk.get(..read)?);
                if output.len() > cap {
                    return None;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Some((output, true)),
        }
    }
}

/// The `/EarlyChange` flag of an LZW stream, which defaults to set.
fn early_change(stream: &lopdf::Stream) -> bool {
    stream
        .dict
        .get(b"DecodeParms")
        .and_then(lopdf::Object::as_dict)
        .and_then(|parms| parms.get(b"EarlyChange"))
        .and_then(lopdf::Object::as_i64)
        .map_or(true, |flag| flag != 0)
}

/// Unpacks LZW data the way lopdf configures its decoder, or `None` when
/// the output is longer than `cap`. Keeps what was decoded before an error.
fn unpack_lzw(input: &[u8], early_change: bool, cap: usize) -> Option<Vec<u8>> {
    use weezl::decode::Decoder;
    use weezl::{BitOrder, LzwStatus};

    // lopdf: codes start at nine bits, most significant bit first.
    const MIN_CODE_SIZE: u8 = 8;

    let mut decoder = if early_change {
        Decoder::with_tiff_size_switch(BitOrder::Msb, MIN_CODE_SIZE)
    } else {
        Decoder::new(BitOrder::Msb, MIN_CODE_SIZE)
    };

    let mut output = Vec::new();
    let mut chunk = vec![0; DECODE_CHUNK_BYTES];
    let mut input = input;

    loop {
        let result = decoder.decode_bytes(input, &mut chunk);
        input = input.get(result.consumed_in..)?;
        output.extend_from_slice(chunk.get(..result.consumed_out)?);
        if output.len() > cap {
            return None;
        }

        match result.status {
            Ok(LzwStatus::Ok) if result.consumed_in > 0 || result.consumed_out > 0 => {}
            Ok(LzwStatus::Ok | LzwStatus::Done | LzwStatus::NoProgress) | Err(_) => {
                return Some(output);
            }
        }
    }
}

/// Decodes `ASCII85` data the way lopdf does, or `None` when the output is
/// longer than `cap`. A `z` stands for four zero bytes, so the output can be
/// four times the input.
///
/// Data lopdf rejects (a `z` inside a group, a group too large for 32 bits)
/// decodes to nothing: lopdf returns an error there and its caller keeps the
/// stored bytes.
fn decode_ascii85(input: &[u8], cap: usize) -> Option<Vec<u8>> {
    let input = input.strip_suffix(b"~>").unwrap_or(input);

    let mut output = Vec::new();
    let mut group: u32 = 0;
    let mut digits = 0_usize;

    for &byte in input {
        if byte == b'z' {
            if digits != 0 {
                return Some(Vec::new());
            }
            output.extend_from_slice(&[0; 4]);
        } else if byte.is_ascii_whitespace() {
            continue;
        } else if !(b'!'..=b'u').contains(&byte) {
            break;
        } else {
            let Some(shifted) = group.checked_mul(85) else {
                return Some(Vec::new());
            };
            // lopdf adds the digit unchecked, which wraps in a release build.
            group = shifted.wrapping_add(u32::from(byte - b'!'));
            digits += 1;

            if digits == 5 {
                output.extend_from_slice(&group.to_be_bytes());
                group = 0;
                digits = 0;
            }
        }

        if output.len() > cap {
            return None;
        }
    }

    if digits > 0 {
        // A short final group is padded with the largest digit and yields
        // one byte less than it has digits.
        for _ in digits..5 {
            let Some(shifted) = group.checked_mul(85) else {
                return Some(Vec::new());
            };
            group = shifted.wrapping_add(84);
        }
        output.extend_from_slice(group.to_be_bytes().get(..digits - 1)?);
    }

    (output.len() <= cap).then_some(output)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    /// `len` zero bytes as a zlib stream.
    fn deflated_zeros(len: usize) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        let block = vec![0_u8; 64 * 1024];

        let mut written = 0;
        while written < len {
            let part = block.len().min(len - written);
            encoder.write_all(&block[..part]).expect("compress zeros");
            written += part;
        }
        encoder.finish().expect("finish zlib stream")
    }

    fn stream(filter: lopdf::Object, content: Vec<u8>) -> lopdf::Stream {
        lopdf::Stream::new(lopdf::dictionary! { "Filter" => filter }, content)
    }

    fn name(filter: &str) -> lopdf::Object {
        lopdf::Object::Name(filter.as_bytes().to_vec())
    }

    #[test]
    fn a_plain_stream_counts_its_stored_bytes() {
        let plain = lopdf::Stream::new(lopdf::Dictionary::new(), vec![7; 100]);

        assert_eq!(decoded_len(&plain, 100), Some(100));
        assert_eq!(decoded_len(&plain, 99), None);
    }

    #[test]
    fn a_flate_stream_counts_what_it_inflates_to() {
        let packed = stream(name("FlateDecode"), deflated_zeros(10_000));

        assert!(packed.content.len() < 100, "zeros compress well");
        assert_eq!(decoded_len(&packed, 10_000), Some(10_000));
        assert_eq!(decoded_len(&packed, 9_999), None);
    }

    #[test]
    fn the_count_agrees_with_what_lopdf_decodes() {
        let flate = stream(name("FlateDecode"), deflated_zeros(70_000));
        let lzw = stream(name("LZWDecode"), weezl_packed(&vec![b'a'; 5_000]));
        let ascii = stream(name("ASCII85Decode"), b"zzzz9jqo^~>".to_vec());
        let chain = stream(
            lopdf::Object::Array(vec![name("ASCII85Decode"), name("FlateDecode")]),
            ascii85_of(&deflated_zeros(4_000)),
        );

        for packed in [flate, lzw, ascii, chain] {
            let decoded = packed.decompressed_content().unwrap_or_default();

            assert!(!decoded.is_empty(), "lopdf must decode the fixture");
            assert_eq!(
                decoded_len(&packed, usize::MAX),
                Some(decoded.len().max(packed.content.len())),
                "{:?}",
                packed.dict
            );
        }
    }

    #[test]
    fn ascii85_decodes_to_the_bytes_lopdf_decodes() {
        let inputs: [&[u8]; 7] = [
            b"9jqo^BlbD-BleB1DJ+*+F(f,q~>",
            b"9jqo^ Bl\nbD-B leB1~>",
            b"zz9jqo^z~>",
            b"9jqo^Bl~>",
            b"9jqo^B",
            b"9jqo^\x7fignored~>",
            b"~>",
        ];

        for input in inputs {
            let by_lopdf = stream(name("ASCII85Decode"), input.to_vec())
                .decompressed_content()
                .unwrap_or_default();

            assert_eq!(
                decode_ascii85(input, usize::MAX),
                Some(by_lopdf),
                "{:?}",
                String::from_utf8_lossy(input)
            );
        }
    }

    #[test]
    fn ascii85_that_lopdf_rejects_decodes_to_nothing() {
        for input in [&b"9jz~>"[..], b"uuuuu~>"] {
            let by_lopdf = stream(name("ASCII85Decode"), input.to_vec()).decompressed_content();

            assert!(by_lopdf.is_err(), "lopdf must reject the fixture");
            assert_eq!(decode_ascii85(input, usize::MAX), Some(Vec::new()));
        }
    }

    #[test]
    fn a_run_of_ascii85_zeros_is_cut_off_at_the_cap() {
        let zeros = stream(name("ASCII85Decode"), vec![b'z'; 1_000]);

        assert_eq!(decoded_len(&zeros, 4_000), Some(4_000));
        assert_eq!(decoded_len(&zeros, 3_999), None);
    }

    #[test]
    fn an_lzw_stream_is_cut_off_at_the_cap() {
        let packed = stream(name("LZWDecode"), weezl_packed(&vec![b'a'; 100_000]));

        assert!(packed.content.len() < 2_000, "a run compresses well");
        assert_eq!(decoded_len(&packed, 100_000), Some(100_000));
        assert_eq!(decoded_len(&packed, 99_999), None);
    }

    #[test]
    fn a_filter_lopdf_cannot_decode_counts_the_stored_bytes() {
        let jpeg = stream(name("DCTDecode"), vec![1; 300]);
        let wrapped = stream(
            lopdf::Object::Array(vec![name("FlateDecode"), name("DCTDecode")]),
            deflated_zeros(5_000),
        );

        assert_eq!(decoded_len(&jpeg, 300), Some(300));
        assert_eq!(decoded_len(&wrapped, 5_000), Some(5_000));
        assert_eq!(decoded_len(&wrapped, 4_999), None);
    }

    #[test]
    fn corrupt_flate_data_counts_what_was_read_before_the_error() {
        let mut data = deflated_zeros(200_000);
        data.truncate(data.len() / 2);
        let packed = stream(name("FlateDecode"), data);

        let counted = decoded_len(&packed, usize::MAX).unwrap_or_default();

        assert!(
            counted > packed.content.len() && counted < 200_000,
            "counted {counted} from {} stored bytes",
            packed.content.len()
        );
    }

    #[test]
    fn the_budget_adds_up_every_stream_of_the_document() {
        let part = MAX_PDF_DECODED_BYTES / 4 + 1;
        let mut document = lopdf::Document::with_version("1.5");
        for _ in 0..3 {
            document.add_object(stream(name("FlateDecode"), deflated_zeros(part)));
        }

        assert!(within_budget(&document), "three parts fit");

        document.add_object(stream(name("FlateDecode"), deflated_zeros(part)));

        assert!(!within_budget(&document), "the fourth does not");
    }

    /// `data` packed the way a PDF `LZWDecode` stream is.
    fn weezl_packed(data: &[u8]) -> Vec<u8> {
        weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
            .encode(data)
            .expect("pack LZW fixture")
    }

    /// `data` as `ASCII85` text with its end marker.
    fn ascii85_of(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for group in data.chunks(4) {
            let mut bytes = [0_u8; 4];
            bytes[..group.len()].copy_from_slice(group);

            let mut value = u32::from_be_bytes(bytes);
            let mut digits = [0_u8; 5];
            for digit in digits.iter_mut().rev() {
                *digit = b'!' + u8::try_from(value % 85).unwrap_or(0);
                value /= 85;
            }
            out.extend_from_slice(&digits[..=group.len()]);
        }
        out.extend_from_slice(b"~>");
        out
    }
}
