"""Document OCR using GLM-OCR (zai-org/GLM-OCR) via Ollama.

Setup:
    1. Install: uv sync --extra ai
    2. Pull the model: ollama pull glm-ocr
    3. Ollama serves on localhost:11434 by default
    4. Use from Ruby:
       ocr = Rubyx.import('services.ocr')
       ocr.load()
       result = ocr.parse_document("app/python/docs/invoice.pdf")

Docs: https://github.com/zai-org/GLM-OCR
"""

_parser = None


def load(config_path=None, device="cpu"):
    """Connect to Ollama serving GLM-OCR.

    Prerequisites:
        ollama pull glm-ocr

    Args:
        config_path: Path to glmocr.yaml (default: config/glmocr.yaml)
        device: Layout model device — "cpu", "cuda", "cuda:0" etc.
    """
    global _parser
    import os
    from glmocr import GlmOcr

    if config_path is None:
        # Look for config relative to the Rails root
        for candidate in [
            os.path.join(os.getcwd(), "config", "glmocr.yaml"),
            os.path.join(os.path.dirname(__file__), "..", "..", "..", "config", "glmocr.yaml"),
        ]:
            if os.path.exists(candidate):
                config_path = candidate
                break

    _parser = GlmOcr(
        config_path=config_path,
        mode="selfhosted",
        model="glm-ocr",
        layout_device=device,
    )
    return "GLM-OCR connected to Ollama"


def parse_document(file_path):
    """Parse a PDF or image file.

    Returns dict with 'markdown' and 'json_result' keys.
    """
    _ensure_loaded()
    result = _parser.parse(file_path)
    return {
        "markdown": result.markdown if hasattr(result, 'markdown') else str(result),
        "json_result": result.json_result if hasattr(result, 'json_result') else [],
    }


def parse_to_markdown(file_path):
    """Parse and return just the markdown text."""
    _ensure_loaded()
    result = _parser.parse(file_path)
    return result.markdown if hasattr(result, 'markdown') else str(result)


def parse_to_json(file_path):
    """Parse and return structured JSON with bounding boxes."""
    _ensure_loaded()
    result = _parser.parse(file_path)
    return result.json_result if hasattr(result, 'json_result') else []


def stream_pages(pdf_path):
    """Stream OCR results page by page from a multi-page PDF.

    Converts each page to an image, then runs OCR one page at a time.
    Yields dict with 'page' and 'text' keys.

    Usage from Ruby:
        Rubyx.stream(ocr.stream_pages("report.pdf")).each do |page|
          puts "Page #{page['page']}: #{page['text']}"
        end
    """
    _ensure_loaded()
    images = _pdf_to_images(pdf_path)

    for i, img_path in enumerate(images):
        result = _parser.parse(img_path)
        text = result.markdown if hasattr(result, 'markdown') else str(result)
        yield {"page": i + 1, "text": text}

    # Cleanup temp images
    import os
    for img_path in images:
        if os.path.exists(img_path):
            os.remove(img_path)


def parse_image(image_path):
    """Parse a single image file (PNG, JPG, etc.)."""
    _ensure_loaded()
    result = _parser.parse(image_path)
    return {
        "markdown": result.markdown if hasattr(result, 'markdown') else str(result),
        "json_result": result.json_result if hasattr(result, 'json_result') else [],
    }


def parse_multiple(file_paths):
    """Parse multiple images as pages of a single document."""
    _ensure_loaded()
    result = _parser.parse(list(file_paths))
    return {
        "markdown": result.markdown if hasattr(result, 'markdown') else str(result),
        "json_result": result.json_result if hasattr(result, 'json_result') else [],
    }


def pdf_info(pdf_path):
    """Get PDF metadata and page count (no OCR model needed)."""
    import fitz  # PyMuPDF (included with glmocr[selfhosted])

    doc = fitz.open(pdf_path)
    info = {
        "page_count": len(doc),
        "metadata": dict(doc.metadata) if doc.metadata else {},
    }
    doc.close()
    return info


# === Internal helpers ===

def _ensure_loaded():
    global _parser
    if _parser is None:
        load()


def _pdf_to_images(pdf_path):
    """Convert PDF pages to temp PNG images for per-page OCR."""
    import fitz  # PyMuPDF
    import tempfile
    import os

    doc = fitz.open(pdf_path)
    image_paths = []
    tmp_dir = tempfile.mkdtemp(prefix="glmocr_")

    for i, page in enumerate(doc):
        pix = page.get_pixmap(dpi=200)
        img_path = os.path.join(tmp_dir, f"page_{i}.png")
        pix.save(img_path)
        image_paths.append(img_path)

    doc.close()
    return image_paths
