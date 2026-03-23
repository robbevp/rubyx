"""Text processing utilities — sync functions and classes."""


class TextAnalyzer:
    """Simple class demonstrating Rubyx object wrapping."""

    def __init__(self, text):
        self.text = text
        self._words = text.split()

    @property
    def word_count(self):
        return len(self._words)

    @property
    def char_count(self):
        return len(self.text)

    def most_common(self, n=5):
        from collections import Counter
        return Counter(self._words).most_common(n)

    def summary(self):
        return {
            "word_count": self.word_count,
            "char_count": self.char_count,
            "unique_words": len(set(self._words)),
            "avg_word_length": round(sum(len(w) for w in self._words) / max(len(self._words), 1), 2),
        }


def slugify(text):
    """Convert text to URL-friendly slug."""
    import re
    text = text.lower().strip()
    text = re.sub(r'[^\w\s-]', '', text)
    text = re.sub(r'[\s_-]+', '-', text)
    return text.strip('-')


def extract_emails(text):
    """Extract email addresses from text."""
    import re
    return re.findall(r'[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}', text)


def chunk_text(text, chunk_size=500, overlap=50):
    """Split text into overlapping chunks (useful for LLM context windows)."""
    words = text.split()
    chunks = []
    for i in range(0, len(words), chunk_size - overlap):
        chunk = ' '.join(words[i:i + chunk_size])
        if chunk:
            chunks.append(chunk)
    return chunks
