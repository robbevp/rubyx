require_relative 'spec_helper'

RSpec.describe 'Rubyx::Context', ruby_integration: true do
  # ========== Class structure ==========

  describe 'class structure' do
    it 'defines the Rubyx::Context class' do
      expect(defined?(Rubyx::Context)).to eq('constant')
    end

    it 'responds to .new' do
      expect(Rubyx::Context).to respond_to(:new)
    end

    it 'instance responds to #eval' do
      ctx = Rubyx::Context.new
      expect(ctx).to respond_to(:eval)
    end
  end

  # ========== Factory method ==========

  describe 'Rubyx.context factory' do
    it 'responds to .context' do
      expect(Rubyx).to respond_to(:context)
    end

    it 'returns a Rubyx::Context instance' do
      ctx = Rubyx.context
      expect(ctx).to be_a(Rubyx::Context)
    end
  end

  # ========== State persistence ==========

  describe 'state persistence' do
    it 'persists variables across eval calls' do
      ctx = Rubyx::Context.new
      ctx.eval("x = 42")
      result = ctx.eval("iter([x + 8])")
      expect(Rubyx.stream(result).to_a).to eq([50])
    end

    it 'accumulates list state' do
      ctx = Rubyx.context
      ctx.eval("items = []")
      ctx.eval("items.append(1)")
      ctx.eval("items.append(2)")
      ctx.eval("items.append(3)")
      result = ctx.eval("items")
      expect(Rubyx.stream(result).to_a).to eq([1, 2, 3])
    end

    it 'persists function definitions' do
      ctx = Rubyx.context
      ctx.eval("def double(n): return n * 2")
      result = ctx.eval("iter([double(21)])")
      expect(Rubyx.stream(result).to_a).to eq([42])
    end

    it 'persists imports' do
      ctx = Rubyx.context
      ctx.eval("import json")
      result = ctx.eval("json.dumps({'key': 'value'})")
      expect(result).not_to be_nil
    end

    it 'persists class definitions' do
      ctx = Rubyx.context
      ctx.eval(<<~PY)
        class Counter:
            def __init__(self):
                self.count = 0
            def inc(self):
                self.count += 1
                return self.count
      PY
      ctx.eval("c = Counter()")
      r1 = ctx.eval("iter([c.inc()])")
      r2 = ctx.eval("iter([c.inc()])")
      r3 = ctx.eval("iter([c.inc()])")
      expect(Rubyx.stream(r1).to_a).to eq([1])
      expect(Rubyx.stream(r2).to_a).to eq([2])
      expect(Rubyx.stream(r3).to_a).to eq([3])
    end

    it 'persists dictionary state' do
      ctx = Rubyx.context
      ctx.eval("d = {}")
      ctx.eval("d['a'] = 1")
      ctx.eval("d['b'] = 2")
      result = ctx.eval("iter([len(d)])")
      expect(Rubyx.stream(result).to_a).to eq([2])
    end
  end

  # ========== Isolation between contexts ==========

  describe 'isolation' do
    it 'separate contexts do not share state' do
      ctx1 = Rubyx::Context.new
      ctx2 = Rubyx::Context.new
      ctx1.eval("shared_var = 'from ctx1'")
      expect { ctx2.eval("shared_var") }.to raise_error(StandardError)
    end

    it 'original Rubyx.eval remains isolated' do
      Rubyx.eval("secret = 999")
      expect { Rubyx.eval("secret") }.to raise_error(StandardError)
    end

    it 'context state does not leak to Rubyx.eval' do
      ctx = Rubyx.context
      ctx.eval("context_only = 42")
      expect { Rubyx.eval("context_only") }.to raise_error(StandardError)
    end

    it 'Rubyx.eval state does not leak to context' do
      ctx = Rubyx.context
      Rubyx.eval("eval_only = 123")
      expect { ctx.eval("eval_only") }.to raise_error(StandardError)
    end
  end

  # ========== Error handling ==========

  describe 'error handling' do
    it 'raises on NameError' do
      ctx = Rubyx.context
      expect { ctx.eval("undefined_var") }.to raise_error(StandardError)
    end

    it 'raises on ZeroDivisionError' do
      ctx = Rubyx.context
      expect { ctx.eval("1 / 0") }.to raise_error(StandardError)
    end

    it 'raises on SyntaxError' do
      ctx = Rubyx.context
      expect { ctx.eval("def class for") }.to raise_error(Exception)
    end

    it 'error does not corrupt context state' do
      ctx = Rubyx.context
      ctx.eval("x = 10")
      expect { ctx.eval("1 / 0") }.to raise_error(StandardError)
      result = ctx.eval("iter([x])")
      expect(Rubyx.stream(result).to_a).to eq([10])
    end

    it 'context is usable after multiple errors' do
      ctx = Rubyx.context
      ctx.eval("y = 5")
      3.times do
        expect { ctx.eval("undefined") }.to raise_error(StandardError)
      end
      result = ctx.eval("iter([y])")
      expect(Rubyx.stream(result).to_a).to eq([5])
    end
  end

  # ========== Returned objects survive context drop ==========

  describe 'object survival after context drop' do
    it 'returned objects survive context garbage collection' do
      result = nil
      ctx = Rubyx.context
      ctx.eval("data = [1, 2, 3]")
      result = ctx.eval("iter(data)")
      ctx = nil
      GC.start

      expect(Rubyx.stream(result).to_a).to eq([1, 2, 3])
    end

    it 'returned integer survives context drop' do
      ctx = Rubyx.context
      result = ctx.eval("42")
      ctx = nil
      GC.start

      expect(result).not_to be_nil
    end
  end

  # ========== Multiple concurrent contexts ==========

  describe 'multiple contexts' do
    it 'supports multiple independent contexts' do
      contexts = 3.times.map { Rubyx.context }
      contexts.each_with_index { |ctx, i| ctx.eval("val = #{i * 10}") }

      results = contexts.map { |ctx| ctx.eval("iter([val])") }
      streamed = results.map { |r| Rubyx.stream(r).to_a }
      expect(streamed).to eq([[0], [10], [20]])
    end

    it 'contexts can hold different types' do
      int_ctx = Rubyx.context
      str_ctx = Rubyx.context

      int_ctx.eval("x = 42")
      str_ctx.eval("x = 'hello'")

      int_result = int_ctx.eval("iter([x])")
      str_result = str_ctx.eval("iter([x])")

      expect(Rubyx.stream(int_result).to_a).to eq([42])
      expect(Rubyx.stream(str_result).to_a).to eq(['hello'])
    end
  end

  # ========== Multiline code in context ==========

  describe 'multiline code' do
    it 'handles multiline statements with trailing expression' do
      ctx = Rubyx.context
      result = ctx.eval(<<~PY)
        a = 3
        b = 4
        iter([a * b])
      PY
      expect(Rubyx.stream(result).to_a).to eq([12])
    end

    it 'handles function def and call in single eval' do
      ctx = Rubyx.context
      result = ctx.eval(<<~PY)
        def square(x):
            return x * x
        iter([square(7)])
      PY
      expect(Rubyx.stream(result).to_a).to eq([49])
    end

    it 'handles all-statement code (no trailing expression)' do
      ctx = Rubyx.context
      result = ctx.eval(<<~PY)
        x = 10
        y = 20
      PY
      expect(result).not_to be_nil
    end
  end

  # ========== Streaming from persistent context ==========

  describe 'streaming integration' do
    it 'streams a generator defined in context' do
      ctx = Rubyx.context
      ctx.eval(<<~PY)
        def count_up(n):
            for i in range(n):
                yield i
      PY
      gen = ctx.eval("count_up(5)")
      expect(Rubyx.stream(gen).to_a).to eq([0, 1, 2, 3, 4])
    end

    it 'streams a generator multiple times from the same context' do
      ctx = Rubyx.context
      ctx.eval("def gen(n): return (i * 2 for i in range(n))")

      r1 = ctx.eval("gen(3)")
      r2 = ctx.eval("gen(4)")

      expect(Rubyx.stream(r1).to_a).to eq([0, 2, 4])
      expect(Rubyx.stream(r2).to_a).to eq([0, 2, 4, 6])
    end
  end

  # ========== Async generators in persistent context ==========

  describe 'async generators' do
    it 'streams async generator from persistent context' do
      ctx = Rubyx.context
      ctx.eval(<<~PY)
        import asyncio
        async def async_count(n):
            for i in range(n):
                await asyncio.sleep(0)
                yield i
      PY
      gen = ctx.eval("async_count(4)")
      expect(Rubyx.stream(gen).to_a).to eq([0, 1, 2, 3])
    end

    it 'reuses async generator factory across calls' do
      ctx = Rubyx.context
      ctx.eval(<<~PY)
        import asyncio
        async def async_range(n):
            for i in range(n):
                await asyncio.sleep(0)
                yield i
      PY

      r1 = ctx.eval("async_range(3)")
      r2 = ctx.eval("async_range(5)")

      expect(Rubyx.stream(r1).to_a).to eq([0, 1, 2])
      expect(Rubyx.stream(r2).to_a).to eq([0, 1, 2, 3, 4])
    end
  end

  # ========== Context#eval with globals ==========

  describe '#eval with globals' do
    it 'injects globals into context eval' do
      ctx = Rubyx::Context.new
      result = ctx.eval('x + y', x: 10, y: 20)
      expect(result.to_ruby).to eq(30)
    end

    it 'injected globals persist in context' do
      ctx = Rubyx::Context.new
      ctx.eval('z = x * 2', x: 21)
      result = ctx.eval('z')
      expect(result.to_ruby).to eq(42)
    end

    it 'injects string globals' do
      ctx = Rubyx::Context.new
      result = ctx.eval("f'{greeting}, {name}!'", greeting: 'Hi', name: 'World')
      expect(result.to_ruby).to eq('Hi, World!')
    end

    it 'injects array globals' do
      ctx = Rubyx::Context.new
      result = ctx.eval('max(items) - min(items)', items: [3, 7, 1, 9])
      expect(result.to_ruby).to eq(8)
    end

    it 'injects hash globals' do
      ctx = Rubyx::Context.new
      result = ctx.eval("config['debug']", config: { 'debug' => true })
      expect(result.to_ruby).to eq(true)
    end

    it 'overrides previously injected globals' do
      ctx = Rubyx::Context.new
      ctx.eval('x', x: 10)
      result = ctx.eval('x', x: 99)
      expect(result.to_ruby).to eq(99)
    end

    it 'mixes injected globals with context state' do
      ctx = Rubyx::Context.new
      ctx.eval('base = 100')
      result = ctx.eval('base + offset', offset: 42)
      expect(result.to_ruby).to eq(142)
    end

    it 'works without globals (backward compatible)' do
      ctx = Rubyx::Context.new
      ctx.eval('val = 5')
      result = ctx.eval('val * 3')
      expect(result.to_ruby).to eq(15)
    end
  end

  # ========== Context#await with globals ==========

  describe '#await with globals' do
    it 'awaits async code with globals' do
      ctx = Rubyx::Context.new
      ctx.eval("import asyncio\nasync def multiply(a, b): return a * b")
      result = ctx.await('multiply(a, b)', a: 6, b: 7)
      expect(result).to eq(42)
    end

    it 'injected globals persist after await' do
      ctx = Rubyx::Context.new
      ctx.eval("import asyncio\nasync def store(val): return val")
      ctx.await('store(x)', x: 99)
      result = ctx.eval('x')
      expect(result.to_ruby).to eq(99)
    end

    it 'propagates errors from async with globals' do
      ctx = Rubyx::Context.new
      ctx.eval("import asyncio\nasync def check(n):\n    if n < 0: raise ValueError('neg')\n    return n")
      expect { ctx.await('check(n)', n: -1) }.to raise_error(StandardError, /neg/)
    end
  end

  # ========== Context#async_await with globals ==========

  describe '#async_await with globals' do
    it 'returns Future with globals' do
      ctx = Rubyx::Context.new
      ctx.eval("import asyncio\nasync def add(x, y): return x + y")
      future = ctx.async_await('add(x, y)', x: 15, y: 27)
      expect(future).to be_a(Rubyx::Future)
      expect(future.value).to eq(42)
    end

    it 'injected globals persist after async_await' do
      ctx = Rubyx::Context.new
      ctx.eval("import asyncio\nasync def identity(v): return v")
      future = ctx.async_await('identity(val)', val: 'hello')
      future.value
      result = ctx.eval('val')
      expect(result.to_ruby).to eq('hello')
    end

    it 'propagates errors from async with globals' do
      ctx = Rubyx::Context.new
      ctx.eval("import asyncio\nasync def div(a, b): return a / b")
      future = ctx.async_await('div(a, b)', a: 10, b: 0)
      expect { future.value }.to raise_error(StandardError, /division by zero|ZeroDivisionError/)
    end
  end
end
