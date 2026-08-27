//! Offline structured parsing for invoices / bills / receipts.
//!
//! Tuned for European (incl. Greek) tax documents: labeled totals, MARK/AFM
//! rejection, and sales-invoice vs expense detection. Greek bank transfer
//! receipts (`έμβασμα`) are a separate class: the labeled principal is the
//! amount, never a clock on `Ημερομηνία Αξίας` or the transfer fee. No network.

use super::analyze::{DocumentSuggestion, EntryKindSuggestion};

/// Parse extracted document text into a draft suggestion.
///
/// Bank transfer receipts stay [`EntryKindSuggestion::Expense`]. The
/// posted amount is the capital debit (`Ποσό Χρέωσης Κεφαλαίου` / `Ποσό:`),
/// not the fee and not a `hh:mm` time.
#[must_use]
pub fn parse_invoice_text(text: &str) -> DocumentSuggestion {
    let normalized = normalize(text);
    let lower_full = normalized.to_lowercase();
    if is_bank_transfer_receipt(&fold_greek(&lower_full)) {
        return parse_bank_transfer(&normalized);
    }

    let amount_minor = find_total_amount(&normalized, &lower_full);
    let entry_date = find_best_date(&normalized);
    let reference = find_invoice_reference(&normalized, &lower_full);
    let merchant = find_merchant(&normalized, &lower_full);
    let description = find_description(
        &normalized,
        &lower_full,
        merchant.as_deref(),
        reference.as_deref(),
    );
    let (kind, bill_unpaid) = classify_kind(&lower_full);

    let confidence = score_confidence(amount_minor, entry_date.as_ref(), reference.as_ref(), kind);

    DocumentSuggestion {
        source: super::analyze::AnalyzeSource::Heuristic,
        model: Some("invoice-parser-v1".into()),
        kind,
        amount_minor,
        entry_date,
        description,
        reference,
        merchant,
        bill_unpaid,
        category_account_id: None,
        wallet_account_id: None,
        payable_account_id: None,
        confidence,
        notes: build_notes(amount_minor, kind, bill_unpaid, &lower_full),
    }
}

fn normalize(text: &str) -> String {
    // Common OCR / PDF quirks before line-oriented parsing.
    let mut t = text.replace('\r', "\n");
    // No-break space, narrow no-break space, thin space
    t = t.replace(['\u{00a0}', '\u{202f}', '\u{2009}'], " ");
    // OCR often inserts spaces around decimal commas/dots: "72 , 53" / "72 . 53"
    t = collapse_spaced_decimals(&t);
    // Euro symbol variants — standalone tokens only, so EUROBANK stays intact.
    t = replace_eur_token(&t);

    t.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replace a standalone `EUR`/`eur` token with `€`.
///
/// A blanket `str::replace` corrupted words containing the trigram
/// (EUROBANK → €OBANK), which then leaked into merchant/description fields.
fn replace_eur_token(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let is_eur = i + 2 < chars.len()
            && chars[i].eq_ignore_ascii_case(&'e')
            && chars[i + 1].eq_ignore_ascii_case(&'u')
            && chars[i + 2].eq_ignore_ascii_case(&'r');
        let boundary_before = i == 0 || !chars[i - 1].is_alphabetic();
        let boundary_after = i + 3 >= chars.len() || !chars[i + 3].is_alphabetic();

        if is_eur && boundary_before && boundary_after {
            out.push('€');
            i += 3;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Collapse "72 , 53" / "72 . 53" → "72,53" / "72.53" (OCR spacing).
fn collapse_spaced_decimals(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if chars[i].is_ascii_digit() {
            // Collect a run that may include spaced separators: 1 234 , 56 or 72 , 53
            let start = i;
            let mut j = i;
            let mut buf = String::new();
            while j < len {
                let c = chars[j];
                if c.is_ascii_digit() {
                    buf.push(c);
                    j += 1;
                } else if (c == ',' || c == '.')
                    && j + 1 < len
                    && (chars[j + 1].is_ascii_digit()
                        || (chars[j + 1].is_whitespace()
                            && j + 2 < len
                            && chars[j + 2].is_ascii_digit()))
                {
                    buf.push(c);
                    j += 1;
                    while j < len && chars[j].is_whitespace() {
                        j += 1;
                    }
                } else if c.is_whitespace()
                    && j + 1 < len
                    && chars[j + 1].is_ascii_digit()
                    && !buf.is_empty()
                {
                    // thousand-space "1 234" — drop the space inside a number run
                    // only when next chunk is 3 digits (EU thousands) OR we already
                    // have a decimal sep in buf (unlikely). Conservative: skip lone spaces
                    // between digit groups of length 3.
                    let mut k = j + 1;
                    while k < len && chars[k].is_ascii_digit() {
                        k += 1;
                    }
                    let group_len = k - (j + 1);
                    if group_len == 3 {
                        j += 1; // skip space
                        continue;
                    }
                    break;
                } else {
                    break;
                }
            }
            if j > start {
                out.push_str(&buf);
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn is_utility_bill(lower: &str) -> bool {
    // Strong utility markers only. Loose tokens ("ηλεκτρ", "έναντι") used to
    // misfire on unrelated documents — a software company's own sales invoice
    // matched via "ΗΛΕΚΤΡΟΝΙΚΩΝ ΣΥΣΤΗΜΑΤΩΝ" in its line of business.
    lower.contains("kwh")
        || lower.contains("ρεύμα")
        || lower.contains("ρευμα")
        || lower.contains("εκκαθαριστικ")
        || lower.contains("δεδδηε")
        || lower.contains("ηκασπ")
        || lower.contains("φυσικού αερίου")
        || lower.contains("φυσικου αεριου")
        || lower.contains("φυσικό αέριο")
        || lower.contains("φυσικο αεριο")
        || lower.contains("προμήθεια φ.α")
        || lower.contains("προμηθεια φ.α")
        || lower.contains("χρέωση προμήθειας φ.α")
        || lower.contains("gas simple")
        || lower.contains("myon")
        || lower.contains("κωδικός παροχής")
        || lower.contains("κωδικος παροχης")
        || lower.contains("ύδρευσ")
        || lower.contains("υδρευσ")
        || lower.contains("power business")
}

/// Sales invoice issued by the book's owner: the counterparty block is
/// labeled "Στοιχεία Πελάτη".
fn is_sales_invoice(lower: &str) -> bool {
    (lower.contains("στοιχεία πελάτη") || lower.contains("στοιχεια πελατη"))
        && (lower.contains("τιμολόγιο") || lower.contains("τιμολογιο") || lower.contains("invoice"))
}

fn classify_kind(lower: &str) -> (EntryKindSuggestion, bool) {
    // Utility / electricity / water / gas settlement bills are expenses/bills for the customer.
    if is_utility_bill(lower) {
        let unpaid = lower.contains("ληξιπρόθεσμ")
            || lower.contains("ανεξόφλητ")
            || lower.contains("amount due");
        return (EntryKindSuggestion::Bill, unpaid);
    }

    // "Σταθερό Τιμολόγιο" is a tariff name, not a sales invoice.
    let sales = is_sales_invoice(lower) || lower.contains("sales invoice");

    let purchase = lower.contains("τιμολόγιο αγορ")
        || lower.contains("purchase invoice")
        || lower.contains("supplier");

    let unpaid = lower.contains("επί πιστώσει")
        || lower.contains("επι πιστωσει")
        || lower.contains("amount due")
        || lower.contains("unpaid")
        || lower.contains("outstanding")
        || lower.contains("please pay");

    if sales && !purchase {
        return (EntryKindSuggestion::Income, unpaid);
    }

    // A recognized biller with a known service (telecom etc.) is a bill to
    // pay even without the utility markers above.
    if let Some((_, Some(_))) = super::brands::known_brand(lower) {
        return (EntryKindSuggestion::Bill, unpaid);
    }

    if unpaid {
        (EntryKindSuggestion::Bill, true)
    } else {
        (EntryKindSuggestion::Expense, false)
    }
}

/// Fold monotonic Greek accents so label matching is accent-insensitive.
fn fold_greek(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ά' | 'ὰ' | 'ᾶ' | 'ἀ' | 'ἁ' | 'ᾳ' => 'α',
            'έ' | 'ὲ' | 'ἐ' | 'ἑ' => 'ε',
            'ή' | 'ὴ' | 'ῆ' | 'ἠ' | 'ἡ' | 'ῃ' => 'η',
            'ί' | 'ὶ' | 'ῖ' | 'ϊ' | 'ΐ' | 'ἰ' | 'ἱ' => 'ι',
            'ό' | 'ὸ' | 'ὀ' | 'ὁ' => 'ο',
            'ύ' | 'ὺ' | 'ῦ' | 'ϋ' | 'ΰ' | 'ὐ' | 'ὑ' => 'υ',
            'ώ' | 'ὼ' | 'ῶ' | 'ὠ' | 'ὡ' | 'ῳ' => 'ω',
            other => other,
        })
        .collect()
}

fn is_bank_transfer_receipt(folded: &str) -> bool {
    folded.contains("εμβασμα")
        || folded.contains("μεταφορα σε αλλη τραπεζα")
        || folded.contains("κωδικος συναλλαγης")
}

/// Expense fill for a Greek bank `έμβασμα` / other-bank transfer receipt.
fn parse_bank_transfer(text: &str) -> DocumentSuggestion {
    let amount_minor = find_transfer_principal(text);
    let entry_date = find_transfer_date(text);
    let reference = find_transfer_reference(text);
    let merchant = find_transfer_payee(text);
    let description = Some(match merchant.as_deref() {
        Some(payee) => format!("Έμβασμα — {payee}"),
        None => "Έμβασμα".to_owned(),
    });
    let fee_minor = find_transfer_fee(text);
    let kind = EntryKindSuggestion::Expense;
    let confidence = score_confidence(amount_minor, entry_date.as_ref(), reference.as_ref(), kind);

    DocumentSuggestion {
        source: super::analyze::AnalyzeSource::Heuristic,
        model: Some("invoice-parser-v1".into()),
        kind,
        amount_minor,
        entry_date,
        description,
        reference,
        merchant,
        bill_unpaid: false,
        category_account_id: None,
        wallet_account_id: None,
        payable_account_id: None,
        confidence,
        notes: build_transfer_notes(amount_minor, fee_minor),
    }
}

fn find_transfer_principal(text: &str) -> Option<i64> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let folded = fold_greek(&line.to_lowercase());
        if is_transfer_fee_line(&folded) {
            continue;
        }
        if folded.contains("ποσο χρεωσης κεφαλαιου")
            && let Some(amount) = amount_on_line_or_next(&lines, i)
        {
            return Some(amount);
        }
    }
    for (i, line) in lines.iter().enumerate() {
        let folded = fold_greek(&line.to_lowercase());
        if is_transfer_fee_line(&folded) {
            continue;
        }
        if is_transfer_poso_label(&folded)
            && let Some(amount) = amount_on_line_or_next(&lines, i)
        {
            return Some(amount);
        }
    }
    None
}

fn amount_on_line_or_next(lines: &[&str], index: usize) -> Option<i64> {
    for candidate in [lines[index], lines.get(index + 1).copied().unwrap_or("")] {
        if candidate.is_empty() {
            continue;
        }
        if let Some(amount) = money_amounts_on_line(candidate)
            .into_iter()
            .filter(|value| is_plausible_money(*value) && *value > 0)
            .max()
        {
            return Some(amount);
        }
    }
    None
}

fn is_transfer_fee_line(folded: &str) -> bool {
    folded.contains("προμηθεια") || folded.contains("εξοδων") || folded.contains("εξοδα")
}

fn is_transfer_poso_label(folded: &str) -> bool {
    let trimmed = folded.trim();
    trimmed == "ποσο"
        || trimmed.starts_with("ποσο:")
        || folded.contains("ποσο:")
        || (folded.contains("ποσο") && folded.contains(':') && !is_transfer_fee_line(folded))
}

fn find_transfer_fee(text: &str) -> Option<i64> {
    for line in text.lines() {
        let folded = fold_greek(&line.to_lowercase());
        if !is_transfer_fee_line(&folded) {
            continue;
        }
        if let Some(amount) = money_amounts_on_line(line)
            .into_iter()
            .filter(|value| is_plausible_money(*value) && *value > 0)
            .min()
        {
            return Some(amount);
        }
    }
    None
}

fn find_transfer_payee(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let folded = fold_greek(&line.to_lowercase());
        let beneficiary = folded.contains("δικαιουχου")
            || (folded.contains("ονοματεπωνυμο") && folded.contains("επωνυμια"));
        if !beneficiary {
            continue;
        }
        if let Some(name) = value_after_colon(line)
            && is_plausible_payee(&name)
        {
            return Some(name);
        }
        if let Some(next) = lines.get(i + 1)
            && is_plausible_payee(next)
        {
            return Some(next.trim().to_owned());
        }
    }
    None
}

fn is_plausible_payee(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.chars().count() < 3 || !trimmed.chars().any(char::is_alphabetic) {
        return false;
    }
    let folded = fold_greek(&trimmed.to_lowercase());
    if is_bank_counterparty(&folded) || folded.contains("iban") {
        return false;
    }
    let stripped = folded
        .replace("ονοματεπωνυμο", " ")
        .replace("επωνυμια", " ")
        .replace("δικαιουχου", " ")
        .replace(['/', ':', '：'], " ");
    stripped.chars().any(char::is_alphabetic)
}

fn is_bank_counterparty(folded: &str) -> bool {
    folded.contains("τραπεζα") || folded.contains("bank")
}

fn find_transfer_reference(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let folded = fold_greek(&line.to_lowercase());
        if !folded.contains("κωδικος συναλλαγης") {
            continue;
        }
        for candidate in [*line, lines.get(i + 1).copied().unwrap_or("")] {
            if let Some(code) = transfer_code_token(candidate) {
                return Some(code);
            }
        }
    }
    None
}

fn transfer_code_token(line: &str) -> Option<String> {
    for tok in line.split_whitespace() {
        let token = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if is_transfer_code(token) {
            return Some(token.to_ascii_uppercase());
        }
    }
    None
}

fn is_transfer_code(token: &str) -> bool {
    if token.len() < 10 || token.len() > 24 {
        return false;
    }
    if !token.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    if !token.chars().any(|c| c.is_ascii_alphabetic()) || !token.chars().any(|c| c.is_ascii_digit())
    {
        return false;
    }
    let upper = token.to_ascii_uppercase();
    if upper.starts_with("RF") && upper.chars().skip(2).all(|c| c.is_ascii_digit()) {
        return false;
    }
    // IBAN-shaped: two letters then only digits.
    if upper.len() >= 15
        && upper.chars().take(2).all(|c| c.is_ascii_alphabetic())
        && upper.chars().skip(2).all(|c| c.is_ascii_digit())
    {
        return false;
    }
    true
}

fn find_transfer_date(text: &str) -> Option<String> {
    let mut labeled: Option<String> = None;
    for line in text.lines() {
        if is_value_date_line(&line.to_lowercase()) {
            continue;
        }
        let Some(date) = first_date_on_line(line) else {
            continue;
        };
        let folded = fold_greek(&line.to_lowercase());
        if folded.contains("εκτελεσ") || folded.contains("execution") {
            return Some(date);
        }
        if labeled.is_none()
            && (folded.contains("ημερομην")
                || folded.contains("date")
                || folded.contains("συναλλαγ"))
        {
            labeled = Some(date);
        }
    }
    labeled.or_else(|| {
        text.lines().find_map(|line| {
            if is_value_date_line(&line.to_lowercase()) {
                None
            } else {
                first_date_on_line(line)
            }
        })
    })
}

fn first_date_on_line(line: &str) -> Option<String> {
    for token in line.split_whitespace() {
        let trimmed =
            token.trim_matches(|c: char| !c.is_ascii_digit() && c != '/' && c != '.' && c != '-');
        if let Some(iso) = parse_eu_date(trimmed).or_else(|| parse_iso_date(trimmed)) {
            return Some(iso);
        }
    }
    None
}

fn build_transfer_notes(amount: Option<i64>, fee_minor: Option<i64>) -> String {
    let mut parts = vec![
        "Parsed offline with the built-in invoice reader (no internet).".to_owned(),
        "Detected a bank transfer / εμβασμα receipt (expense).".to_owned(),
    ];
    if amount.is_none() {
        parts.push(
            "Could not confidently detect the transfer principal — please enter the amount.".into(),
        );
    }
    if let Some(fee) = fee_minor {
        parts.push(format!(
            "Transfer fee {} is shown on the receipt and is not the posted amount.",
            format_minor_comma(fee)
        ));
    }
    parts.join(" ")
}

fn format_minor_comma(minor: i64) -> String {
    let whole = minor / 100;
    let cents = minor.rem_euclid(100);
    format!("{whole},{cents:02}")
}

fn find_total_amount(text: &str, lower: &str) -> Option<i64> {
    // 1) Strong labeled totals always win — even when PDF extract is jumbled.
    //    (Utility frequency scoring used to run first and could pick date days as €.)
    if let Some(v) = find_labeled_total(text) {
        return Some(v);
    }

    // 2) Utility bills: vote among € amounts (with date tokens masked).
    if is_utility_bill(lower)
        && let Some(v) = find_utility_payment_total(text)
    {
        return Some(v);
    }

    // 3) Line containing "σύνολα" / "totals" — take the largest plausible amount
    for line in text.lines() {
        let line_l = line.to_lowercase();
        if is_noise_amount_line(&line_l) {
            continue;
        }
        if (line_l.contains("σύνολα") || line_l.contains("συνολα") || line_l.contains("totals"))
            && let Some(v) = money_amounts_on_line(line)
                .into_iter()
                .filter(|a| is_plausible_money(*a) && *a > 0)
                .max()
        {
            return Some(v);
        }
    }

    // 4) Weighted fallback (skip rate / mix / area lines)
    let mut weighted: Vec<(i64, i32)> = Vec::new();
    for line in text.lines() {
        let line_l = line.to_lowercase();
        if is_noise_amount_line(&line_l) {
            continue;
        }
        let mut w = 1;
        // `αξία` on invoices means line-value; `Ημερομηνία Αξίας` is a value
        // date and must not boost a clock (`7:00` → 700 minor).
        if !is_value_date_line(&line_l)
            && (line_l.contains("αξία") || line_l.contains("value") || line_l.contains("total"))
        {
            w += 3;
        }
        if line_l.contains('€') {
            w += 4;
        }
        if line_l.contains("iban")
            || line_l.contains("α.φ.μ")
            || line_l.contains("αφμ")
            || line_l.contains("mark")
        {
            w = 0;
        }
        if w == 0 {
            continue;
        }
        for a in money_amounts_on_line(line) {
            if is_plausible_money(a) {
                // Prefer amounts with cents over bare whole euros (less ID/date-like).
                let mut score = w;
                if a % 100 != 0 {
                    score += 2;
                }
                weighted.push((a, score));
            }
        }
    }
    weighted.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
    weighted.first().map(|(v, _)| *v)
}

const TOTAL_LABELS: &[&str] = &[
    "συνολικό ποσό πληρωμής",
    "συνολικο ποσο πληρωμης",
    "ποσό πληρωμής",
    "ποσο πληρωμης",
    "σύνολο τρέχοντος λογαριασμού",
    "συνολο τρεχοντος λογαριασμου",
    "τρέχοντος λογαριασμού",
    "τρεχοντος λογαριασμου",
    "πληρωτέο",
    "πληρωτεο",
    "payable",
    "amount due",
    "grand total",
    "total due",
    "amount payable",
    "amount to pay",
    "total to pay",
    "συνολ. αξία",
    "συνολική αξία",
    "συνολικη αξια",
    "τελ. αξία",
    "τελική αξία",
    "total amount",
    "invoice total",
    "net payable",
];

fn find_labeled_total(text: &str) -> Option<i64> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let line_l = line.to_lowercase();
        if is_noise_amount_line(&line_l) {
            continue;
        }
        if !TOTAL_LABELS.iter().any(|p| line_l.contains(p)) {
            continue;
        }
        // Same line first, then next line (labels and values often split in PDF extract).
        for candidate in [*line, lines.get(i + 1).copied().unwrap_or("")] {
            if candidate.is_empty() {
                continue;
            }
            let cand_l = candidate.to_lowercase();
            if is_noise_amount_line(&cand_l) && candidate != *line {
                continue;
            }
            if let Some(v) = money_amounts_on_line(candidate)
                .into_iter()
                .filter(|a| is_plausible_money(*a) && *a > 0)
                .max()
            {
                return Some(v);
            }
        }
    }
    None
}

/// Electricity/gas/water bills: payment total is usually a short line with `€`,
/// often repeated, and not on kWh/GWh/rate tables.
fn find_utility_payment_total(text: &str) -> Option<i64> {
    use std::collections::HashMap;

    let mut score: HashMap<i64, i32> = HashMap::new();
    let mut freq: HashMap<i64, i32> = HashMap::new();

    for line in text.lines() {
        let line_l = line.to_lowercase();
        if is_noise_amount_line(&line_l) {
            continue;
        }

        let amounts = money_amounts_on_line(line);
        if amounts.is_empty() {
            continue;
        }

        let short = line.chars().count() <= 40;
        let has_euro = line.contains('€');
        let has_date = line_has_date(line);
        let mostly_amount = is_amount_only_line(line);
        let pay_label = {
            let l = line_l.as_str();
            l.contains("πληρωμ")
                || l.contains("τρέχοντος")
                || l.contains("τρεχοντος")
                || l.contains("payable")
                || l.contains("amount due")
        };

        for a in amounts {
            if !is_plausible_money(a) {
                continue;
            }
            // Bare whole-euro integers (e.g. day-of-month 26 → €26) are almost never
            // the printed payment total on Greek utilities — those show cents.
            let has_cents = a % 100 != 0;
            if !has_cents && !mostly_amount && !pay_label {
                continue;
            }

            let mut s = 1;
            if has_euro {
                s += 12;
            }
            if has_cents {
                s += 16;
            } else {
                s -= 8;
            }
            if mostly_amount {
                s += 18;
            }
            if has_date && has_euro {
                s += 14;
            }
            if short && has_euro {
                s += 8;
            }
            if pay_label {
                s += 25;
            }
            // Typical monthly utility total band
            if (1_000..=50_000).contains(&a) {
                s += 6;
            }
            // Zone prices / large one-offs are often noise
            if a >= 100_000 {
                s -= 10;
            }

            *score.entry(a).or_insert(0) += s;
            *freq.entry(a).or_insert(0) += 1;
        }
    }

    // Frequency bonus (payment total is printed several times on Greek power bills)
    for (a, f) in &freq {
        if *f >= 2 {
            *score.entry(*a).or_insert(0) += f * 10;
        }
    }

    score
        .into_iter()
        .filter(|(_, s)| *s > 0)
        .max_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(a, _)| a)
}

fn is_noise_amount_line(line_l: &str) -> bool {
    // Deposits / guarantees are not the bill total
    if line_l.contains("εγγύηση") || line_l.contains("εγγυηση") || line_l.contains("deposit")
    {
        return true;
    }
    // Rate / volume / dimension lines (not payment total)
    line_l.contains("kwh")
        || line_l.contains("gwh")
        || line_l.contains("kva")
        || line_l.contains("/kwh")
        || line_l.contains("€/kwh")
        || line_l.contains("€/kva")
        || line_l.contains("τ.μ")
        || line_l.contains("τμ ")
        || line_l.contains("τιμή ζώνης")
        || line_l.contains("τιμη ζωνης")
        || line_l.contains("συντελεστ")
        || line_l.contains("λιγνιτ")
        || line_l.contains("υδροηλεκτ")
        || line_l.contains("διασύνδεσ")
        || line_l.contains("παραγωγ")
        // Gas volume / calorific tables (not the euro total)
        || line_l.contains("κατανάλωση") && line_l.contains('x')
        || line_l.contains("καταναλωση") && (line_l.contains('x') || line_l.contains('×'))
        || line_l.contains('%')
        || line_l.contains("x0,")
        || line_l.contains("x 0,")
        || line_l.contains("x0.")
        || line_l.contains('×')
}

fn is_value_date_line(line_l: &str) -> bool {
    let folded = fold_greek(line_l);
    folded.contains("ημερομηνια αξιας") || folded.contains("value date")
}

fn is_amount_only_line(line: &str) -> bool {
    // Ignore date tokens when deciding if the line is "just an amount".
    let stripped = mask_date_tokens(line);
    let cleaned: String = stripped
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '€' && *c != '.')
        .collect();
    // e.g. "76,65" or "76,65€"
    let digits_and_comma = !cleaned.is_empty()
        && cleaned
            .chars()
            .all(|c| c.is_ascii_digit() || c == ',' || c == '.');
    digits_and_comma && money_amounts_on_line(line).len() == 1
}

fn line_has_date(line: &str) -> bool {
    line.split_whitespace().any(|token| {
        let t =
            token.trim_matches(|c: char| !c.is_ascii_digit() && c != '/' && c != '.' && c != '-');
        parse_eu_date(t).or_else(|| parse_iso_date(t)).is_some()
    })
}

/// Blank out EU/ISO date tokens so day/month numbers are not parsed as euros.
///
/// Real PDF extracts often put `13/08/2026 72,53 €` on one line — without this,
/// `13`, `08`, and `2026` become €13 / €8 / €2026 candidates. Bank receipts
/// also print unpadded `27/8/2026`, which must not become €27 / €8.
fn mask_date_tokens(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if is_iso_date_at(&chars, i) {
            out.push_str("          ");
            i += 10;
            continue;
        }
        if let Some(len) = eu_date_len_at(&chars, i) {
            for _ in 0..len {
                out.push(' ');
            }
            i += len;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn is_iso_date_at(chars: &[char], i: usize) -> bool {
    i + 9 < chars.len()
        && chars[i].is_ascii_digit()
        && chars[i + 1].is_ascii_digit()
        && chars[i + 2].is_ascii_digit()
        && chars[i + 3].is_ascii_digit()
        && chars[i + 4] == '-'
        && chars[i + 5].is_ascii_digit()
        && chars[i + 6].is_ascii_digit()
        && chars[i + 7] == '-'
        && chars[i + 8].is_ascii_digit()
        && chars[i + 9].is_ascii_digit()
}

/// Length of a `d/m/yyyy` (or `dd.mm.yy`, …) token starting at `i`.
fn eu_date_len_at(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && chars[i - 1].is_ascii_digit() {
        return None;
    }
    if i >= chars.len() || !chars[i].is_ascii_digit() {
        return None;
    }

    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_digit() && j - i < 2 {
        j += 1;
    }
    if j == i || j >= chars.len() {
        return None;
    }
    let sep = chars[j];
    if sep != '/' && sep != '.' && sep != '-' {
        return None;
    }
    j += 1;

    let month_start = j;
    while j < chars.len() && chars[j].is_ascii_digit() && j - month_start < 2 {
        j += 1;
    }
    if j == month_start || j >= chars.len() || chars[j] != sep {
        return None;
    }
    j += 1;

    let year_start = j;
    while j < chars.len() && chars[j].is_ascii_digit() && j - year_start < 4 {
        j += 1;
    }
    let year_len = j - year_start;
    if year_len != 2 && year_len != 4 {
        return None;
    }
    if j < chars.len() && chars[j].is_ascii_digit() {
        return None;
    }

    let token: String = chars[i..j].iter().collect();
    parse_eu_date(&token).map(|_| j - i)
}

/// Blank out `h:mm` / `hh:mm` clocks so `7:00` is not parsed as €7.00.
fn mask_time_tokens(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if let Some(len) = time_len_at(&chars, i) {
            for _ in 0..len {
                out.push(' ');
            }
            i += len;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn time_len_at(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && chars[i - 1].is_ascii_digit() {
        return None;
    }
    if i >= chars.len() || !chars[i].is_ascii_digit() {
        return None;
    }

    let two_digit_hour = i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
    let (hour, after_hour) = if two_digit_hour {
        let hour = chars[i].to_digit(10)? * 10 + chars[i + 1].to_digit(10)?;
        if hour <= 23 && i + 2 < chars.len() && chars[i + 2] == ':' {
            (hour, i + 2)
        } else {
            (chars[i].to_digit(10)?, i + 1)
        }
    } else {
        (chars[i].to_digit(10)?, i + 1)
    };
    if hour > 23 || after_hour >= chars.len() || chars[after_hour] != ':' {
        return None;
    }

    let minute_at = after_hour + 1;
    if minute_at + 1 >= chars.len()
        || !chars[minute_at].is_ascii_digit()
        || !chars[minute_at + 1].is_ascii_digit()
    {
        return None;
    }
    let minutes = chars[minute_at].to_digit(10)? * 10 + chars[minute_at + 1].to_digit(10)?;
    if minutes > 59 {
        return None;
    }

    let mut j = minute_at + 2;
    if j + 2 < chars.len()
        && chars[j] == ':'
        && chars[j + 1].is_ascii_digit()
        && chars[j + 2].is_ascii_digit()
    {
        let seconds = chars[j + 1].to_digit(10)? * 10 + chars[j + 2].to_digit(10)?;
        if seconds <= 59 {
            j += 3;
        }
    }
    if j < chars.len() && chars[j].is_ascii_digit() {
        return None;
    }
    Some(j - i)
}

/// Accept amounts that look like currency, not AFM / invoice IDs / ZIPs.
fn is_plausible_money(minor: i64) -> bool {
    // 0.50 EUR .. 10_000_000.00 EUR
    (50..=1_000_000_000).contains(&minor)
}

fn money_amounts_on_line(line: &str) -> Vec<i64> {
    let line = mask_time_tokens(&mask_date_tokens(line));
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in line.chars() {
        if ch.is_ascii_digit() || ch == '.' || ch == ',' {
            buf.push(ch);
        } else {
            if let Some(v) = parse_money_token(&buf) {
                out.push(v);
            }
            buf.clear();
        }
    }
    if let Some(v) = parse_money_token(&buf) {
        out.push(v);
    }
    out
}

fn parse_money_token(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() || s.len() > 14 {
        return None;
    }

    // Pure long digit strings without decimal are IDs (AFM, MARK, IBAN fragments), not money.
    let has_decimal = s.contains(',') || s.contains('.');
    if !has_decimal {
        // Integers only if short enough to be whole euros (e.g. "50").
        // Cap at 5 digits so long IDs never become money.
        if s.len() > 5 {
            return None;
        }
        // Leading-zero numbers (08, 06) are date fragments, not euro amounts.
        if s.len() > 1 && s.starts_with('0') {
            return None;
        }
        let whole: i64 = s.parse().ok()?;
        // 4-digit years leak from unmasked date fragments — not money.
        if (1_900..=2_100).contains(&whole) {
            return None;
        }
        return whole.checked_mul(100);
    }

    let last_comma = s.rfind(',');
    let last_dot = s.rfind('.');
    let normalized = if let (Some(c), Some(d)) = (last_comma, last_dot) {
        if c > d {
            // 1.234,56
            s.replace('.', "").replace(',', ".")
        } else {
            // 1,234.56
            s.replace(',', "")
        }
    } else if last_comma.is_some() {
        let parts: Vec<_> = s.split(',').collect();
        if parts.len() == 2 && (1..=2).contains(&parts[1].len()) {
            format!("{}.{}", parts[0].replace('.', ""), parts[1])
        } else {
            // thousands commas only
            s.replace(',', "")
        }
    } else {
        // dots only: 1860.00 or 1.234.567
        let parts: Vec<_> = s.split('.').collect();
        if parts.len() == 2 && (1..=2).contains(&parts[1].len()) {
            s.to_owned()
        } else if parts.len() > 2 {
            // thousand separators: drop dots
            s.replace('.', "")
        } else {
            s.to_owned()
        }
    };

    let parts: Vec<_> = normalized.split('.').collect();
    if parts.len() > 2 || parts[0].is_empty() {
        return None;
    }
    // Whole part too long → ID
    if parts[0].len() > 8 {
        return None;
    }
    let whole: i64 = parts[0].parse().ok()?;
    let frac = if parts.len() == 2 {
        let f = parts[1];
        if f.is_empty() || f.len() > 2 {
            return None;
        }
        format!("{f:0<2}").parse::<i64>().ok()?
    } else {
        0
    };
    whole.checked_mul(100)?.checked_add(frac)
}

fn find_best_date(text: &str) -> Option<String> {
    // Prefer due/payment date on the same line as a € amount (utility payment slips).
    for line in text.lines() {
        if line.contains('€')
            && line_has_date(line)
            && let Some(iso) = first_date_on_line(line)
        {
            return Some(iso);
        }
    }

    for line in text.lines() {
        let l = line.to_lowercase();
        if (l.contains("ημερομην")
            || l.contains("date")
            || l.contains("έκδοσ")
            || l.contains("ληξ")
            || l.contains("due"))
            && let Some(iso) = first_date_on_line(line)
        {
            return Some(iso);
        }
    }

    text.lines().find_map(first_date_on_line)
}

fn parse_iso_date(s: &str) -> Option<String> {
    let parts: Vec<_> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let d: u32 = parts[2].parse().ok()?;
    if (1990..=2100).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d) {
        Some(format!("{y:04}-{m:02}-{d:02}"))
    } else {
        None
    }
}

fn parse_eu_date(s: &str) -> Option<String> {
    let sep = if s.contains('/') {
        '/'
    } else if s.contains('.') {
        '.'
    } else if s.contains('-') {
        '-'
    } else {
        return None;
    };
    let parts: Vec<_> = s.split(sep).collect();
    if parts.len() != 3 {
        return None;
    }
    let d: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let mut y: i32 = parts[2].parse().ok()?;
    if y < 100 {
        y += 2000;
    }
    if (1990..=2100).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d) {
        Some(format!("{y:04}-{m:02}-{d:02}"))
    } else {
        None
    }
}

fn find_invoice_reference(text: &str, lower: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();

    // Supply / meter codes (e.g. NGS000000001) — value often on the next line in PDF extract.
    for (i, line) in lines.iter().enumerate() {
        let l = line.to_lowercase();
        if l.contains("κωδικός παροχής")
            || l.contains("κωδικος παροχης")
            || l.contains("supply")
            || l.contains("ηκασπ")
        {
            for candidate in [*line, lines.get(i + 1).copied().unwrap_or("")] {
                if let Some(code) = alnum_supply_code(candidate) {
                    return Some(code);
                }
            }
        }
    }

    // Standalone supply-style codes (NGS…, etc.) anywhere in the body.
    for line in &lines {
        if let Some(code) = alnum_supply_code(line) {
            let up = code.to_ascii_uppercase();
            if up.starts_with("NGS") || up.starts_with("ΗΚΑΣ") {
                return Some(code);
            }
        }
    }

    // RF payment code on Greek utility bills
    for line in &lines {
        for tok in line.split_whitespace() {
            let t = tok.trim();
            let up = t.to_ascii_uppercase();
            if up.len() >= 10
                && up.starts_with("RF")
                && up.chars().skip(2).all(|c| c.is_ascii_digit())
            {
                return Some(up);
            }
        }
        // PDF extract may glue RF to neighbouring text without spaces.
        if let Some(idx) = line.to_ascii_uppercase().find("RF") {
            let slice: String = line
                .chars()
                .skip(idx)
                .take_while(char::is_ascii_alphanumeric)
                .collect();
            let up = slice.to_ascii_uppercase();
            if up.len() >= 10
                && up.starts_with("RF")
                && up.chars().skip(2).all(|c| c.is_ascii_digit())
            {
                return Some(up);
            }
        }
    }
    let _ = lower;

    // MARK number on Greek invoices (long digit string near MARK / Α.Α.)
    for (i, line) in text.lines().enumerate() {
        let l = line.to_lowercase();
        if l.contains("μαρκ") || l.contains("mark") || l.contains("α.α") || l.contains("αα ")
        {
            if let Some(n) = long_digit_token(line) {
                return Some(n);
            }
            // sometimes values are on the next line
            if let Some(next) = text.lines().nth(i + 1)
                && let Some(n) = long_digit_token(next)
            {
                return Some(n);
            }
        }
    }

    // Explicit invoice / ref labels
    for line in text.lines() {
        let l = line.to_lowercase();
        if (l.contains("invoice")
            || l.contains("αρ. παραστατ")
            || l.contains("αριθμός")
            || l.contains("number")
            || l.contains("ref"))
            && let Some(n) = long_digit_token(line)
        {
            return Some(n);
        }
    }

    // Fallback: longest digit run that looks like an invoice id (10–20 digits)
    let mut best: Option<String> = None;
    for line in text.lines() {
        if let Some(n) = long_digit_token(line)
            && n.len() >= 10
            && best.as_ref().is_none_or(|b| n.len() > b.len())
        {
            best = Some(n);
        }
    }
    best
}

fn long_digit_token(line: &str) -> Option<String> {
    let mut best: Option<String> = None;
    let mut buf = String::new();

    let take = |buf: &mut String, best: &mut Option<String>| {
        if buf.len() >= 6 && buf.len() <= 20 && best.as_ref().is_none_or(|b| buf.len() >= b.len()) {
            *best = Some(buf.clone());
        }
        buf.clear();
    };

    for ch in line.chars() {
        if ch.is_ascii_digit() {
            buf.push(ch);
        } else {
            take(&mut buf, &mut best);
        }
    }
    take(&mut buf, &mut best);
    best
}

/// Alphanumeric supply / point-of-delivery codes (e.g. `NGS000000001`).
fn alnum_supply_code(line: &str) -> Option<String> {
    for tok in line.split_whitespace() {
        let t = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if t.len() >= 8
            && t.len() <= 24
            && t.chars().any(|c| c.is_ascii_alphabetic())
            && t.chars().any(|c| c.is_ascii_digit())
            && t.chars().all(|c| c.is_ascii_alphanumeric())
        {
            // Skip pure RF payment refs (handled separately) and obvious words.
            let up = t.to_ascii_uppercase();
            if up.starts_with("RF") && up.chars().skip(2).all(|c| c.is_ascii_digit()) {
                continue;
            }
            return Some(t.to_owned());
        }
    }
    // Also scan without whitespace (jumbled extract: "NGS000000001" alone is fine).
    let mut buf = String::new();
    for ch in line.chars() {
        if ch.is_ascii_alphanumeric() {
            buf.push(ch);
        } else {
            if let Some(c) = take_supply_buf(&buf) {
                return Some(c);
            }
            buf.clear();
        }
    }
    take_supply_buf(&buf)
}

fn take_supply_buf(buf: &str) -> Option<String> {
    if buf.len() >= 8
        && buf.len() <= 24
        && buf.chars().any(|c| c.is_ascii_alphabetic())
        && buf.chars().any(|c| c.is_ascii_digit())
    {
        let up = buf.to_ascii_uppercase();
        if up.starts_with("RF") && up.chars().skip(2).all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some(buf.to_owned())
    } else {
        None
    }
}

fn find_merchant(text: &str, lower: &str) -> Option<String> {
    // Outgoing sales invoice: the counterparty is the customer. This runs
    // before brand recognition because the issuer's payment footer often
    // names a bank ("PIRAEUS BANK, IBAN …") that must not win.
    if is_sales_invoice(lower)
        && let Some(customer) = sales_invoice_customer(text)
    {
        return Some(customer);
    }

    // Known billers: brand tokens that survive text extraction.
    if let Some((brand, _)) = super::brands::known_brand(lower) {
        return Some(brand.to_owned());
    }

    // Unrecognized utility supplier: generic but honest labels.
    if is_utility_bill(lower) {
        if lower.contains("φυσικού αερίου")
            || lower.contains("φυσικου αεριου")
            || lower.contains("φυσικό αέριο")
            || lower.contains("gas simple")
            || lower.contains("προμήθεια φ.α")
        {
            return Some("Natural gas".into());
        }
        if lower.contains("power business") {
            return Some("Electricity supplier".into());
        }
    }

    // Issuer: first Επωνυμία value
    for line in text.lines() {
        let l = line.to_lowercase();
        if l.contains("επωνυμία") || l.contains("επωνυμια") {
            if let Some(name) = value_after_colon(line)
                && name.chars().count() >= 3
            {
                return Some(name);
            }
            // same line after spaces
            let cleaned = line
                .split_whitespace()
                .skip_while(|w| {
                    let w = w.to_lowercase();
                    w.contains("επων") || w == ":"
                })
                .collect::<Vec<_>>()
                .join(" ");
            if cleaned.chars().count() >= 3 {
                return Some(cleaned);
            }
        }
    }

    text.lines()
        .map(str::trim)
        .find(|l| {
            l.chars().count() >= 5
                && l.chars().count() <= 80
                && l.chars().any(char::is_alphabetic)
                && !l.to_lowercase().contains("τιμολόγιο")
        })
        .map(ToOwned::to_owned)
}

/// Customer name from the "Στοιχεία Πελάτη" block of a sales invoice.
fn sales_invoice_customer(text: &str) -> Option<String> {
    let mut after_client = false;

    for line in text.lines() {
        let l = line.to_lowercase();

        if l.contains("στοιχεία πελάτη") || l.contains("στοιχεια πελατη") || l.contains("customer")
        {
            after_client = true;
            continue;
        }
        if !after_client {
            continue;
        }

        if (l.contains("επωνυμία") || l.contains("επωνυμια") || l.starts_with("name"))
            && let Some(name) = value_after_colon(line)
            && name.chars().count() >= 3
        {
            return Some(name);
        }

        // Next substantial non-label line
        if !l.contains("α.φ.μ")
            && !l.contains("αφμ")
            && !l.contains("διεύθυν")
            && line.chars().count() >= 5
            && line.chars().any(char::is_alphabetic)
            && !l.ends_with(':')
        {
            return Some(line.trim().to_owned());
        }
    }

    None
}

fn value_after_colon(line: &str) -> Option<String> {
    let (idx, ch) = line.char_indices().find(|(_, c)| *c == ':' || *c == '：')?;
    let v = line[idx + ch.len_utf8()..].trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_owned())
    }
}

fn find_description(
    text: &str,
    lower: &str,
    merchant: Option<&str>,
    reference: Option<&str>,
) -> Option<String> {
    // Outgoing sales invoice: customer-first title.
    if is_sales_invoice(lower)
        && let Some(m) = merchant
    {
        return Some(match reference {
            Some(r) => format!("{m} — Invoice {r}"),
            None => format!("{m} — Invoice"),
        });
    }

    // Recognized biller or utility bill: company-first title with the
    // service decided by weighted scoring, never by a single keyword.
    let brand_service = super::brands::known_brand(lower).and_then(|(_, service)| service);

    if is_utility_bill(lower) || brand_service.is_some() {
        let service = brand_service.or_else(|| super::brands::classify_service(lower));
        let label = service.map_or("Utility", super::brands::Service::label);

        return Some(match merchant {
            Some(m) => format!("{m} — {label} bill"),
            None => format!("{label} bill"),
        });
    }

    // Line-item description under Περιγραφή
    let mut after_header = false;
    for line in text.lines() {
        let l = line.to_lowercase();
        if l.contains("περιγραφή") || l.contains("description") {
            after_header = true;
            continue;
        }
        if after_header {
            // skip table noise / numbers-only
            let alpha: String = line
                .chars()
                .filter(|c| c.is_alphabetic() || c.is_whitespace())
                .collect();
            let alpha = alpha.trim();
            if alpha.chars().count() >= 4
                && !alpha.to_lowercase().contains("ποσότητα")
                && !alpha.to_lowercase().contains("quantity")
            {
                return Some(alpha.to_owned());
            }
        }
    }

    if let (Some(m), Some(r)) = (merchant, reference) {
        return Some(format!("Invoice {r} — {m}"));
    }
    if let Some(r) = reference {
        return Some(format!("Invoice {r}"));
    }
    if lower.contains("τιμολόγιο") {
        return Some("Invoice".into());
    }
    merchant.map(ToOwned::to_owned)
}

fn score_confidence(
    amount: Option<i64>,
    date: Option<&String>,
    reference: Option<&String>,
    kind: EntryKindSuggestion,
) -> f32 {
    let mut c = 0.2_f32;
    if amount.is_some() {
        c += 0.4;
    }
    if date.is_some() {
        c += 0.15;
    }
    if reference.is_some() {
        c += 0.1;
    }
    if matches!(
        kind,
        EntryKindSuggestion::Income | EntryKindSuggestion::Bill
    ) {
        c += 0.05;
    }
    c.min(0.95)
}

fn build_notes(
    amount: Option<i64>,
    kind: EntryKindSuggestion,
    unpaid: bool,
    lower: &str,
) -> String {
    let mut parts =
        vec!["Parsed offline with the built-in invoice reader (no internet).".to_owned()];
    if amount.is_none() {
        parts.push("Could not confidently detect a total — please enter the amount.".into());
    }
    if matches!(kind, EntryKindSuggestion::Income) {
        parts.push("Detected a sales/service invoice (income).".into());
    }
    if is_utility_bill(lower) {
        parts.push("Detected a utility / electricity bill (expense).".into());
    }
    if unpaid {
        parts.push("Marked as credit terms / amount due.".into());
    }
    if lower.contains("χωρίς φπα") || lower.contains("0%") {
        parts.push("VAT appears zero / exempt.".into());
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Load a week-1 corpus fixture so unit tests share the public golden tree.
    fn corpus_text(relative: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/documents")
            .join(relative);
        #[expect(clippy::expect_used, reason = "fixture tests fail loudly by design")]
        {
            std::fs::read_to_string(&path).expect("corpus fixture")
        }
    }

    #[test]
    fn greek_service_invoice_total_and_kind() {
        let s = parse_invoice_text(&corpus_text("synthetic/text/greek_sales_invoice.txt"));
        assert_eq!(s.amount_minor, Some(186_000), "expected €1860.00");
        assert_eq!(s.kind, EntryKindSuggestion::Income);
        assert_eq!(s.entry_date.as_deref(), Some("2026-06-25"));
        assert_eq!(s.reference.as_deref(), Some("900000000000001"));
        assert!(
            s.merchant
                .as_deref()
                .is_some_and(|m| m.contains("ACME CONSULTING")),
            "merchant={:?}",
            s.merchant
        );
        assert!(s.bill_unpaid, "Επί πιστώσει should mark unpaid/credit");
    }

    #[test]
    fn received_service_invoice_is_not_income() {
        let text = "\
Τιμολόγιο Παροχής Υπηρεσιών
Επωνυμία: ACME ΛΟΓΙΣΤΙΚΗ ΙΚΕ
Α.Φ.Μ.: 000000000
Πληρωτέο (€): 200,00
";
        let s = parse_invoice_text(text);
        assert_ne!(s.kind, EntryKindSuggestion::Income, "kind={:?}", s.kind);
    }

    #[test]
    fn value_after_fullwidth_colon_does_not_panic() {
        assert_eq!(value_after_colon("Name：ACME LTD"), Some("ACME LTD".into()));
        assert_eq!(value_after_colon("Name: ACME LTD"), Some("ACME LTD".into()));
    }

    #[test]
    fn settlement_bill_is_not_automatically_unpaid() {
        let s = parse_invoice_text(&corpus_text("synthetic/text/dei_settlement.txt"));
        assert!(
            !s.bill_unpaid,
            "εμπρόθεσμο/εκκαθαριστικό/εξόφληση μέσω must not force unpaid"
        );
    }

    #[test]
    fn cosmote_pay_via_is_not_unpaid() {
        let s = parse_invoice_text(&corpus_text("synthetic/text/cosmote_pay_via.txt"));
        assert_eq!(s.kind, EntryKindSuggestion::Bill);
        assert!(
            !s.bill_unpaid,
            "known-brand εξόφληση μέσω must not force unpaid: {s:?}"
        );
    }

    #[test]
    fn eur_token_replacement_keeps_words() {
        assert_eq!(replace_eur_token("TOTAL 10 EUR"), "TOTAL 10 €");
        assert_eq!(replace_eur_token("10eur"), "10€");
        assert_eq!(replace_eur_token("EUROBANK EUROPE"), "EUROBANK EUROPE");
        assert_eq!(replace_eur_token("EUR"), "€");
    }

    #[test]
    fn rejects_afm_as_money() {
        assert_eq!(parse_money_token("000000000"), None);
        assert_eq!(parse_money_token("900000000000001"), None);
        assert_eq!(parse_money_token("1860,00"), Some(186_000));
    }

    #[test]
    fn utility_titles_are_company_first() {
        // Gas bill whose issuer only appears via the MyON portal branding.
        let gas = parse_invoice_text(&corpus_text("synthetic/text/volton_myon_gas.txt"));
        assert_eq!(gas.merchant.as_deref(), Some("Volton"));
        assert_eq!(gas.description.as_deref(), Some("Volton — Gas bill"));

        // Telecom bill: brand implies the service without utility markers.
        let telecom = parse_invoice_text(&corpus_text("synthetic/text/nova_telecom.txt"));
        assert_eq!(telecom.merchant.as_deref(), Some("Nova"));
        assert_eq!(telecom.description.as_deref(), Some("Nova — Telecom bill"));
        assert_eq!(telecom.kind, EntryKindSuggestion::Bill);
    }

    #[test]
    fn electricity_supplier_beats_grid_operator_and_energy_mix() {
        // Every Greek electricity bill mentions ΔΕΔΔΗΕ (grid operator) and a
        // national energy-mix table that includes natural gas; neither may
        // decide the title.
        let s = parse_invoice_text(&corpus_text("synthetic/text/zenith_supplier_vs_grid.txt"));
        assert_eq!(s.merchant.as_deref(), Some("ZeniΘ"));
        assert_eq!(s.description.as_deref(), Some("ZeniΘ — Electricity bill"));
    }

    #[test]
    fn sales_invoice_titles_carry_the_customer() {
        let s = parse_invoice_text(
            "Επωνυμία ACME ΛΟΓΙΣΤΙΚΗ ΙΚΕ\n\
             Τιμολόγιο Παροχής / Ενδοκοινοτική Παροχή Υπηρεσιών\n\
             900000000000001 Επί πιστώσειB 51 25/06/2026\n\
             Στοιχεία Πελάτη\nΑ.Φ.Μ.: 000000000\nΕπωνυμία: ACME CONSULTING LTD\n\
             Πληρωτέο (€): 1860,00",
        );
        assert_eq!(s.kind, EntryKindSuggestion::Income);
        assert_eq!(s.merchant.as_deref(), Some("ACME CONSULTING LTD"));
        assert!(
            s.description
                .as_deref()
                .is_some_and(|d| d.starts_with("ACME CONSULTING LTD — Invoice")),
            "description={:?}",
            s.description
        );

        // The issuer's payment footer must not hijack the merchant.
        let with_bank = parse_invoice_text(
            "Τιμολόγιο Παροχής Υπηρεσιών\nΣτοιχεία Πελάτη\nΕπωνυμία: ACME CONSULTING LTD\n\
             Πληρωτέο (€): 500,00\nPIRAEUS BANK, GREECE, IBAN: GR0000000000000000000000000",
        );
        assert_eq!(with_bank.merchant.as_deref(), Some("ACME CONSULTING LTD"));
    }

    #[test]
    fn zenith_electricity_bill_total() {
        let s = parse_invoice_text(&corpus_text("synthetic/text/zenith_electricity.txt"));
        assert_eq!(
            s.amount_minor,
            Some(7_665),
            "expected €76.65, got {:?}",
            s.amount_minor
        );
        assert!(
            matches!(
                s.kind,
                EntryKindSuggestion::Bill | EntryKindSuggestion::Expense
            ),
            "kind={:?}",
            s.kind
        );
        assert_ne!(s.kind, EntryKindSuggestion::Income);
        assert_eq!(s.entry_date.as_deref(), Some("2026-08-18"));
        assert!(
            s.merchant
                .as_deref()
                .is_some_and(|m| m.to_lowercase().contains("zeni")),
            "merchant={:?}",
            s.merchant
        );
        assert!(
            s.reference.as_deref().is_some_and(|r| r.starts_with("RF")),
            "reference={:?}",
            s.reference
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "fixture tests fail loudly by design")]
    fn greek_bank_transfer_receipt_principal_not_fee_or_clock() {
        let text = corpus_text("synthetic/text/greek_bank_embasma.txt");
        let suggestion = parse_invoice_text(&text);
        assert_eq!(
            suggestion.amount_minor,
            Some(31_000),
            "expected €310,00 not fee 1,40 or clock 7:00, got {:?}",
            suggestion.amount_minor
        );
        assert_eq!(suggestion.kind, EntryKindSuggestion::Expense);
        assert!(!suggestion.bill_unpaid);
        assert_eq!(suggestion.entry_date.as_deref(), Some("2026-08-27"));
        assert_eq!(suggestion.merchant.as_deref(), Some("HELIOS TRADING IKE"));
        assert_eq!(suggestion.reference.as_deref(), Some("F000TO0000000001"));
        assert_eq!(
            suggestion.description.as_deref(),
            Some("Έμβασμα — HELIOS TRADING IKE")
        );
        assert!(
            suggestion.notes.contains("1,40"),
            "notes should mention the fee only: {}",
            suggestion.notes
        );
        assert_ne!(suggestion.amount_minor, Some(140));
        assert_ne!(suggestion.amount_minor, Some(700));

        let via_analyze = crate::documents::analyze_document_bytes(
            "greek_bank_embasma.txt",
            "text/plain",
            text.as_bytes(),
            &[],
            "EUR",
            None,
        );
        let analyzed = via_analyze.expect("analyze text/plain");
        assert_eq!(analyzed.amount_minor, suggestion.amount_minor);
        assert_eq!(analyzed.entry_date, suggestion.entry_date);
        assert_eq!(analyzed.kind, suggestion.kind);
        assert_eq!(analyzed.merchant, suggestion.merchant);
        assert_eq!(analyzed.reference, suggestion.reference);
        assert_eq!(analyzed.description, suggestion.description);
    }

    #[test]
    fn unaccented_bank_transfer_markers_are_detected() {
        let text = "\
Εμβασμα
Μεταφορα σε αλλη τραπεζα
Ημερομηνια Εκτελεσης: 27/8/2026
Ημερομηνια Αξιας: 28/8/2026 7:00 μ.μ.
Ποσο Χρεωσης Κεφαλαιου 310,00
Προμηθεια 1,40
Ονοματεπωνυμο / Επωνυμια Δικαιουχου: HELIOS TRADING IKE
Κωδικος Συναλλαγης: F000TO0000000001
";
        let suggestion = parse_invoice_text(text);
        assert_eq!(suggestion.amount_minor, Some(31_000));
        assert_eq!(suggestion.entry_date.as_deref(), Some("2026-08-27"));
        assert_eq!(suggestion.merchant.as_deref(), Some("HELIOS TRADING IKE"));
        assert_eq!(suggestion.reference.as_deref(), Some("F000TO0000000001"));
        assert_eq!(suggestion.kind, EntryKindSuggestion::Expense);
    }

    #[test]
    fn ngs_gas_bill_payment_total() {
        let s = parse_invoice_text(&corpus_text("synthetic/text/ngs_gas_bill.txt"));
        assert_eq!(
            s.amount_minor,
            Some(7_253),
            "expected €72.53 (not deposit 60 or subtotal 50.50), got {:?}",
            s.amount_minor
        );
        assert_ne!(s.kind, EntryKindSuggestion::Income);
        assert!(
            matches!(
                s.kind,
                EntryKindSuggestion::Bill | EntryKindSuggestion::Expense
            ),
            "kind={:?}",
            s.kind
        );
        assert_eq!(s.entry_date.as_deref(), Some("2026-08-13"));
        assert!(
            s.merchant.as_deref().is_some_and(
                |m| m.to_lowercase().contains("gas") || m.to_lowercase().contains("ngs")
            ),
            "merchant={:?}",
            s.merchant
        );
        assert!(
            s.reference
                .as_deref()
                .is_some_and(|r| { r.contains("NGS") || r.contains("SYN") || r.starts_with("RF") }),
            "reference={:?}",
            s.reference
        );
    }
}

#[cfg(test)]
mod jumbled_extract {
    use super::*;

    /// Synthetic jumbled layout (the shape `pdf_extract` produces on a
    /// text-layer utility PDF). Placeholders only — not a live dump.
    #[test]
    #[expect(clippy::expect_used, reason = "fixture tests fail loudly by design")]
    fn parse_jumbled_ngs_extract_fixture() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/ngs_gas_jumbled_extract.txt"
        ))
        .expect("fixture extract");
        let s = parse_invoice_text(&text);
        assert_eq!(
            s.amount_minor,
            Some(7_253),
            "expected €72.53 not date/deposit noise, got {:?}",
            s.amount_minor
        );
        assert_ne!(s.kind, EntryKindSuggestion::Income);
        assert_eq!(s.entry_date.as_deref(), Some("2026-08-13"));
        assert!(
            s.reference
                .as_deref()
                .is_some_and(|r| { r.contains("NGS") || r.contains("SYN") || r.starts_with("RF") }),
            "reference={:?}",
            s.reference
        );
        assert_eq!(
            s.merchant.as_deref(),
            Some("Volton"),
            "MyON portal branding identifies the supplier"
        );
        assert_eq!(s.description.as_deref(), Some("Volton — Gas bill"));
    }

    #[test]
    fn date_tokens_are_not_money() {
        assert!(money_amounts_on_line("13/08/2026 72,53 €").contains(&7_253));
        assert!(!money_amounts_on_line("13/08/2026 72,53 €").contains(&1_300));
        assert!(!money_amounts_on_line("26/05/2026 30/06/2026").contains(&2_600));
        assert!(!money_amounts_on_line("27/8/2026 310,00").contains(&2_700));
        assert!(!money_amounts_on_line("27/8/2026 310,00").contains(&800));
        assert!(money_amounts_on_line("27/8/2026 310,00").contains(&31_000));
        assert_eq!(parse_money_token("08"), None);
        assert_eq!(parse_money_token("2026"), None);
    }

    #[test]
    fn clock_tokens_are_not_money() {
        assert!(!money_amounts_on_line("Ημερομηνία Αξίας 28/8/2026 7:00 μ.μ.").contains(&700));
        assert!(money_amounts_on_line("7:00").is_empty());
        assert!(money_amounts_on_line("19:30").is_empty());
    }

    #[test]
    #[expect(clippy::expect_used, reason = "fixture tests fail loudly by design")]
    fn parse_optional_local_gas_pdf_bytes() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/local_gas_bill.pdf"
        );
        let Ok(bytes) = std::fs::read(path) else {
            // Optional local PDF — never required in the public tree.
            return;
        };
        let text = pdf_extract::extract_text_from_mem(&bytes).expect("pdf text");
        let s = parse_invoice_text(&text);
        assert_eq!(
            s.amount_minor,
            Some(7_253),
            "local pdf_extract amount={:?}\ntext excerpt:\n{}",
            s.amount_minor,
            text.chars().take(800).collect::<String>()
        );
    }
}
