"""Predictor module — demonstrates nested submodule import."""


def predict(values):
    """Simple prediction: returns the average of input values."""
    if not values:
        return 0.0
    return sum(values) / len(values)


def classify(value, threshold=0.5):
    """Binary classification based on threshold."""
    return "positive" if value >= threshold else "negative"


class Model:
    def __init__(self, name="default"):
        self.name = name
        self._trained = False

    def train(self):
        self._trained = True
        return self

    def is_trained(self):
        return self._trained

    def get_name(self):
        return self.name
