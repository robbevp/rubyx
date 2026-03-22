"""Calculator module — demonstrates functions and classes."""

VERSION = "1.0.0"


def add(a, b):
    return a + b


def multiply(a, b):
    return a * b


def divide(a, b):
    if b == 0:
        raise ZeroDivisionError("division by zero")
    return a / b


def fibonacci(n):
    if n <= 0:
        return []
    if n == 1:
        return [0]
    fibs = [0, 1]
    for _ in range(2, n):
        fibs.append(fibs[-1] + fibs[-2])
    return fibs


class Calculator:
    def __init__(self, initial=0):
        self._value = initial
        self._history = []

    def add(self, n):
        self._value += n
        self._history.append(f"add({n})")
        return self

    def multiply(self, n):
        self._value *= n
        self._history.append(f"multiply({n})")
        return self

    def get_value(self):
        return self._value

    def get_history(self):
        return self._history
