"""Normalization utilities for testing 3-level deep imports."""


def min_max(values):
    if not values:
        return []
    lo, hi = min(values), max(values)
    if lo == hi:
        return [0.5] * len(values)
    return [(v - lo) / (hi - lo) for v in values]
