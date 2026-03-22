"""Normalization utilities — demonstrates 3-level deep module import."""


def min_max(values):
    """Normalize values to [0, 1] range."""
    if not values:
        return []
    lo, hi = min(values), max(values)
    if lo == hi:
        return [0.5] * len(values)
    return [(v - lo) / (hi - lo) for v in values]


def z_score(values):
    """Normalize values using z-score."""
    if not values:
        return []
    mean = sum(values) / len(values)
    std = (sum((v - mean) ** 2 for v in values) / len(values)) ** 0.5
    if std == 0:
        return [0.0] * len(values)
    return [(v - mean) / std for v in values]
