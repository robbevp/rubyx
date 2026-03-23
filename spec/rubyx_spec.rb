require_relative 'spec_helper'

RSpec.describe 'Rubyx', ruby_integration: true do
  # ========== Module structure ==========

  describe 'module structure' do
    it 'defines the Rubyx module' do
      expect(defined?(Rubyx)).to eq('constant')
    end

    it 'responds to .eval' do
      expect(Rubyx).to respond_to(:eval)
    end

    it 'responds to .import' do
      expect(Rubyx).to respond_to(:import)
    end

    it 'responds to .stream' do
      expect(Rubyx).to respond_to(:stream)
    end

    it 'responds to .async_stream' do
      expect(Rubyx).to respond_to(:async_stream)
    end

    it 'responds to .initialized?' do
      expect(Rubyx).to respond_to(:initialized?)
    end
  end

  # ========== Rubyx.initialized? ==========

  describe '.initialized?' do
    it 'returns true after initialization' do
      expect(Rubyx.initialized?).to eq(true)
    end

    it 'returns a boolean' do
      expect(Rubyx.initialized?).to be(true).or be(false)
    end
  end

  # ========== Error class mapping ==========

  describe 'error classes' do
    it 'defines error hierarchy' do
      expect(Rubyx::Error.superclass).to eq(StandardError)
      expect(Rubyx::PythonError.superclass).to eq(Rubyx::Error)
      expect(Rubyx::ImportError.superclass).to eq(Rubyx::PythonError)
      expect(Rubyx::KeyError.superclass).to eq(Rubyx::Error)
      expect(Rubyx::IndexError.superclass).to eq(Rubyx::Error)
      expect(Rubyx::ValueError.superclass).to eq(Rubyx::Error)
      expect(Rubyx::AttributeError.superclass).to eq(Rubyx::Error)
      expect(Rubyx::TypeError.superclass).to eq(Rubyx::Error)
    end

    it 'raises Rubyx::KeyError for dict key miss' do
      expect { Rubyx.eval('{}["missing"]') }.to raise_error(Rubyx::KeyError)
    end

    it 'raises Rubyx::IndexError for list index out of range' do
      expect { Rubyx.eval('[][5]') }.to raise_error(Rubyx::IndexError)
    end

    it 'raises Rubyx::ValueError for invalid conversion' do
      expect { Rubyx.eval('int("not_a_number")') }.to raise_error(Rubyx::ValueError)
    end

    it 'raises Rubyx::TypeError for type mismatch' do
      expect { Rubyx.eval('1 + "a"') }.to raise_error(Rubyx::TypeError)
    end

    it 'raises Rubyx::AttributeError for missing attribute' do
      expect { Rubyx.eval('(1).nonexistent') }.to raise_error(Rubyx::AttributeError)
    end

    it 'raises Rubyx::ImportError for missing module' do
      expect { Rubyx.import('nonexistent_module_xyz') }.to raise_error(Rubyx::ImportError)
    end

    it 'raises Rubyx::PythonError for unmapped Python errors' do
      expect { Rubyx.eval('1/0') }.to raise_error(Rubyx::PythonError)
    end

    it 'all mapped errors are subclasses of Rubyx::Error' do
      expect { Rubyx.eval('{}["x"]') }.to raise_error(Rubyx::Error)
      expect { Rubyx.eval('[][0]') }.to raise_error(Rubyx::Error)
      expect { Rubyx.eval('int("x")') }.to raise_error(Rubyx::Error)
      expect { Rubyx.eval('1 + "x"') }.to raise_error(Rubyx::Error)
    end

    it 'includes Python error message in exception' do
      begin
        Rubyx.eval('{}["missing"]')
      rescue Rubyx::KeyError => e
        expect(e.message).to include('missing')
      end
    end
  end

  # ========== Rubyx.eval ==========

  describe '.eval' do
    it 'evaluates a simple Python expression' do
      result = Rubyx.eval('1 + 2')
      expect(result).not_to be_nil
    end

    it 'evaluates a string expression' do
      result = Rubyx.eval('"hello"')
      expect(result).not_to be_nil
    end

    it 'evaluates a list expression' do
      result = Rubyx.eval('[1, 2, 3]')
      expect(result).not_to be_nil
    end

    it 'evaluates None' do
      result = Rubyx.eval('None')
      expect(result).not_to be_nil
    end

    it 'evaluates a boolean expression' do
      result = Rubyx.eval('True')
      expect(result).not_to be_nil
    end

    it 'evaluates multiline statements and returns last expression value' do
      # Body "x = 10" runs as statement, "iter([x * 2])" evals as expression
      result = Rubyx.eval("x = 10\niter([x * 2])")
      expect(Rubyx.stream(result).to_a).to eq([20])
    end

    it 'raises on syntax error' do
      expect { Rubyx.eval('def class for') }.to raise_error(Exception)
    end

    it 'raises on NameError' do
      expect { Rubyx.eval('undefined_variable_xyz') }.to raise_error(StandardError)
    end

    it 'raises on division by zero' do
      expect { Rubyx.eval('1 / 0') }.to raise_error(StandardError)
    end

    it 'does not leak state across failed and successful calls' do
      expect { Rubyx.eval('bad_var') }.to raise_error(StandardError)
      result = Rubyx.eval('42')
      expect(result).not_to be_nil
    end

    # ---- AST-based splitting (multiline code) ----
    # Each test wraps the final value inside iter([...]) in Python so we
    # can stream it back and verify the actual value.

    it 'returns value from function def + call' do
      result = Rubyx.eval(<<~PY)
        def square(x):
            return x * x
        iter([square(7)])
      PY
      expect(Rubyx.stream(result).to_a).to eq([49])
    end

    it 'handles if block followed by expression' do
      result = Rubyx.eval(<<~PY)
        x = 0
        if True:
            x = 42
        iter([x])
      PY
      expect(Rubyx.stream(result).to_a).to eq([42])
    end

    it 'handles multiple assignments followed by expression' do
      result = Rubyx.eval(<<~PY)
        a = 3
        b = 4
        c = 5
        iter([a * b * c])
      PY
      expect(Rubyx.stream(result).to_a).to eq([60])
    end

    it 'handles import followed by expression' do
      result = Rubyx.eval(<<~PY)
        import math
        iter([math.factorial(5)])
      PY
      expect(Rubyx.stream(result).to_a).to eq([120])
    end

    it 'handles for loop followed by expression' do
      result = Rubyx.eval(<<~PY)
        total = 0
        for i in range(5):
            total += i
        iter([total])
      PY
      expect(Rubyx.stream(result).to_a).to eq([10])
    end

    it 'handles try/except followed by expression' do
      result = Rubyx.eval(<<~PY)
        x = 0
        try:
            x = 1 // 0
        except ZeroDivisionError:
            x = -1
        iter([x])
      PY
      expect(Rubyx.stream(result).to_a).to eq([-1])
    end

    it 'handles class def followed by instantiation' do
      result = Rubyx.eval(<<~PY)
        class Pair:
            def __init__(self, a, b):
                self.sum = a + b
        iter([Pair(3, 7).sum])
      PY
      expect(Rubyx.stream(result).to_a).to eq([10])
    end

    it 'handles all-statement code (no trailing expression)' do
      result = Rubyx.eval(<<~PY)
        x = 10
        y = 20
      PY
      expect(result).not_to be_nil
    end

    it 'handles nested function def + call' do
      result = Rubyx.eval(<<~PY)
        def outer():
            def inner(n):
                return n * 2
            return inner(21)
        iter([outer()])
      PY
      expect(Rubyx.stream(result).to_a).to eq([42])
    end

    it 'handles decorator + function def + call' do
      result = Rubyx.eval(<<~PY)
        def my_decorator(fn):
            def wrapper(*args):
                return fn(*args) + 100
            return wrapper
        @my_decorator
        def add(a, b):
            return a + b
        iter([add(1, 2)])
      PY
      expect(Rubyx.stream(result).to_a).to eq([103])
    end

    it 'handles multi-line list expression as last statement' do
      result = Rubyx.eval(<<~PY)
        x = 10
        [
            x,
            x + 1,
            x + 2,
        ]
      PY
      expect(Rubyx.stream(result).to_a).to eq([10, 11, 12])
    end

    it 'does not leak state between separate eval calls' do
      Rubyx.eval("secret = 999")
      expect { Rubyx.eval("secret") }.to raise_error(StandardError)
    end
  end

  # ========== Rubyx.import ==========

  describe '.import' do
    # Use pure-Python modules to avoid RTLD_LOCAL issues with C extensions
    it 'imports the os module' do
      result = Rubyx.import('os')
      expect(result).not_to be_nil
    end

    it 'imports the json module' do
      result = Rubyx.import('json')
      expect(result).not_to be_nil
    end

    it 'imports the sys module' do
      result = Rubyx.import('sys')
      expect(result).not_to be_nil
    end

    it 'raises on nonexistent module' do
      expect { Rubyx.import('definitely_not_a_real_module_xyz') }.to raise_error(StandardError)
    end

    it 'can import the same module twice without error' do
      m1 = Rubyx.import('json')
      m2 = Rubyx.import('json')
      expect(m1).not_to be_nil
      expect(m2).not_to be_nil
    end
  end
end

RSpec.describe 'Rubyx::Stream', ruby_integration: true do
  # ========== Class structure ==========

  describe 'class structure' do
    it 'defines the Rubyx::Stream class' do
      expect(defined?(Rubyx::Stream)).to eq('constant')
    end

    it 'defines Rubyx::Stream as the canonical stream class (tests-first contract)' do
      expect(defined?(Rubyx::Stream)).to eq('constant')
    end

    it 'includes Enumerable' do
      expect(Rubyx::Stream.ancestors).to include(Enumerable)
    end

    it 'has an each method' do
      expect(Rubyx::Stream.method_defined?(:each)).to be true
    end

    it 'has a next method' do
      expect(Rubyx::Stream.method_defined?(:next)).to be true
    end

    it 'has Enumerable methods from each' do
      %i[map select reject reduce first take to_a].each do |method|
        expect(Rubyx::Stream.method_defined?(method)).to be(true),
                                                         "expected Rubyx::Stream to have #{method}"
      end
    end
  end

  # ========== Rubyx.stream factory ==========

  describe 'Rubyx.stream' do
    it 'creates a Rubyx::Stream from a Python iterator' do
      gen = Rubyx.eval('iter(range(5))')
      stream = Rubyx.stream(gen)
      expect(stream).to be_a(Rubyx::Stream)
    end

    it 'supports block-only stream creation (tests-first contract)' do
      stream = Rubyx.stream { Rubyx.eval('iter(range(5))') }
      expect(stream.to_a).to eq([0, 1, 2, 3, 4])
    end

    it 'raises ArgumentError when called with both iterable and block (tests-first contract)' do
      iterable = Rubyx.eval('iter(range(3))')
      expect do
        Rubyx.stream(iterable) { Rubyx.eval('iter(range(1))') }
      end.to raise_error(ArgumentError)
    end

    it 'raises ArgumentError when called with no args and no block (tests-first contract)' do
      expect { Rubyx.stream }.to raise_error(ArgumentError)
    end

    it 'raises ArgumentError when called with too many args (tests-first contract)' do
      a = Rubyx.eval('iter(range(1))')
      b = Rubyx.eval('iter(range(1))')
      expect { Rubyx.stream(a, b) }.to raise_error(ArgumentError)
    end

    it 'does not hang for block-only stream creation and consumption (tests-first contract)' do
      result = Timeout.timeout(3) do
        Rubyx.stream { Rubyx.eval('iter(range(1000000))') }.take(5)
      end
      expect(result).to eq([0, 1, 2, 3, 4])
    end

    it 'raises TypeError for non-iterable Python objects' do
      py_int = Rubyx.eval('42')
      expect { Rubyx.stream(py_int) }.to raise_error(RuntimeError, /not iterable/)
    end

    it 'raises TypeError for non-RubyxObject arguments' do
      expect { Rubyx.stream('not a python object') }.to raise_error(StandardError)
    end
  end

  # ========== Streaming with each ==========

  describe '#each' do
    it 'returns Enumerator when no block is given (tests-first contract)' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(4))'))
      enumerator = stream.each
      expect(enumerator).to be_a(Enumerator)
      expect(enumerator.to_a).to eq([0, 1, 2, 3])
    end

    it 'does not hang on no-block each enumerator path (tests-first contract)' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(1000000))'))
      result = Timeout.timeout(3) { stream.each.take(5) }
      expect(result).to eq([0, 1, 2, 3, 4])
    end

    it 'iterates over all values from a Python range' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(5))'))
      results = []
      stream.each { |v| results << v }
      expect(results).to eq([0, 1, 2, 3, 4])
    end

    it 'iterates over a Python list iterator' do
      stream = Rubyx.stream(Rubyx.eval('iter([10, 20, 30])'))
      results = []
      stream.each { |v| results << v }
      expect(results).to eq([10, 20, 30])
    end

    it 'handles an empty iterator' do
      stream = Rubyx.stream(Rubyx.eval('iter([])'))
      results = []
      stream.each { |v| results << v }
      expect(results).to eq([])
    end

    it 'streams string characters' do
      stream = Rubyx.stream(Rubyx.eval('iter("abc")'))
      expect(stream.to_a).to eq(%w[a b c])
    end

    it 'streams mixed Python types correctly' do
      stream = Rubyx.stream(Rubyx.eval('iter([1, 2.5, "hello", True, False, None])'))
      result = stream.to_a
      expect(result).to eq([1, 2.5, 'hello', true, false, nil])
    end

    it 'streams a generator expression' do
      stream = Rubyx.stream(Rubyx.eval('(x * 10 for x in range(4))'))
      expect(stream.to_a).to eq([0, 10, 20, 30])
    end
  end

  # ========== Manual iteration with next ==========

  describe '#next' do
    it 'returns values one at a time' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(3))'))
      expect(stream.next).to eq(0)
      expect(stream.next).to eq(1)
      expect(stream.next).to eq(2)
    end

    it 'raises StopIteration when exhausted' do
      stream = Rubyx.stream(Rubyx.eval('iter([42])'))
      stream.next
      expect { stream.next }.to raise_error(StopIteration)
    end

    it 'raises StopIteration on empty stream' do
      stream = Rubyx.stream(Rubyx.eval('iter([])'))
      expect { stream.next }.to raise_error(StopIteration)
    end
  end

  # ========== Enumerable methods ==========

  describe 'Enumerable methods' do
    it '#to_a collects all values' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(5))'))
      expect(stream.to_a).to eq([0, 1, 2, 3, 4])
    end

    it '#map transforms values' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(5))'))
      expect(stream.map { |x| x * 2 }).to eq([0, 2, 4, 6, 8])
    end

    it '#select filters values' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(10))'))
      expect(stream.select(&:even?)).to eq([0, 2, 4, 6, 8])
    end

    it '#reject filters values' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(10))'))
      expect(stream.reject(&:even?)).to eq([1, 3, 5, 7, 9])
    end

    it '#reduce accumulates values' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(1, 6))'))
      expect(stream.reduce(0) { |sum, x| sum + x }).to eq(15)
    end

    it '#first returns the first value' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(100))'))
      expect(stream.first).to eq(0)
    end

    it '#take returns the first N values' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(100))'))
      expect(stream.take(3)).to eq([0, 1, 2])
    end
  end

  # ========== Lazy evaluation / early termination ==========

  describe 'lazy evaluation' do
    it 'take does not consume the entire stream' do
      # Large range — if not lazy, would take very long
      stream = Rubyx.stream(Rubyx.eval('iter(range(1000000))'))
      result = stream.take(5)
      expect(result).to eq([0, 1, 2, 3, 4])
    end

    it 'first does not consume the entire stream' do
      stream = Rubyx.stream(Rubyx.eval('iter(range(1000000))'))
      expect(stream.first).to eq(0)
    end
  end

  # ========== Error propagation ==========

  describe 'error propagation' do
    it 'propagates Python errors raised during iteration' do
      # Generator expression that raises ZeroDivisionError at x == 2
      gen = Rubyx.eval('(1 // 0 if x == 2 else x for x in range(5))')
      stream = Rubyx.stream(gen)
      expect { stream.to_a }.to raise_error(RuntimeError, /ZeroDivisionError/)
    end
  end

  # ========== Async generator integration ==========

  # Each eval gets a fresh namespace (no state leaks between calls).
  # eval now returns the last expression's value, so we can define
  # the async generator and call it on the last line.
  describe 'async generators' do
    def async_range_gen(n)
      Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
          for i in range(#{n}):
            await asyncio.sleep(0)
            yield i
        _agen()
      PY
    end

    def async_slow_gen(n)
      Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
          for i in range(#{n}):
            await asyncio.sleep(0.001)
            yield i
        _agen()
      PY
    end

    def async_boom_gen
      Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
          yield 1
          raise ValueError("async boom")
        _agen()
      PY
    end

    def async_empty_gen
      Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
          return
          yield
        _agen()
      PY
    end

    # ---- Basic functionality ----

    it 'Rubyx.stream auto-detects async generator via adapter path' do
      expect(Rubyx.stream(async_range_gen(5)).to_a).to eq([0, 1, 2, 3, 4])
    end

    it 'Rubyx.async_stream drives async generator from Rust' do
      expect(Rubyx.async_stream(async_range_gen(4)).to_a).to eq([0, 1, 2, 3])
    end

    it 'adapter and Rust-driving produce identical results' do
      expect(Rubyx.stream(async_range_gen(5)).to_a).to eq(Rubyx.async_stream(async_range_gen(5)).to_a)
    end

    # ---- Edge cases ----

    it 'handles empty async generator via adapter path' do
      expect(Rubyx.stream(async_empty_gen).to_a).to eq([])
    end

    it 'handles empty async generator via Rust driving' do
      expect(Rubyx.async_stream(async_empty_gen).to_a).to eq([])
    end

    # ---- Error handling ----

    it 'Rubyx.async_stream rejects non-async iterables' do
      sync_iter = Rubyx.eval('iter(range(3))')
      expect { Rubyx.async_stream(sync_iter) }
        .to raise_error(TypeError, /not an async iterable/)
    end

    it 'Rubyx.async_stream propagates async generator errors' do
      expect { Rubyx.async_stream(async_boom_gen).to_a }
        .to raise_error(RuntimeError, /ValueError|async boom/)
    end

    it 'Rubyx.stream propagates async generator errors via adapter path' do
      expect { Rubyx.stream(async_boom_gen).to_a }
        .to raise_error(RuntimeError, /ValueError|async boom/)
    end

    # ---- Manual iteration with #next ----

    it 'supports manual iteration with #next on async stream (adapter)' do
      stream = Rubyx.stream(async_range_gen(3))
      expect(stream.next).to eq(0)
      expect(stream.next).to eq(1)
      expect(stream.next).to eq(2)
      expect { stream.next }.to raise_error(StopIteration)
    end

    it 'supports manual iteration with #next on async stream (Rust driving)' do
      stream = Rubyx.async_stream(async_range_gen(3))
      expect(stream.next).to eq(0)
      expect(stream.next).to eq(1)
      expect(stream.next).to eq(2)
      expect { stream.next }.to raise_error(StopIteration)
    end

    # ---- Lazy evaluation / early termination ----

    it 'take does not hang on large async generator (adapter)' do
      result = Timeout.timeout(5) { Rubyx.stream(async_slow_gen(10_000)).take(3) }
      expect(result).to eq([0, 1, 2])
    end

    it 'take does not hang on large async generator (Rust driving)' do
      result = Timeout.timeout(5) { Rubyx.async_stream(async_slow_gen(10_000)).take(3) }
      expect(result).to eq([0, 1, 2])
    end

    it 'first works on async generators without hanging (adapter)' do
      result = Timeout.timeout(5) { Rubyx.stream(async_range_gen(1000)).first }
      expect(result).to eq(0)
    end

    it 'first works on async generators without hanging (Rust driving)' do
      result = Timeout.timeout(5) { Rubyx.async_stream(async_range_gen(1000)).first }
      expect(result).to eq(0)
    end

    # ---- Enumerable methods on async streams ----

    it 'supports select on async streams' do
      expect(Rubyx.stream(async_range_gen(6)).select(&:even?)).to eq([0, 2, 4])
    end

    it 'supports map on async streams' do
      expect(Rubyx.stream(async_range_gen(5)).map { |x| x * 10 }).to eq([0, 10, 20, 30, 40])
    end

    it 'supports reduce on async streams' do
      expect(Rubyx.stream(async_range_gen(5)).reduce(0) { |sum, x| sum + x }).to eq(10)
    end

    it 'supports reject on async streams' do
      expect(Rubyx.stream(async_range_gen(6)).reject(&:even?)).to eq([1, 3, 5])
    end

    # ---- AST splitting with async code ----

    it 'splits async generator with await in loop body' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
            for i in [10, 20, 30]:
                await asyncio.sleep(0)
                yield i * 2
        _agen()
      PY
      expect(Rubyx.stream(result).to_a).to eq([20, 40, 60])
    end

    it 'splits async generator with multiple awaits' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
            await asyncio.sleep(0)
            yield "a"
            await asyncio.sleep(0)
            yield "b"
            await asyncio.sleep(0)
            yield "c"
        _agen()
      PY
      expect(Rubyx.stream(result).to_a).to eq(%w[a b c])
    end

    it 'splits async generator with try/except around await' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
            for i in range(3):
                try:
                    await asyncio.sleep(0)
                    yield i
                except Exception:
                    yield -1
        _agen()
      PY
      expect(Rubyx.stream(result).to_a).to eq([0, 1, 2])
    end

    it 'splits async generator with helper function defined before it' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        def transform(x):
            return x ** 2
        async def _agen():
            for i in range(4):
                await asyncio.sleep(0)
                yield transform(i)
        _agen()
      PY
      expect(Rubyx.stream(result).to_a).to eq([0, 1, 4, 9])
    end

    it 'splits async generator with class and async method' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        class AsyncCounter:
            def __init__(self, n):
                self.n = n
            async def count(self):
                for i in range(self.n):
                    await asyncio.sleep(0)
                    yield i
        AsyncCounter(3).count()
      PY
      expect(Rubyx.stream(result).to_a).to eq([0, 1, 2])
    end

    it 'splits async generator with decorator' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        def logged(fn):
            async def wrapper(*args):
                async for item in fn(*args):
                    yield item
            return wrapper
        @logged
        async def _agen():
            for i in range(3):
                await asyncio.sleep(0)
                yield i + 100
        _agen()
      PY
      expect(Rubyx.stream(result).to_a).to eq([100, 101, 102])
    end

    it 'splits async generator with conditional yields' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
            for i in range(6):
                await asyncio.sleep(0)
                if i % 2 == 0:
                    yield i
        _agen()
      PY
      expect(Rubyx.stream(result).to_a).to eq([0, 2, 4])
    end

    it 'splits code with multiple imports before async generator' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        import math
        async def _agen():
            for i in range(1, 5):
                await asyncio.sleep(0)
                yield math.factorial(i)
        _agen()
      PY
      expect(Rubyx.stream(result).to_a).to eq([1, 2, 6, 24])
    end

    # ---- AST splitting with async_stream (Rust-driving) ----

    it 'async_stream splits async generator with await in loop body' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
            for i in [10, 20, 30]:
                await asyncio.sleep(0)
                yield i * 2
        _agen()
      PY
      expect(Rubyx.async_stream(result).to_a).to eq([20, 40, 60])
    end

    it 'async_stream splits async generator with multiple awaits' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
            await asyncio.sleep(0)
            yield "a"
            await asyncio.sleep(0)
            yield "b"
            await asyncio.sleep(0)
            yield "c"
        _agen()
      PY
      expect(Rubyx.async_stream(result).to_a).to eq(%w[a b c])
    end

    it 'async_stream splits async generator with try/except around await' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
            for i in range(3):
                try:
                    await asyncio.sleep(0)
                    yield i
                except Exception:
                    yield -1
        _agen()
      PY
      expect(Rubyx.async_stream(result).to_a).to eq([0, 1, 2])
    end

    it 'async_stream splits async generator with helper function' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        def transform(x):
            return x ** 2
        async def _agen():
            for i in range(4):
                await asyncio.sleep(0)
                yield transform(i)
        _agen()
      PY
      expect(Rubyx.async_stream(result).to_a).to eq([0, 1, 4, 9])
    end

    it 'async_stream splits async generator with class and async method' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        class AsyncCounter:
            def __init__(self, n):
                self.n = n
            async def count(self):
                for i in range(self.n):
                    await asyncio.sleep(0)
                    yield i
        AsyncCounter(3).count()
      PY
      expect(Rubyx.async_stream(result).to_a).to eq([0, 1, 2])
    end

    it 'async_stream splits async generator with decorator' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        def logged(fn):
            async def wrapper(*args):
                async for item in fn(*args):
                    yield item
            return wrapper
        @logged
        async def _agen():
            for i in range(3):
                await asyncio.sleep(0)
                yield i + 100
        _agen()
      PY
      expect(Rubyx.async_stream(result).to_a).to eq([100, 101, 102])
    end

    it 'async_stream splits async generator with conditional yields' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
            for i in range(6):
                await asyncio.sleep(0)
                if i % 2 == 0:
                    yield i
        _agen()
      PY
      expect(Rubyx.async_stream(result).to_a).to eq([0, 2, 4])
    end

    it 'async_stream splits code with multiple imports before async generator' do
      result = Rubyx.eval(<<~PY)
        import asyncio
        import math
        async def _agen():
            for i in range(1, 5):
                await asyncio.sleep(0)
                yield math.factorial(i)
        _agen()
      PY
      expect(Rubyx.async_stream(result).to_a).to eq([1, 2, 6, 24])
    end
  end
end

RSpec.describe 'Rubyx::NonBlockingStream', ruby_integration: true do
  # ========== Class structure ==========

  describe 'class structure' do
    it 'defines the Rubyx::NonBlockingStream class' do
      expect(defined?(Rubyx::NonBlockingStream)).to eq('constant')
    end

    it 'includes Enumerable' do
      expect(Rubyx::NonBlockingStream.ancestors).to include(Enumerable)
    end

    it 'has an each method' do
      expect(Rubyx::NonBlockingStream.method_defined?(:each)).to be true
    end

    it 'has Enumerable methods from each' do
      %i[map select reject reduce first take to_a].each do |method|
        expect(Rubyx::NonBlockingStream.method_defined?(method)).to be(true),
                                                                     "expected Rubyx::NonBlockingStream to have #{method}"
      end
    end
  end

  # ========== Rubyx.nb_stream factory ==========

  describe 'Rubyx.nb_stream' do
    it 'responds to .nb_stream' do
      expect(Rubyx).to respond_to(:nb_stream)
    end

    it 'creates a Rubyx::NonBlockingStream from a Python iterator' do
      gen = Rubyx.eval('iter(range(5))')
      stream = Rubyx.nb_stream(gen)
      expect(stream).to be_a(Rubyx::NonBlockingStream)
    end

    it 'raises TypeError for non-RubyxObject arguments' do
      expect { Rubyx.nb_stream('not a python object') }.to raise_error(StandardError)
    end

    it 'raises TypeError for non-iterable Python objects' do
      py_int = Rubyx.eval('42')
      expect { Rubyx.nb_stream(py_int) }.to raise_error(StandardError, /not iterable/)
    end
  end

  # ========== Streaming with each ==========

  describe '#each' do
    it 'iterates over all values from a Python range' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter(range(5))'))
      results = []
      stream.each { |v| results << v }
      expect(results).to eq([0, 1, 2, 3, 4])
    end

    it 'iterates over a Python list iterator' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter([10, 20, 30])'))
      results = []
      stream.each { |v| results << v }
      expect(results).to eq([10, 20, 30])
    end

    it 'handles an empty iterator' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter([])'))
      results = []
      stream.each { |v| results << v }
      expect(results).to eq([])
    end

    it 'streams mixed Python types correctly' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter([1, 2.5, "hello", True, False, None])'))
      result = stream.to_a
      expect(result).to eq([1, 2.5, 'hello', true, false, nil])
    end

    it 'streams a generator expression' do
      stream = Rubyx.nb_stream(Rubyx.eval('(x * 10 for x in range(4))'))
      expect(stream.to_a).to eq([0, 10, 20, 30])
    end

    it 'streams string characters' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter("abc")'))
      expect(stream.to_a).to eq(%w[a b c])
    end
  end

  # ========== Enumerable methods ==========

  describe 'Enumerable methods' do
    it '#to_a collects all values' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter(range(5))'))
      expect(stream.to_a).to eq([0, 1, 2, 3, 4])
    end

    it '#map transforms values' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter(range(5))'))
      expect(stream.map { |x| x * 2 }).to eq([0, 2, 4, 6, 8])
    end

    it '#select filters values' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter(range(10))'))
      expect(stream.select(&:even?)).to eq([0, 2, 4, 6, 8])
    end

    it '#reject filters values' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter(range(10))'))
      expect(stream.reject(&:even?)).to eq([1, 3, 5, 7, 9])
    end

    it '#reduce accumulates values' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter(range(1, 6))'))
      expect(stream.reduce(0) { |sum, x| sum + x }).to eq(15)
    end

    it '#first returns the first value' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter(range(5))'))
      expect(stream.first).to eq(0)
    end

    it '#take returns the first N values' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter(range(5))'))
      expect(stream.take(3)).to eq([0, 1, 2])
    end
  end

  # ========== Error propagation ==========

  describe 'error propagation' do
    it 'propagates Python errors raised during iteration' do
      gen = Rubyx.eval('(1 // 0 if x == 2 else x for x in range(5))')
      stream = Rubyx.nb_stream(gen)
      expect { stream.to_a }.to raise_error(RuntimeError, /ZeroDivisionError/)
    end
  end

  # ========== Ordering and large streams ==========

  describe 'ordering' do
    it 'preserves item order for 100 items' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter(range(100))'))
      expect(stream.to_a).to eq((0..99).to_a)
    end

    it 'preserves item order for 1000 items' do
      stream = Rubyx.nb_stream(Rubyx.eval('iter(range(1000))'))
      expect(stream.to_a).to eq((0..999).to_a)
    end
  end

  # ========== Python generator (yield) ==========

  describe 'Python generators' do
    it 'streams a Python generator function' do
      gen = Rubyx.eval(<<~PY)
        def _gen():
            for i in range(5):
                yield i * i
        _gen()
      PY
      expect(Rubyx.nb_stream(gen).to_a).to eq([0, 1, 4, 9, 16])
    end

    it 'streams a Python generator with mixed yields' do
      gen = Rubyx.eval(<<~PY)
        def _gen():
            yield 42
            yield "hello"
            yield 3.14
            yield True
            yield None
        _gen()
      PY
      expect(Rubyx.nb_stream(gen).to_a).to eq([42, 'hello', 3.14, true, nil])
    end
  end

  # ========== Async generator support ==========

  describe 'async generators' do
    def async_range_gen(n)
      Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
          for i in range(#{n}):
            await asyncio.sleep(0)
            yield i
        _agen()
      PY
    end

    it 'streams an async generator via nb_stream' do
      expect(Rubyx.nb_stream(async_range_gen(5)).to_a).to eq([0, 1, 2, 3, 4])
    end

    it 'handles an empty async generator' do
      gen = Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
          return
          yield
        _agen()
      PY
      expect(Rubyx.nb_stream(gen).to_a).to eq([])
    end

    it 'propagates async generator errors' do
      gen = Rubyx.eval(<<~PY)
        import asyncio
        async def _agen():
          yield 1
          raise ValueError("async boom")
        _agen()
      PY
      expect { Rubyx.nb_stream(gen).to_a }.to raise_error(RuntimeError, /ValueError|async boom/)
    end

    it 'take works on small async generator' do
      result = Rubyx.nb_stream(async_range_gen(5)).take(3)
      expect(result).to eq([0, 1, 2])
    end
  end

  # ========== GVL release (other threads run during streaming) ==========

  describe 'GVL release' do
    it 'allows other Ruby threads to run during streaming' do
      counter = 0
      stop = false

      counter_thread = Thread.new do
        until stop
          counter += 1
          sleep 0.01
        end
      end

      gen = Rubyx.eval(<<~PY)
        import time
        def _slow():
            for i in range(5):
                time.sleep(0.1)
                yield i
        _slow()
      PY

      Rubyx.nb_stream(gen).each { |_| }

      stop = true
      counter_thread.join

      expect(counter).to be > 10,
        "Counter was #{counter}, expected > 10 (other threads should run during streaming)"
    end
  end

  # ========== Rubyx.eval with globals ==========

  describe '.eval with globals' do
    it 'injects integer globals' do
      result = Rubyx.eval('x + y', x: 10, y: 20)
      expect(result.to_ruby).to eq(30)
    end

    it 'injects string globals' do
      result = Rubyx.eval("f'Hello, {name}!'", name: 'Alice')
      expect(result.to_ruby).to eq('Hello, Alice!')
    end

    it 'injects float globals' do
      result = Rubyx.eval('a * b', a: 2.5, b: 4.0)
      expect(result.to_ruby).to eq(10.0)
    end

    it 'injects boolean globals' do
      result = Rubyx.eval('flag', flag: true)
      expect(result.to_ruby).to eq(true)
    end

    it 'injects nil as None' do
      result = Rubyx.eval('val is None', val: nil)
      expect(result.to_ruby).to eq(true)
    end

    it 'injects array as list' do
      result = Rubyx.eval('sum(items)', items: [1, 2, 3, 4])
      expect(result.to_ruby).to eq(10)
    end

    it 'injects hash as dict' do
      result = Rubyx.eval("data['a'] + data['b']", data: { 'a' => 100, 'b' => 200 })
      expect(result.to_ruby).to eq(300)
    end

    it 'injects symbol keys as string keys in dict' do
      result = Rubyx.eval("d['name']", d: { name: 'Bob' })
      expect(result.to_ruby).to eq('Bob')
    end

    it 'injects nested structures' do
      result = Rubyx.eval('len(data["items"])', data: { 'items' => [1, 2, 3] })
      expect(result.to_ruby).to eq(3)
    end

    it 'works with multiline code and globals' do
      code = <<~PY
        total = 0
        for v in values:
            total += v
        total
      PY
      result = Rubyx.eval(code, values: [10, 20, 30])
      expect(result.to_ruby).to eq(60)
    end

    it 'works with no globals (backward compatible)' do
      result = Rubyx.eval('2 ** 10')
      expect(result.to_ruby).to eq(1024)
    end

    it 'raises on undefined variable not in globals' do
      expect { Rubyx.eval('x + missing', x: 1) }.to raise_error(Exception)
    end

    it 'injects RubyxObject as passthrough' do
      py_list = Rubyx.eval('[1, 2, 3]')
      result = Rubyx.eval('len(items)', items: py_list)
      expect(result.to_ruby).to eq(3)
    end

    it 'globals do not leak between calls' do
      Rubyx.eval('x + 1', x: 10)
      expect { Rubyx.eval('x') }.to raise_error(Exception)
    end
  end
end
