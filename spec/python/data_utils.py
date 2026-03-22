"""Data utility module for testing."""

import json
import re
from collections import Counter


def word_frequency(text, top_n=10):
    words = re.findall(r'\b\w+\b', text.lower())
    return dict(Counter(words).most_common(top_n))


def clean_text(text, lowercase=True, remove_punctuation=False):
    result = text
    if lowercase:
        result = result.lower()
    if remove_punctuation:
        result = re.sub(r'[^\w\s]', '', result)
    return re.sub(r'\s+', ' ', result).strip()


def parse_json(json_string):
    return json.loads(json_string)


def to_json(data, indent=2):
    return json.dumps(data, indent=indent)
