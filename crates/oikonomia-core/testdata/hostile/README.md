# Hostile PDF fixtures

Small hand-written PDFs whose structure has no end for pdf-extract to
reach. The app must refuse them before extraction; the tests in
`src/documents/pdf_nesting.rs` and `src/documents/analyze.rs` check that it
does. Nothing here is a real document.

| File | What it covers |
|------|----------------|
| `xobject_self_loop.pdf` | A form `XObject` that refers back to itself. |
| `page_parent_loop.pdf` | A page tree whose `Parent` links form a loop. |

The files are plain text. If you edit one, the `xref` offsets must be
rewritten to match.
