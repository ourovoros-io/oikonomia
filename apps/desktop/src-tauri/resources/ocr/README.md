# Bundled OCR models (offline)

These neural network weights power on-device receipt/bill text recognition.

| File | Purpose | Approx size |
|------|---------|-------------|
| `text-detection.rten` | Find text regions | ~2.4 MB |
| `text-recognition.rten` | Read characters | ~9.3 MB |

Source: [ocrs](https://github.com/robertknight/ocrs) models (MIT/Apache).

They are **shipped with the app** and loaded locally. No downloads at runtime, no cloud APIs.

Re-download if missing:

```bash
curl -L -o text-detection.rten https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten
curl -L -o text-recognition.rten https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten
```
