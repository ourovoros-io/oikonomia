//! Width-preserving repair of stale PDF cross-reference offsets.
//!
//! Some PDF post-processors — bank statement stampers and signers are the
//! usual culprits — shift bytes after the cross-reference data was written
//! and leave stale offsets behind at two levels: the `startxref` / trailer
//! `/Prev` pointers to the tables, and the per-object offsets inside the
//! tables themselves. Desktop viewers rebuild silently; lopdf rejects the
//! file ("invalid file trailer") or reads garbage objects.
//!
//! Every rewrite here is padded with leading zeros to the original digit
//! width, so no byte moves and untouched offsets stay valid.

use std::collections::HashMap;
use std::ops::Range;

/// Largest input the repair reads. Matches the upload cap
/// ([`MAX_DOCUMENT_BYTES`](crate::documents::MAX_DOCUMENT_BYTES)), so every
/// stored document can be repaired and nothing larger is scanned.
const MAX_REPAIR_BYTES: usize = 8 * 1024 * 1024;

/// How many `startxref` and `/Prev` pointers are examined. Each one is an
/// incremental update of the file; real documents have a handful.
const MAX_XREF_POINTERS: usize = 256;

/// How many table entries one repair rewrites. A statement or invoice has
/// far fewer objects, and the bound keeps a crafted table from costing more
/// than it is worth.
const MAX_REPAIRED_ENTRIES: usize = 65_536;

/// Returns a patched copy of `data` if any stale xref offset was repaired.
///
/// The copy has the same length as `data`. Only the first
/// [`MAX_XREF_POINTERS`] table pointers are examined and at most
/// [`MAX_REPAIRED_ENTRIES`] table entries are rewritten.
///
/// `None` means nothing was repaired:
///
/// - no pointer and no entry was stale, or none could be matched to a
///   table or an object header;
/// - `data` is longer than [`MAX_REPAIR_BYTES`];
/// - the file has no classic `xref` table;
/// - the digits after a `startxref` or `/Prev` do not fit a `usize`;
/// - a corrected table pointer needs more digits than the original has.
pub(crate) fn repair_xref_offsets(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() > MAX_REPAIR_BYTES {
        return None;
    }

    let tables = xref_keyword_positions(data);
    if tables.is_empty() {
        return None;
    }

    let mut patched = data.to_vec();
    let mut repaired_any = false;
    for span in claimed_offset_spans(data)
        .into_iter()
        .take(MAX_XREF_POINTERS)
    {
        // The spans index `data` itself, so the lookup only fails to parse.
        let claimed = parse_ascii_usize(data.get(span.clone())?)?;

        if offset_points_at_table(data, claimed) {
            continue;
        }

        let nearest = *tables.iter().min_by_key(|p| p.abs_diff(claimed))?;

        if !patch_span(&mut patched, span, nearest) {
            return None;
        }
        repaired_any = true;
    }

    let mut entries = EntryRepair {
        data,
        headers: index_object_headers(data),
        remaining: MAX_REPAIRED_ENTRIES,
    };
    for &table in &tables {
        entries.repair_table(&mut patched, table);
    }
    repaired_any |= entries.remaining < MAX_REPAIRED_ENTRIES;

    repaired_any.then_some(patched)
}

/// Overwrites `span` with `value`, zero-padded to the span's width.
/// Fails (false) when the value needs more digits than the span holds.
fn patch_span(patched: &mut [u8], span: Range<usize>, value: usize) -> bool {
    let width = span.len();
    let formatted = format!("{value:0width$}");

    if formatted.len() != width {
        return false;
    }

    let Some(slot) = patched.get_mut(span) else {
        return false;
    };

    slot.copy_from_slice(formatted.as_bytes());
    true
}

/// The offsets of every `N G obj` header in a file, in ascending order, by
/// object number and generation.
type ObjectHeaders = HashMap<(usize, usize), Vec<usize>>;

/// One pass over the entries of a file's classic tables.
struct EntryRepair<'data> {
    /// The file as it was read; entries are parsed from it, never from the
    /// patched copy.
    data: &'data [u8],
    /// Where each object really starts.
    headers: ObjectHeaders,
    /// How many entries this pass may still rewrite.
    remaining: usize,
}

impl EntryRepair<'_> {
    /// Walks the table at `table` (the `xref` keyword position) and fixes
    /// every in-use entry whose offset does not land on its own `N G obj`
    /// header. An entry that cannot be fixed is skipped.
    ///
    /// The walk ends at the first thing that is not a subsection or an
    /// entry, at an object number that does not fit a `usize`, or when the
    /// pass has rewritten [`MAX_REPAIRED_ENTRIES`] entries.
    fn repair_table(&mut self, patched: &mut [u8], table: usize) {
        let data = self.data;
        let mut cursor = table + 4;

        loop {
            cursor += leading_whitespace(data, cursor);

            let Some((first_object, after)) = read_number(data, cursor) else {
                return;
            };
            let Some((count, after)) = read_number(data, after) else {
                return;
            };
            cursor = after;

            for index in 0..count {
                cursor += leading_whitespace(data, cursor);

                let Some(entry) = read_entry(data, cursor) else {
                    return;
                };
                cursor = entry.end;

                let Some(object) = first_object.checked_add(index) else {
                    return;
                };
                if entry.in_use && !self.repair_entry(patched, object, &entry) {
                    return;
                }
            }
        }
    }

    /// Rewrites the offset of one in-use entry when it is stale and the
    /// object's header can be found. Returns `false` once the pass has no
    /// rewrites left.
    fn repair_entry(&mut self, patched: &mut [u8], object: usize, entry: &Entry) -> bool {
        let Some(claimed) = self
            .data
            .get(entry.offset_span.clone())
            .and_then(parse_ascii_usize)
        else {
            return true;
        };
        if object_header_at(self.data, claimed, object, entry.generation) {
            return true;
        }

        let Some(actual) = nearest_header(&self.headers, object, entry.generation, claimed) else {
            return true;
        };
        if self.remaining == 0 {
            return false;
        }
        if patch_span(patched, entry.offset_span.clone(), actual) {
            self.remaining -= 1;
        }
        true
    }
}

/// One entry of a classic cross-reference table.
struct Entry {
    /// Where the offset's digits are in the file.
    offset_span: Range<usize>,
    /// The generation number the entry gives its object.
    generation: usize,
    /// `n` entries point at an object; `f` entries are free.
    in_use: bool,
    /// The position after the entry's keyword.
    end: usize,
}

/// Number of whitespace bytes at `at`; zero when `at` is past the end.
fn leading_whitespace(data: &[u8], at: usize) -> usize {
    data.get(at..)
        .map_or(0, |rest| count_while(rest, u8::is_ascii_whitespace))
}

/// Parses one xref entry at `at`: offset digits, generation digits, and the
/// `n`/`f` keyword.
fn read_entry(data: &[u8], at: usize) -> Option<Entry> {
    let (offset_span, after) = read_digits(data, at)?;

    let generation_at = after + leading_whitespace(data, after);
    let (generation_span, after_generation) = read_digits(data, generation_at)?;
    let generation = parse_ascii_usize(data.get(generation_span)?)?;

    let kind_at = after_generation + leading_whitespace(data, after_generation);
    let in_use = match data.get(kind_at)? {
        b'n' => true,
        b'f' => false,
        _ => return None,
    };

    Some(Entry {
        offset_span,
        generation,
        in_use,
        end: kind_at + 1,
    })
}

fn read_digits(data: &[u8], at: usize) -> Option<(Range<usize>, usize)> {
    let len = count_while(data.get(at..)?, u8::is_ascii_digit);
    if len == 0 {
        return None;
    }
    Some((at..at + len, at + len))
}

fn read_number(data: &[u8], at: usize) -> Option<(usize, usize)> {
    let (span, after) = read_digits(data, at)?;
    let value = parse_ascii_usize(data.get(span)?)?;

    Some((value, after + leading_whitespace(data, after)))
}

/// Does `offset` (after optional whitespace) hold the header of exactly
/// `object` at generation `generation`?
fn object_header_at(data: &[u8], offset: usize, object: usize, generation: usize) -> bool {
    let at = offset + leading_whitespace(data, offset);

    object_header_token(data, at) == Some((object, generation))
}

/// The object number and generation of the `N G obj` header that starts a
/// token at `at`: the file start or whitespace comes before it.
///
/// So `12 0 obj` inside `112 0 obj` is not a header of object 12.
fn object_header_token(data: &[u8], at: usize) -> Option<(usize, usize)> {
    let starts_token = at
        .checked_sub(1)
        .and_then(|before| data.get(before))
        .is_none_or(u8::is_ascii_whitespace);
    if !starts_token {
        return None;
    }

    parse_object_header(data.get(at..)?)
}

/// Parses a leading `N G obj` header into object number and generation.
///
/// `obj` must end a token: `1 0 object` is not a header.
fn parse_object_header(data: &[u8]) -> Option<(usize, usize)> {
    let (object_span, after_object) = read_digits(data, 0)?;
    let object = parse_ascii_usize(data.get(object_span)?)?;

    let gap = leading_whitespace(data, after_object);
    if gap == 0 {
        return None;
    }

    let (generation_span, after_generation) = read_digits(data, after_object + gap)?;
    let generation = parse_ascii_usize(data.get(generation_span)?)?;

    let gap = leading_whitespace(data, after_generation);
    if gap == 0 {
        return None;
    }

    let keyword = data.get(after_generation + gap..)?;
    let ends_token = keyword
        .get(3)
        .is_none_or(|after| !after.is_ascii_alphanumeric());

    (keyword.starts_with(b"obj") && ends_token).then_some((object, generation))
}

/// Finds every `N G obj` header in one pass over the file.
///
/// Scanning once keeps the repair linear in the file size however many
/// entries are stale.
fn index_object_headers(data: &[u8]) -> ObjectHeaders {
    let mut headers = ObjectHeaders::new();

    for (at, byte) in data.iter().enumerate() {
        if !byte.is_ascii_digit() {
            continue;
        }
        if let Some(key) = object_header_token(data, at) {
            headers.entry(key).or_default().push(at);
        }
    }

    headers
}

/// The header of `object` at `generation` nearest to the claimed offset; the
/// earlier one on a tie.
///
/// Post-processors shift bytes by small deltas, so proximity picks the right
/// revision when an object was redefined.
fn nearest_header(
    headers: &ObjectHeaders,
    object: usize,
    generation: usize,
    claimed: usize,
) -> Option<usize> {
    let offsets = headers.get(&(object, generation))?;

    // The offsets are ascending, so the nearest is one of the two around
    // the claimed position.
    let after = offsets.partition_point(|offset| *offset < claimed);
    let before = after.checked_sub(1).and_then(|index| offsets.get(index));

    before
        .into_iter()
        .chain(offsets.get(after))
        .copied()
        .min_by_key(|offset| offset.abs_diff(claimed))
}

/// Positions of standalone `xref` keywords (classic tables). The `xref`
/// inside `startxref` is preceded by `t`, so requiring leading whitespace
/// excludes it.
fn xref_keyword_positions(data: &[u8]) -> Vec<usize> {
    let mut positions = Vec::new();

    for (i, window) in data.windows(4).enumerate() {
        if window != b"xref" {
            continue;
        }

        let preceded_ok = i == 0 || data.get(i - 1).is_some_and(u8::is_ascii_whitespace);
        let followed_ok = data.get(i + 4).is_none_or(u8::is_ascii_whitespace);

        if preceded_ok && followed_ok {
            positions.push(i);
        }
    }

    positions
}

/// Byte ranges of the digit runs after every `startxref` keyword and every
/// trailer `/Prev` key.
fn claimed_offset_spans(data: &[u8]) -> Vec<Range<usize>> {
    let mut spans = Vec::new();

    for keyword in [&b"startxref"[..], &b"/Prev"[..]] {
        let mut from = 0;
        while let Some(found) = find_from(data, keyword, from) {
            let after = found + keyword.len();
            from = after;

            let digits_start = after + leading_whitespace(data, after);
            if let Some((span, _)) = read_digits(data, digits_start) {
                spans.push(span);
            }
        }
    }

    spans
}

/// Whether a claimed table offset is plausible: after optional whitespace it
/// lands on the `xref` keyword of a classic table or on any `N G obj`
/// header.
///
/// A cross-reference stream is an ordinary object, so its pointer lands on
/// a header. The header is not checked to be that of a cross-reference
/// stream: a pointer at any object is left alone.
fn offset_points_at_table(data: &[u8], offset: usize) -> bool {
    let Some(rest) = data.get(offset..) else {
        return false;
    };

    let rest = rest.trim_ascii_start();
    rest.starts_with(b"xref") || parse_object_header(rest).is_some()
}

fn find_from(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    data.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

fn count_while(data: &[u8], pred: impl Fn(&u8) -> bool) -> usize {
    data.iter().take_while(|b| pred(b)).count()
}

fn parse_ascii_usize(digits: &[u8]) -> Option<usize> {
    std::str::from_utf8(digits).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_startxref_and_prev_are_rewritten_in_place() {
        // "xref" really lives at offset 21; both references claim 7.
        let data = b"%PDF-1.4\n1 0 obj\nend\n\
              xref\n0 1\n0000000000 65535 f \n\
              trailer\n<</Prev 0000007>>\nstartxref\n0000007\n%%EOF";

        let repaired = repair_xref_offsets(data).unwrap();

        assert_eq!(
            repaired.len(),
            data.len(),
            "width-preserving: no byte shifts (and repair must apply)"
        );

        let text = String::from_utf8_lossy(&repaired);
        assert!(text.contains("/Prev 0000021>>"), "Prev patched: {text}");
        assert!(
            text.contains("startxref\n0000021\n"),
            "startxref patched: {text}"
        );
    }

    #[test]
    fn stale_entry_offset_is_rewritten_to_the_object_header() {
        // Object 1 really starts at offset 19, the entry claims 9.
        let data = b"%PDF-1.4\npadding..\n1 0 obj\nend\nendobj\n\
              xref\n0 2\n0000000000 65535 f \n0000000009 00000 n \n\
              trailer\n<<>>\nstartxref\n38\n%%EOF";

        assert_eq!(&data[19..26], b"1 0 obj", "test fixture geometry");

        let repaired = repair_xref_offsets(data).unwrap();
        assert!(!repaired.is_empty(), "repair must apply");

        let text = String::from_utf8_lossy(&repaired);
        assert!(text.contains("0000000019 00000 n"), "entry patched: {text}");
    }

    #[test]
    fn valid_offsets_are_left_alone() {
        let data = b"%PDF-1.4\n1 0 obj\nendobj\n\
              xref\n0 2\n0000000000 65535 f \n0000000009 00000 n \n\
              trailer\n<<>>\nstartxref\n24\n%%EOF";

        assert_eq!(&data[9..16], b"1 0 obj", "test fixture geometry");
        assert_eq!(&data[24..28], b"xref", "test fixture geometry");

        assert!(repair_xref_offsets(data).is_none(), "nothing to repair");
    }

    #[test]
    fn xref_stream_offsets_count_as_valid() {
        // startxref points at "12 0 obj" (a cross-reference stream), which
        // must not be treated as stale.
        let data = b"%PDF-1.5\nxref\n12 0 obj\n<<>>\nstartxref\n14\n%%EOF";

        assert_eq!(&data[14..22], b"12 0 obj", "test fixture geometry");

        assert!(repair_xref_offsets(data).is_none());
    }

    #[test]
    fn an_offset_into_the_middle_of_another_header_is_stale() {
        // Object 1 is at 25. Its entry claims 10: the second digit of
        // "11 0 obj", which reads as "1 0 obj" from there.
        let data = b"%PDF-1.4\n11 0 obj\nendobj\n1 0 obj\nendobj\n\
              xref\n1 1\n0000000010 00000 n \n\
              trailer\n<<>>\nstartxref\n40\n%%EOF";

        assert_eq!(&data[10..17], b"1 0 obj", "test fixture geometry");
        assert_eq!(&data[25..32], b"1 0 obj", "test fixture geometry");
        assert_eq!(&data[40..44], b"xref", "test fixture geometry");

        let repaired = repair_xref_offsets(data).unwrap();

        let text = String::from_utf8_lossy(&repaired);
        assert!(text.contains("0000000025 00000 n"), "entry patched: {text}");
    }

    #[test]
    fn a_word_that_only_starts_with_obj_is_not_a_header() {
        // "1 0 object" at 9 is text; the real header is at 20.
        let data = b"%PDF-1.4\n1 0 object\n1 0 obj\nendobj\n\
              xref\n1 1\n0000000009 00000 n \n\
              trailer\n<<>>\nstartxref\n35\n%%EOF";

        assert_eq!(&data[9..19], b"1 0 object", "test fixture geometry");
        assert_eq!(&data[20..27], b"1 0 obj", "test fixture geometry");
        assert_eq!(&data[35..39], b"xref", "test fixture geometry");

        let repaired = repair_xref_offsets(data).unwrap();

        let text = String::from_utf8_lossy(&repaired);
        assert!(text.contains("0000000020 00000 n"), "entry patched: {text}");
    }

    #[test]
    fn an_object_number_past_the_integer_range_stops_the_table_repair() {
        // The subsection claims to start at the largest object number, so
        // its second entry has no number at all.
        let data = format!(
            "%PDF-1.4\n1 0 obj\nendobj\nxref\n{} 2\n0000000000 65535 f \n0000000009 00000 n \n\
             trailer\n<<>>\nstartxref\n24\n%%EOF",
            usize::MAX
        );

        assert_eq!(repair_xref_offsets(data.as_bytes()), None);
    }

    /// A file of `objects` one-line objects whose table gives every one of
    /// them offset zero, and the true offset of each object.
    fn pdf_with_stale_entries(objects: usize) -> (Vec<u8>, Vec<usize>) {
        let mut data = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::with_capacity(objects);

        for number in 1..=objects {
            offsets.push(data.len());
            data.extend_from_slice(format!("{number} 0 obj\nendobj\n").as_bytes());
        }

        let table = data.len();
        data.extend_from_slice(format!("xref\n1 {objects}\n").as_bytes());
        for _ in 0..objects {
            data.extend_from_slice(b"0000000000 00000 n \n");
        }
        data.extend_from_slice(format!("trailer\n<<>>\nstartxref\n{table}\n%%EOF").as_bytes());

        (data, offsets)
    }

    /// The offsets a table of 20-byte entries holds, in order.
    fn entry_offsets(data: &[u8], entries: usize) -> Vec<usize> {
        let text = String::from_utf8_lossy(data);
        let (_, table) = text
            .split_once("xref\n")
            .expect("the fixture has a classic table");

        // The first line is the subsection header.
        table
            .lines()
            .skip(1)
            .take(entries)
            .filter_map(|entry| entry.get(..10)?.parse().ok())
            .collect()
    }

    #[test]
    fn every_stale_entry_of_a_large_table_is_repaired() {
        let objects = 5_000;
        let (data, offsets) = pdf_with_stale_entries(objects);

        let repaired = repair_xref_offsets(&data).unwrap();

        assert_eq!(repaired.len(), data.len(), "width-preserving");
        assert_eq!(entry_offsets(&repaired, objects), offsets);
    }

    #[test]
    fn repair_stops_at_the_entry_cap() {
        let objects = MAX_REPAIRED_ENTRIES + 10;
        let (data, offsets) = pdf_with_stale_entries(objects);

        let repaired = repair_xref_offsets(&data).unwrap();

        assert_eq!(repaired.len(), data.len(), "width-preserving");
        let after = entry_offsets(&repaired, objects);
        assert_eq!(
            after.get(..MAX_REPAIRED_ENTRIES),
            offsets.get(..MAX_REPAIRED_ENTRIES),
            "entries up to the cap are repaired"
        );
        assert_eq!(
            after.get(MAX_REPAIRED_ENTRIES..),
            Some(&[0; 10][..]),
            "entries past the cap keep their stale offset"
        );
    }

    #[test]
    fn a_redefined_object_is_repaired_to_the_revision_nearest_its_claimed_offset() {
        // Object 1 is defined at 9 and again at 28; the entry claims 30.
        let data = b"%PDF-1.4\n1 0 obj\nA\nendobj\n \n1 0 obj\nB\nendobj\n\
              xref\n1 1\n0000000030 00000 n \n\
              trailer\n<<>>\nstartxref\n45\n%%EOF";

        assert_eq!(&data[9..16], b"1 0 obj", "test fixture geometry");
        assert_eq!(&data[28..35], b"1 0 obj", "test fixture geometry");
        assert_eq!(&data[45..49], b"xref", "test fixture geometry");

        let repaired = repair_xref_offsets(data).unwrap();

        let text = String::from_utf8_lossy(&repaired);
        assert!(text.contains("0000000028 00000 n"), "entry patched: {text}");
    }

    #[test]
    fn refuses_when_corrected_offset_does_not_fit() {
        // Claimed offset has 1 digit; the real xref position needs more.
        let data = b"%PDF-1.4\npadding padding padding\n\
              xref\n0 1\n0000000000 65535 f \n\
              trailer\n<<>>\nstartxref\n7\n%%EOF";

        assert!(repair_xref_offsets(data).is_none());
    }
}
