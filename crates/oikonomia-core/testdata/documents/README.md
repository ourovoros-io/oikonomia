# Offline document parse corpus

Golden fixtures for the **offline** invoice / bill reader in `oikonomia-core`.
CI runs the harness (`tests/document_corpus.rs`) against this tree. A parse
regression fails the build.

Nothing here is fetched at test or runtime. There is **no network** in the
corpus, the harness, or the parser.

## Rights

| `rights` value | Meaning |
|----------------|---------|
| `synthetic` | Invented document text. Not copied from a real customer bill. Safe for the public tree. |
| `private` | Local-only real or redacted scan. **Must not** be committed. Listed in `MANIFEST.toml` only so a developer machine can run extra cases. |

Public fixtures are all `rights = "synthetic"`. Do not add a real bill, email,
or scan to any path other than `private/`.

## Redaction rules (public tree)

The public corpus must contain **zero real PII**:

- No real names, street addresses, phone numbers, emails, or account numbers
  belonging to a person or company you did not invent for the fixture.
- No real ΑΦΜ / VAT, IBAN, RF payment codes, or meter / supply IDs taken from
  a live document. Synthetic IDs (obvious placeholders) are fine.
- No photographs of real mail, PDFs exported from a real portal, or OCR dumps
  of those files.
- Brand names that already appear in `brands.rs` (ΔΕΗ, Cosmote, Volton, ΕΥΔΑΠ,
  …) may be used as the **biller**, the way a form letter names a utility.

If a fixture would need a real identifier to be useful, put it under `private/`
and keep it gitignored.

Public fixtures use **scrubbed placeholders** only (invented names, invalid
ΑΦΜ such as `000000000`, synthetic supply / ΗΚΑΣΠ / RF digit runs). Do not
“improve” them with details from a live bill.

## Layout

```
documents/
  README.md                 this file
  MANIFEST.toml             id → path, sector, brand, split, rights
  golden/                   expected DocumentSuggestion fields (JSON)
  synthetic/text/           invented plain-text bills
  synthetic/pdf/            invented text-layer PDF
  synthetic/image/          invented JPEG for the OCR smoke path
  holdout/                  extra synthetics (same harness, split = holdout)
  private/                  gitignored; add .txt + golden locally
```

## How to add a fixture

1. Write the document text (or generate a PDF) under `synthetic/`, `holdout/`,
   or `private/`.
2. Add a `[[documents]]` table to `MANIFEST.toml` (`id`, `path`, `golden`,
   `sector`, `brand`, `split`, `rights`).
3. Emit a starter golden (review every field before committing):

   ```bash
   DUMP_DOCUMENT_GOLDENS=1 cargo test -p oikonomia-core --test document_corpus -- write_missing_goldens
   ```

4. Trim the JSON to the fields you want to lock. The harness asserts a field
   only when the golden has a value:
   - `amount_minor`, `entry_date`, `kind`, `bill_unpaid` — exact
   - `merchant` — exact, or any string in `merchant_aliases` (contains)
   - `description` — optional; if present, the suggestion must contain it
5. Run `cargo test -p oikonomia-core --test document_corpus`.

ΑΦΜ / IBAN / MARK digit runs must **not** become `amount_minor` (existing
parser rule). Include those tokens in a fixture when you want that lock.

## OCR JPEG smoke

`synthetic/image/english_total.jpg` exercises `analyze_document_bytes` on
`image/jpeg`. Bundled `ocrs` models (`text-detection.rten`,
`text-recognition.rten`) are **not** in this crate and are not downloaded by
CI. If `analyzer_status` reports models missing, the smoke test skips.

## Private local run

`private/**` is gitignored except `.gitkeep`. You may list private rows in
`MANIFEST.toml`; the harness skips a row when its file is absent.
