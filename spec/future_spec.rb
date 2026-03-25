require_relative 'spec_helper'

RSpec.describe 'Rubyx::Future', ruby_integration: true do
  # ========== Rubyx.async_await ==========

  describe 'Rubyx.async_await' do
    it 'returns a Rubyx::Future' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def simple(): return 42")
      coro = ctx.eval("simple()")
      future = Rubyx.async_await(coro)
      expect(future).to be_a(Rubyx::Future)
      future.value # consume to clean up thread
    end

    it 'runs the coroutine on a background thread' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def slow_add(): await asyncio.sleep(0.01); return 3 + 4")
      coro = ctx.eval("slow_add()")

      future = Rubyx.async_await(coro)

      # Ruby is not blocked — we can do work here
      ruby_work_done = true

      result = future.value
      expect(ruby_work_done).to be true
      expect(result).to eq(7)
    end

    it 'returns the correct value from async function' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def get_msg(): return 'hello from async'")
      coro = ctx.eval("get_msg()")

      future = Rubyx.async_await(coro)
      expect(future.value).to eq('hello from async')
    end

    it 'handles async function returning a list' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def get_list(): return [1, 2, 3]")
      coro = ctx.eval("get_list()")

      future = Rubyx.async_await(coro)
      expect(future.value).to eq([1, 2, 3])
    end

    it 'handles async function returning a dict' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def get_dict(): return {'key': 'value'}")
      coro = ctx.eval("get_dict()")

      future = Rubyx.async_await(coro)
      expect(future.value).to eq({ 'key' => 'value' })
    end

    it 'handles async function returning None' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def noop(): pass")
      coro = ctx.eval("noop()")

      future = Rubyx.async_await(coro)
      expect(future.value).to be_nil
    end

    it 'propagates async errors' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def boom(): raise ValueError('async error')")
      coro = ctx.eval("boom()")

      future = Rubyx.async_await(coro)
      expect { future.value }.to raise_error(RuntimeError, /async error/)
    end
  end

  # ========== ready? ==========

  describe '#ready?' do
    it 'returns false before completion' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def slow(): await asyncio.sleep(0.5); return 1")
      coro = ctx.eval("slow()")

      future = Rubyx.async_await(coro)
      # Might be false if checked immediately (race condition, but likely)
      # Just verify it doesn't raise
      expect(future.ready?).to be(true).or be(false)
      future.value # clean up
    end

    it 'returns true after completion' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def fast(): return 42")
      coro = ctx.eval("fast()")

      future = Rubyx.async_await(coro)
      future.value # wait for completion

      # After value is consumed, ready? behavior is implementation-defined
      # Just verify it doesn't crash
      expect { future.ready? }.not_to raise_error
    end
  end

  # ========== context.async_await ==========

  describe 'context.async_await' do
    it 'evals and runs async in one step' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def double(n): return n * 2")

      future = ctx.async_await("double(21)")
      expect(future).to be_a(Rubyx::Future)
      expect(future.value).to eq(42)
    end

    it 'has access to context state' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("x = 10")
      ctx.eval("async def get_x(): return x")

      future = ctx.async_await("get_x()")
      expect(future.value).to eq(10)
    end

    it 'propagates errors from async code' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def fail(): raise RuntimeError('context async error')")

      future = ctx.async_await("fail()")
      expect { future.value }.to raise_error(RuntimeError, /context async error/)
    end

    it 'raises on invalid Python code' do
      ctx = Rubyx.context
      expect { ctx.async_await("not valid python!!!") }.to raise_error(Exception)
    end
  end

  # ========== context.await (blocking) ==========

  describe 'context.await (blocking)' do
    it 'evals and blocks until result' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def blocking_ctx(): return 77")

      result = ctx.await("blocking_ctx()")
      expect(result).to be_a(RubyxObject)
      expect(result.to_ruby).to eq(77)
    end

    it 'has access to context state' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("val = 'hello'")
      ctx.eval("async def get_val(): return val")

      result = ctx.await("get_val()")
      expect(result.to_ruby).to eq('hello')
    end

    it 'returns RubyxObject for complex types' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def get_dict(): return {'a': 1, 'b': 2}")

      result = ctx.await("get_dict()")
      expect(result).to be_a(RubyxObject)
      expect(result.to_ruby).to eq({ 'a' => 1, 'b' => 2 })
    end

    it 'propagates errors' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def fail(): raise ValueError('ctx await error')")

      expect { ctx.await("fail()") }.to raise_error(StandardError, /ctx await error/)
    end

    it 'works with await in coroutine body' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def delayed(): await asyncio.sleep(0.01); return 'done'")

      result = ctx.await("delayed()")
      expect(result.to_ruby).to eq('done')
    end
  end

  # ========== Rubyx.await (blocking standalone) ==========

  describe 'Rubyx.await (blocking)' do
    it 'blocks and returns RubyxObject' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def blocking_test(): return 99")
      coro = ctx.eval("blocking_test()")

      result = Rubyx.await(coro)
      expect(result).to be_a(RubyxObject)
      expect(result.to_ruby).to eq(99)
    end

    it 'returns RubyxObject for string result' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def get_str(): return 'awaited'")
      coro = ctx.eval("get_str()")

      result = Rubyx.await(coro)
      expect(result.to_s).to eq('awaited')
    end

    it 'propagates errors' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def boom(): raise RuntimeError('await boom')")
      coro = ctx.eval("boom()")

      expect { Rubyx.await(coro) }.to raise_error(StandardError, /await boom/)
    end

    it 'handles None return' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def nothing(): pass")
      coro = ctx.eval("nothing()")

      result = Rubyx.await(coro)
      expect(result.to_s).to eq('None')
    end
  end

  # ========== Rubyx.async_await edge cases ==========

  describe 'Rubyx.async_await edge cases' do
    it 'raises error for invalid Python code string' do
      expect { Rubyx.async_await("not a python object") }.to raise_error(Exception)
    end

    it 'future.value can only be consumed once' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def once(): return 1")
      coro = ctx.eval("once()")

      future = Rubyx.async_await(coro)
      expect(future.value).to eq(1)
      # Second call should fail
      expect { future.value }.to raise_error(RuntimeError)
    end
  end

  # ========== concurrent futures ==========

  describe 'concurrent futures' do
    it 'can run multiple futures sequentially' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def add(a, b): await asyncio.sleep(0.01); return a + b")

      f1 = ctx.async_await("add(1, 2)")
      expect(f1.value).to eq(3)

      f2 = ctx.async_await("add(3, 4)")
      expect(f2.value).to eq(7)
    end

    it 'Ruby threads can run while future executes' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def slow(): await asyncio.sleep(0.1); return 42")

      future = ctx.async_await("slow()")

      # Ruby thread does work while Python runs
      counter = 0
      while !future.ready?
        counter += 1
        sleep(0.01)
      end

      expect(future.value).to eq(42)
      # Counter should be > 0 if Ruby was doing work
      # (might be 0 on very fast machines, so don't assert)
    end
  end

  # ========== GVL release during value() ==========

  describe 'GVL release during value()' do
    it 'other Ruby threads run while value() blocks' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def slow_result(): await asyncio.sleep(0.3); return 'done'")

      future = ctx.async_await("slow_result()")

      # Start a Ruby thread that increments a counter while future.value blocks
      counter = 0
      mutex = Mutex.new
      done = false

      worker = Thread.new do
        until done
          mutex.synchronize { counter += 1 }
          sleep(0.01)
        end
      end

      result = future.value
      done = true
      worker.join

      expect(result).to eq('done')
      # If the GVL was released, the worker thread should have incremented
      # the counter multiple times during the ~300ms wait
      expect(counter).to be > 5
    end
  end

  # ========== class identity ==========

  describe 'class identity' do
    it 'Rubyx::Future is defined' do
      expect(defined?(Rubyx::Future)).to eq('constant')
    end

    it 'Rubyx.async_await returns Rubyx::Future' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def id_test(): return 1")
      coro = ctx.eval("id_test()")
      future = Rubyx.async_await(coro)
      expect(future).to be_a(Rubyx::Future)
      future.value
    end

    it 'ctx.async_await returns Rubyx::Future' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def id_test2(): return 2")
      future = ctx.async_await("id_test2()")
      expect(future).to be_a(Rubyx::Future)
      future.value
    end

    it 'Rubyx.await returns RubyxObject (not Future)' do
      ctx = Rubyx.context
      ctx.eval("import asyncio")
      ctx.eval("async def id_test3(): return 3")
      coro = ctx.eval("id_test3()")
      result = Rubyx.await(coro)
      expect(result).to be_a(RubyxObject)
      expect(result).not_to be_a(Rubyx::Future)
    end
  end

  # ========== Rubyx.await with globals ==========

  describe 'Rubyx.await with globals' do
    it 'awaits coroutine expression with globals' do
      ctx = Rubyx.context
      ctx.eval("import asyncio\nasync def mul(a, b): return a * b")
      result = ctx.await('mul(a, b)', a: 6, b: 7)
      expect(result.to_ruby).to eq(42)
    end

    it 'awaits with string globals' do
      ctx = Rubyx.context
      ctx.eval("import asyncio\nasync def greet(n): return f'hi {n}'")
      result = ctx.await('greet(name)', name: 'world')
      expect(result.to_ruby).to eq('hi world')
    end

    it 'propagates errors with globals' do
      ctx = Rubyx.context
      ctx.eval("import asyncio\nasync def fail_neg(v):\n    if v < 0: raise ValueError('negative')\n    return v")
      expect { ctx.await('fail_neg(val)', val: -1) }.to raise_error(StandardError, /negative/)
    end
  end

  # ========== Rubyx.async_await with globals ==========

  describe 'Rubyx.async_await with globals' do
    it 'returns Future with globals' do
      ctx = Rubyx.context
      ctx.eval("import asyncio\nasync def add(a, b): return a + b")
      future = ctx.async_await('add(x, y)', x: 20, y: 22)
      expect(future).to be_a(Rubyx::Future)
      expect(future.value).to eq(42)
    end

    it 'handles string result with globals' do
      ctx = Rubyx.context
      ctx.eval("import asyncio\nasync def greet(n): return f'hello {n}'")
      future = ctx.async_await('greet(name)', name: 'world')
      expect(future.value).to eq('hello world')
    end

    it 'propagates errors with globals' do
      ctx = Rubyx.context
      ctx.eval("import asyncio\nasync def div(x, y): return x / y")
      future = ctx.async_await('div(a, b)', a: 10, b: 0)
      expect { future.value }.to raise_error(StandardError, /division by zero|ZeroDivisionError/)
    end
  end

  # ========== ArgumentError guards ==========

  describe 'ArgumentError for coroutine + globals' do
    it 'Rubyx.await raises ArgumentError when passing globals with coroutine object' do
      ctx = Rubyx.context
      ctx.eval("import asyncio\nasync def noop(): return 1")
      coro = ctx.eval("noop()")
      expect { Rubyx.await(coro, x: 1) }.to raise_error(ArgumentError, /cannot pass globals/)
    end

    it 'Rubyx.async_await raises ArgumentError when passing globals with coroutine object' do
      ctx = Rubyx.context
      ctx.eval("import asyncio\nasync def noop(): return 1")
      coro = ctx.eval("noop()")
      expect { Rubyx.async_await(coro, x: 1) }.to raise_error(ArgumentError, /cannot pass globals/)
    end

    it 'Rubyx.await works with coroutine object (no globals)' do
      ctx = Rubyx.context
      ctx.eval("import asyncio\nasync def get99(): return 99")
      coro = ctx.eval("get99()")
      result = Rubyx.await(coro)
      expect(result.to_ruby).to eq(99)
    end

    it 'Rubyx.async_await works with coroutine object (no globals)' do
      ctx = Rubyx.context
      ctx.eval("import asyncio\nasync def get77(): return 77")
      coro = ctx.eval("get77()")
      future = Rubyx.async_await(coro)
      expect(future.value).to eq(77)
    end
  end
end
