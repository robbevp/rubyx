"""Predictor module for testing nested imports."""


def predict(values):
    if not values:
        return 0.0
    return sum(values) / len(values)


def classify(value, threshold=0.5):
    return "positive" if value >= threshold else "negative"
