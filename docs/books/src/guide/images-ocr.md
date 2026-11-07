# Images and OCR

## Embedded images

`--extract-images DIR` writes images from PDF, Word, PowerPoint, OpenDocument, Jupyter notebooks, EPUB and MOBI into `DIR` and links them from the Markdown. With several inputs each file gets its own sub-directory.

## OCR

The `ocr` feature needs Tesseract:

```sh
brew install tesseract              # macOS
sudo apt install tesseract-ocr      # Ubuntu/Debian
sudo pacman -S tesseract            # Arch Linux
```

```sh
mq-conv photo.png --format ocr
mq-conv photo.png --format ocr --ocr-lang jpn
mq-conv mixed.png --format ocr --ocr-lang eng+jpn   # combine with "+"
```

OCR defaults to English. Install the matching language pack for others (`brew install tesseract-lang`, or `sudo apt install tesseract-ocr-jpn`). With the feature enabled, image-only PDF pages also fall back to Tesseract.
