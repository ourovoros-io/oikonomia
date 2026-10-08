//! Distrust of a reading made from OCR text in which glyphs are unknown.
//!
//! The bundled OCR models are Latin-only. On a Greek receipt they write `?`
//! for each letter they cannot place, and the reader then sees labels it does
//! not know and numbers it cannot place: the transfer fee came out as the
//! amount, and `A?O?EI?H ?YNA?AATHZ` as the payee. A reading from text like
//! that must not look sure of itself.
//!
//! [`InvoiceReading::drop_unreadable_ocr_fields`] applies two rules:
//!
//! - a merchant or a line-item description that contains an unknown glyph is
//!   removed, so no `?`-filled string reaches the form;
//! - when [`GARBLED_SHARE`] of the letters in the whole text are unknown, the
//!   amount is removed too, because the label that tells a total from a fee
//!   was probably one of the letters lost. The caller then says so.

use crate::documents::invoice::InvoiceReading;
use crate::documents::invoice::merchant::{Description, Merchant};

/// The glyphs OCR writes in place of a letter it cannot read: the question
/// mark of the `ocrs` recognizer and the Unicode replacement character.
const UNKNOWN_GLYPHS: [char; 2] = ['?', '\u{fffd}'];

/// The share of unknown glyphs among the letters and unknown glyphs of a
/// text at which most of it counts as unread.
///
/// Text in a Latin script has next to no `?` among its letters, and Greek
/// read by a Latin-only model has many. One fifth is a judgment call between
/// the two, leaning to distrust: a wrongly emptied amount costs the user a
/// keystroke, a wrong one that looks sure costs a bad entry.
/// `a_few_question_marks_do_not_make_a_text_garbled` and
/// `a_text_with_a_fifth_of_its_letters_unknown_is_garbled` pin each side.
pub(crate) const GARBLED_SHARE: f32 = 0.2;

/// Whether `text` holds a glyph OCR writes for a letter it cannot read.
fn has_unknown_glyph(text: &str) -> bool {
    text.contains(UNKNOWN_GLYPHS)
}

/// The share of unknown glyphs among the letters and unknown glyphs of
/// `text`; zero when it has neither.
fn unknown_share(text: &str) -> f32 {
    let (mut unknown, mut letters) = (0_u32, 0_u32);
    for character in text.chars() {
        if UNKNOWN_GLYPHS.contains(&character) {
            unknown += 1;
        } else if character.is_alphabetic() {
            letters += 1;
        }
    }
    let total = unknown + letters;
    if total == 0 {
        return 0.0;
    }
    // Both counts are at most the length of a document, far below the 2^24
    // a `f32` holds exactly.
    #[expect(
        clippy::cast_precision_loss,
        reason = "counts of characters of a document stay far below 2^24"
    )]
    let share = unknown as f32 / total as f32;
    share
}

impl InvoiceReading {
    /// Removes what an OCR text with unknown glyphs cannot support, and says
    /// whether most of `text` was unread.
    ///
    /// Call it only for a reading made from OCR text.
    pub(crate) fn drop_unreadable_ocr_fields(&mut self, text: &str) -> bool {
        if matches!(&self.merchant, Some(Merchant::Named(name)) if has_unknown_glyph(name)) {
            self.merchant = None;
        }
        if matches!(&self.description, Some(Description::LineItem(line)) if has_unknown_glyph(line))
        {
            self.description = None;
        }
        if self.reference.as_deref().is_some_and(has_unknown_glyph) {
            self.reference = None;
        }

        let garbled = unknown_share(text) >= GARBLED_SHARE;
        if garbled {
            self.amount_minor = None;
        }
        garbled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::invoice::read_invoice_text;
    use crate::prefs::Locale;

    /// A Latin-only OCR of a Greek invoice: the labels are gone, the digits
    /// and the Latin letters survive.
    const GARBLED_INVOICE: &str = "A?O?EI?H ?YNA?AATHZ\n\
        ?O?O ?O?OY 1,40\n\
        ?E?A?O? ?EI?O?H 310,00\n\
        20/08/2026\n";

    #[test]
    fn a_text_in_a_latin_script_is_not_garbled() {
        assert!(unknown_share("Invoice 12345\nTotal 310,00 EUR\nThank you") < GARBLED_SHARE);
    }

    #[test]
    fn a_few_question_marks_do_not_make_a_text_garbled() {
        let text = "Invoice for the order, did you mean this one?\nTotal 310,00 EUR\nPaid in full";

        assert!(unknown_share(text) < GARBLED_SHARE);
    }

    #[test]
    fn a_text_with_a_fifth_of_its_letters_unknown_is_garbled() {
        assert!(unknown_share("ab?cd ab?cd") >= GARBLED_SHARE);
        assert!(unknown_share(GARBLED_INVOICE) >= GARBLED_SHARE);
    }

    #[test]
    fn a_text_without_letters_has_no_unknown_share() {
        assert_eq!(unknown_share("12,34 / 5"), 0.0);
        assert_eq!(unknown_share(""), 0.0);
    }

    #[test]
    fn a_garbled_reading_loses_its_amount_and_its_question_mark_names() {
        let mut reading = read_invoice_text(GARBLED_INVOICE);

        let garbled = reading.drop_unreadable_ocr_fields(GARBLED_INVOICE);

        assert!(garbled);
        assert_eq!(reading.amount_minor, None);
        assert_eq!(reading.merchant_in(Locale::En), None);
        let description = reading.description_in(Locale::En).unwrap_or_default();
        assert!(!description.contains('?'), "{description}");
    }

    #[test]
    fn a_clean_reading_keeps_its_amount() {
        let text = "ACME Ltd\nInvoice 2026-0042\nTotal 310,00 EUR\n20/08/2026\n";
        let mut reading = read_invoice_text(text);
        let before = reading.clone();

        let garbled = reading.drop_unreadable_ocr_fields(text);

        assert!(!garbled);
        assert_eq!(reading, before);
        assert_eq!(reading.amount_minor, Some(31_000));
    }
}
