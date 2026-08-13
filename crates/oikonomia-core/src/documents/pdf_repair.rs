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

/// Return a patched copy of `data` if any stale xref offset was repaired.
///
/// `None` means nothing needed fixing or the file is beyond this repair
/// (no classic `xref` table at all, or a corrected table pointer does not
/// fit the original digit width).
pub(crate) fn repair_xref_offsets(data: &[u8]) -> Option<Vec<u8>> {
    const MAX_STALE_XREF_SPANS: usize = 256;

    let tables = xref_keyword_positions(data);
    if tables.is_empty() {
        return None;
    }

    let mut patched = data.to_vec();
    let mut repaired_any = false;
    for (i, span) in claimed_offset_spans(data).into_iter().enumerate() {
        if i >= MAX_STALE_XREF_SPANS {
            break;
        }
        let claimed = parse_ascii_usize(data.get(span.clone())?)?;

        if offset_points_at_xref(data, claimed) {
            continue;
        }

        let nearest = *tables.iter().min_by_key(|p| p.abs_diff(claimed))?;

        if !patch_span(&mut patched, span, nearest) {
            return None;
        }
        repaired_any = true;
    }

    for &table in &tables {
        repaired_any |= repair_table_entries(data, &mut patched, table);
    }

    repaired_any.then_some(patched)
}

/// Overwrite `span` with `value`, zero-padded to the span's width.
/// Fails (false) when the value needs more digits than the span holds.
fn patch_span(patched: &mut [u8], span: std::ops::Range<usize>, value: usize) -> bool {
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

/// Walk one classic table at `table` (the `xref` keyword position) and fix
/// every in-use entry whose offset does not land on its own `N G obj`
/// header. Unfixable entries are skipped rather than failing the repair.
fn repair_table_entries(data: &[u8], patched: &mut [u8], table: usize) -> bool {
    let mut repaired_any = false;
    let mut cursor = table + 4;

    loop {
        cursor += count_while(&data[cursor.min(data.len())..], u8::is_ascii_whitespace);

        let Some((first_object, after)) = read_number(data, cursor) else {
            break;
        };
        let Some((count, after)) = read_number(data, after) else {
            break;
        };
        cursor = after;

        for index in 0..count {
            cursor += count_while(&data[cursor.min(data.len())..], u8::is_ascii_whitespace);

            let Some((offset_span, generation, kind, after)) = read_entry(data, cursor) else {
                return repaired_any;
            };
            cursor = after;

            if kind != b'n' {
                continue;
            }

            let object = first_object + index;
            let Some(claimed) = data.get(offset_span.clone()).and_then(parse_ascii_usize) else {
                continue;
            };

            if object_header_at(data, claimed, object, generation) {
                continue;
            }

            let Some(actual) = find_object_header(data, object, generation, claimed) else {
                continue;
            };

            if patch_span(patched, offset_span, actual) {
                repaired_any = true;
            }
        }
    }

    repaired_any
}

/// Parse one xref entry at `at`: offset digits, generation digits, and the
/// `n`/`f` keyword. Returns the offset's byte span, the generation value,
/// the keyword, and the position after the keyword.
fn read_entry(data: &[u8], at: usize) -> Option<(std::ops::Range<usize>, usize, u8, usize)> {
    let (offset_span, after) = read_digits(data, at)?;

    let ws = count_while(data.get(after..)?, u8::is_ascii_whitespace);
    let (generation_span, after_generation) = read_digits(data, after + ws)?;
    let generation = parse_ascii_usize(data.get(generation_span)?)?;

    let ws = count_while(data.get(after_generation..)?, u8::is_ascii_whitespace);
    let kind_at = after_generation + ws;
    let kind = *data.get(kind_at)?;

    if kind != b'n' && kind != b'f' {
        return None;
    }

    Some((offset_span, generation, kind, kind_at + 1))
}

fn read_digits(data: &[u8], at: usize) -> Option<(std::ops::Range<usize>, usize)> {
    let len = count_while(data.get(at..)?, u8::is_ascii_digit);
    if len == 0 {
        return None;
    }
    Some((at..at + len, at + len))
}

fn read_number(data: &[u8], at: usize) -> Option<(usize, usize)> {
    let (span, after) = read_digits(data, at)?;
    let value = parse_ascii_usize(data.get(span)?)?;

    let ws = count_while(data.get(after..)?, u8::is_ascii_whitespace);
    Some((value, after + ws))
}

/// Does `offset` (after optional whitespace) hold the header of exactly
/// `object` at generation `generation`?
fn object_header_at(data: &[u8], offset: usize, object: usize, generation: usize) -> bool {
    let Some(rest) = data.get(offset..) else {
        return false;
    };
    let at = count_while(rest, u8::is_ascii_whitespace);

    parse_object_header(&rest[at..]) == Some((object, generation))
}

/// Parse a leading `N G obj` header.
fn parse_object_header(data: &[u8]) -> Option<(usize, usize)> {
    let (num_span, after) = read_digits(data, 0)?;
    let num = parse_ascii_usize(data.get(num_span)?)?;

    let ws = count_while(data.get(after..)?, u8::is_ascii_whitespace);
    if ws == 0 {
        return None;
    }

    let (generation_span, after_generation) = read_digits(data, after + ws)?;
    let generation = parse_ascii_usize(data.get(generation_span)?)?;

    let ws = count_while(data.get(after_generation..)?, u8::is_ascii_whitespace);
    if ws == 0 {
        return None;
    }

    data.get(after_generation + ws..)?
        .starts_with(b"obj")
        .then_some((num, generation))
}

/// Scan the whole file for `object generation obj` headers and return the one
/// nearest to the claimed offset (post-processors shift by small deltas,
/// so proximity picks the right revision when an object was redefined).
fn find_object_header(
    data: &[u8],
    object: usize,
    generation: usize,
    claimed: usize,
) -> Option<usize> {
    let needle = format!("{object} {generation} obj");
    let needle = needle.as_bytes();

    let mut best: Option<usize> = None;
    let mut from = 0;

    while let Some(found) = find_from(data, needle, from) {
        from = found + 1;

        let preceded_ok = found == 0 || data.get(found - 1).is_some_and(u8::is_ascii_whitespace);
        if !preceded_ok {
            continue;
        }

        let followed_ok = data
            .get(found + needle.len())
            .is_none_or(|b| !b.is_ascii_alphanumeric());
        if !followed_ok {
            continue;
        }

        best = match best {
            Some(prev) if prev.abs_diff(claimed) <= found.abs_diff(claimed) => Some(prev),
            _ => Some(found),
        };
    }

    best
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
fn claimed_offset_spans(data: &[u8]) -> Vec<std::ops::Range<usize>> {
    let mut spans = Vec::new();

    for keyword in [&b"startxref"[..], &b"/Prev"[..]] {
        let mut from = 0;
        while let Some(found) = find_from(data, keyword, from) {
            let after = found + keyword.len();
            from = after;

            let digits_start = after + count_while(&data[after..], u8::is_ascii_whitespace);
            let digits_len = count_while(&data[digits_start..], u8::is_ascii_digit);

            if digits_len > 0 {
                spans.push(digits_start..digits_start + digits_len);
            }
        }
    }

    spans
}

/// A claimed table offset is fine when (after optional whitespace) it lands
/// on a classic `xref` table or an `N G obj` cross-reference stream.
fn offset_points_at_xref(data: &[u8], offset: usize) -> bool {
    let Some(rest) = data.get(offset..) else {
        return false;
    };

    let rest = &rest[count_while(rest, u8::is_ascii_whitespace)..];
    if rest.starts_with(b"xref") {
        return true;
    }

    parse_object_header(rest).is_some()
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
        let data = b"%PDF-1.4\n1 0 obj\nend\nxref\n0 1\n0000000000 65535 f \ntrailer\n<</Prev 0000007>>\nstartxref\n0000007\n%%EOF";

        let repaired = repair_xref_offsets(data).unwrap_or_default();

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
        let data = b"%PDF-1.4\npadding..\n1 0 obj\nend\nendobj\nxref\n0 2\n0000000000 65535 f \n0000000009 00000 n \ntrailer\n<<>>\nstartxref\n38\n%%EOF";

        assert_eq!(&data[19..26], b"1 0 obj", "test fixture geometry");

        let repaired = repair_xref_offsets(data).unwrap_or_default();
        assert!(!repaired.is_empty(), "repair must apply");

        let text = String::from_utf8_lossy(&repaired);
        assert!(text.contains("0000000019 00000 n"), "entry patched: {text}");
    }

    #[test]
    fn valid_offsets_are_left_alone() {
        let data =
            b"%PDF-1.4\n1 0 obj\nendobj\nxref\n0 2\n0000000000 65535 f \n0000000009 00000 n \ntrailer\n<<>>\nstartxref\n24\n%%EOF";

        assert_eq!(&data[9..16], b"1 0 obj", "test fixture geometry");
        assert_eq!(&data[24..28], b"xref", "test fixture geometry");

        assert!(repair_xref_offsets(data).is_none(), "nothing to repair");
    }

    #[test]
    fn xref_stream_offsets_count_as_valid() {
        // startxref points at "12 0 obj" (a cross-reference stream), which
        // must not be treated as stale.
        let data = b"%PDF-1.5\nxref\n12 0 obj\n<<>>\nstartxref\n9\n%%EOF";

        assert!(repair_xref_offsets(data).is_none());
    }

    #[test]
    fn refuses_when_corrected_offset_does_not_fit() {
        // Claimed offset has 1 digit; the real xref position needs more.
        let data =
            b"%PDF-1.4\npadding padding padding\nxref\n0 1\n0000000000 65535 f \ntrailer\n<<>>\nstartxref\n7\n%%EOF";

        assert!(repair_xref_offsets(data).is_none());
    }
}
