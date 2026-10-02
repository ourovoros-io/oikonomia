# Bundled OCR models (offline)

These neural network weights power on-device receipt/bill text recognition.

| File | Purpose | Approx size |
|------|---------|-------------|
| `text-detection.rten` | Find text regions | ~2.4 MB |
| `text-recognition.rten` | Read characters | ~9.3 MB |

## Provenance and licence

These are the pre-trained [ocrs](https://github.com/robertknight/ocrs) models by
Robert Knight, downloaded from the URLs in the commands below. The files in
this directory are byte-identical to those downloads as of 2026-10-02 and are
distributed unmodified.

| File | SHA-256 |
|------|---------|
| `text-detection.rten` | `f15cfb56bd02c4bf478a20343986504a1f01e1665c2b3a0ad66340f054b1b5ca` |
| `text-recognition.rten` | `e484866d4cce403175bd8d00b128feb08ab42e208de30e42cd9889d8f1735a6e` |

- Licence of the model weights: [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/),
  as declared on the author's model card, <https://huggingface.co/robertknight/ocrs>.
- The training project, <https://github.com/robertknight/ocrs-models>, states
  that the models are trained on HierText, which is CC BY-SA 4.0.
- The ocrs library code, <https://github.com/robertknight/ocrs>, is dual-licensed
  MIT / Apache-2.0. That is the licence of the code, not of these weights.

Verify the files (compare with the table above):

```bash
shasum -a 256 text-detection.rten text-recognition.rten
```

They are **shipped with the app** and loaded locally. No downloads at runtime, no cloud APIs.

Re-download if missing:

```bash
curl -L -o text-detection.rten https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten
curl -L -o text-recognition.rten https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten
```
