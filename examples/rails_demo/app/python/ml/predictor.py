"""ML predictor — demonstrates class-based Python objects in Rails.

No external deps needed. For real ML, add 'scikit-learn' or 'torch'
to pyproject.toml dependencies.
"""

import math


class SimplePredictor:
    """A toy predictor that demonstrates persistent Python objects in Ruby.

    Usage from Ruby:
        ml = Rubyx.import('ml.predictor')
        model = ml.SimplePredictor.new([1,2,3,4,5], [2,4,6,8,10])
        prediction = model.predict(6)  # => ~12.0
        puts model.summary.to_ruby
    """

    def __init__(self, x_data, y_data):
        if len(x_data) != len(y_data):
            raise ValueError("x_data and y_data must have same length")
        self.x_data = list(x_data)
        self.y_data = list(y_data)
        self._fit()

    def _fit(self):
        """Simple linear regression: y = slope * x + intercept."""
        n = len(self.x_data)
        sum_x = sum(self.x_data)
        sum_y = sum(self.y_data)
        sum_xy = sum(x * y for x, y in zip(self.x_data, self.y_data))
        sum_x2 = sum(x * x for x in self.x_data)

        denom = n * sum_x2 - sum_x ** 2
        if denom == 0:
            self.slope = 0
            self.intercept = sum_y / n
        else:
            self.slope = (n * sum_xy - sum_x * sum_y) / denom
            self.intercept = (sum_y - self.slope * sum_x) / n

    def predict(self, x):
        """Predict y for a given x."""
        return self.slope * x + self.intercept

    def predict_batch(self, x_values):
        """Predict for multiple x values. Returns list."""
        return [self.predict(x) for x in x_values]

    def r_squared(self):
        """Calculate R-squared (coefficient of determination)."""
        y_mean = sum(self.y_data) / len(self.y_data)
        ss_tot = sum((y - y_mean) ** 2 for y in self.y_data)
        ss_res = sum((y - self.predict(x)) ** 2 for x, y in zip(self.x_data, self.y_data))
        if ss_tot == 0:
            return 1.0
        return 1 - (ss_res / ss_tot)

    def summary(self):
        return {
            "slope": round(self.slope, 4),
            "intercept": round(self.intercept, 4),
            "r_squared": round(self.r_squared(), 4),
            "n_samples": len(self.x_data),
        }


class KMeansSimple:
    """Toy K-Means clustering (no deps).

    Usage from Ruby:
        ml = Rubyx.import('ml.predictor')
        km = ml.KMeansSimple.new(3)
        data = [[1,1],[1,2],[10,10],[10,11],[20,20],[20,21]]
        km.fit(data)
        puts km.predict([5, 5]).to_ruby    # => cluster index
        puts km.centroids.to_ruby          # => [[1.0,1.5], [10.0,10.5], [20.0,20.5]]
    """

    def __init__(self, k, max_iter=100):
        self.k = k
        self.max_iter = max_iter
        self.centroids = None

    def fit(self, data):
        points = [list(p) for p in data]
        dims = len(points[0])

        # Initialize centroids from first k points
        self.centroids = [list(points[i % len(points)]) for i in range(self.k)]

        for _ in range(self.max_iter):
            clusters = [[] for _ in range(self.k)]
            for p in points:
                closest = min(range(self.k), key=lambda i: self._dist(p, self.centroids[i]))
                clusters[closest].append(p)

            new_centroids = []
            for i, cluster in enumerate(clusters):
                if cluster:
                    new_centroids.append([sum(p[d] for p in cluster) / len(cluster) for d in range(dims)])
                else:
                    new_centroids.append(self.centroids[i])

            if new_centroids == self.centroids:
                break
            self.centroids = new_centroids

        return self

    def predict(self, point):
        return min(range(self.k), key=lambda i: self._dist(point, self.centroids[i]))

    def _dist(self, a, b):
        return math.sqrt(sum((ai - bi) ** 2 for ai, bi in zip(a, b)))
