require_relative 'spec_helper'

# Check if the async gem is available
begin
  require 'async'
  HAS_ASYNC = true
rescue LoadError
  HAS_ASYNC = false
end

RSpec.describe 'NonBlockingStream fiber tests', ruby_integration: true do
  before(:all) do
    skip 'async gem not available (gem install async)' unless HAS_ASYNC
  end

  # Define Python helper functions
  def timed_range(n, delay = 0.1)
    Rubyx.eval(<<~PY)
      import time
      def _timed_range():
          for i in range(#{n}):
              time.sleep(#{delay})
              yield i
      _timed_range()
    PY
  end

  def fast_range(n)
    Rubyx.eval(<<~PY)
      def _fast_range():
          for i in range(#{n}):
              yield i
      _fast_range()
    PY
  end

  # ========== Fiber Scheduler detection ==========

  describe 'Fiber Scheduler detection' do
    it 'detects Fiber Scheduler when async gem is active' do
      detected = nil
      Async do
        detected = Fiber.scheduler
      end
      expect(detected).not_to be_nil,
        "Fiber.scheduler should be set inside Async block"
    end

    it 'no Fiber Scheduler outside Async block' do
      expect(Fiber.scheduler).to be_nil,
        "Fiber.scheduler should be nil outside Async block"
    end
  end

  # ========== Concurrent fiber streaming ==========

  describe 'concurrent fibers' do
    it 'runs two streams concurrently on a single thread' do
      start = Time.now
      results = [[], []]

      Async do |task|
        2.times do |i|
          task.async do
            gen = timed_range(10, 0.1) # ~1s per stream
            Rubyx.nb_stream(gen).each { |v| results[i] << v }
          end
        end
      end

      elapsed = Time.now - start

      # Both streams should have all values
      expect(results[0]).to eq((0..9).to_a)
      expect(results[1]).to eq((0..9).to_a)

      # Two 1-second streams should complete in ~1s, not ~2s
      expect(elapsed).to be < 1.5,
        "Took #{elapsed}s — fibers not concurrent (expected < 1.5s)"
    end

    it 'runs three streams concurrently' do
      start = Time.now
      results = [[], [], []]

      Async do |task|
        3.times do |i|
          task.async do
            gen = timed_range(5, 0.1) # ~0.5s per stream
            Rubyx.nb_stream(gen).each { |v| results[i] << v }
          end
        end
      end

      elapsed = Time.now - start

      3.times do |i|
        expect(results[i]).to eq([0, 1, 2, 3, 4])
      end

      # Three 0.5s streams concurrently should take ~0.5s, not ~1.5s
      expect(elapsed).to be < 1.0,
        "Three concurrent streams took #{elapsed}s, expected < 1.0s"
    end
  end

  # ========== Basic fiber streaming ==========

  describe 'basic fiber streaming' do
    it 'streams values correctly inside Async block' do
      result = nil
      Async do
        gen = fast_range(5)
        result = Rubyx.nb_stream(gen).to_a
      end
      expect(result).to eq([0, 1, 2, 3, 4])
    end

    it 'handles empty stream inside Async block' do
      result = nil
      Async do
        gen = Rubyx.eval(<<~PY)
          def _empty():
              return
              yield
          _empty()
        PY
        result = Rubyx.nb_stream(gen).to_a
      end
      expect(result).to eq([])
    end

    it 'preserves item order inside Async block' do
      result = nil
      Async do
        gen = fast_range(100)
        result = Rubyx.nb_stream(gen).to_a
      end
      expect(result).to eq((0..99).to_a)
    end

    it 'streams mixed types inside Async block' do
      result = nil
      Async do
        gen = Rubyx.eval(<<~PY)
          def _mixed():
              yield 42
              yield "hello"
              yield 3.14
              yield True
              yield None
          _mixed()
        PY
        result = Rubyx.nb_stream(gen).to_a
      end
      expect(result).to eq([42, 'hello', 3.14, true, nil])
    end
  end

  # ========== Error propagation in fibers ==========

  describe 'error propagation in fibers' do
    it 'propagates Python errors inside Async task' do
      error = nil
      Async do |task|
        task.async do
          gen = Rubyx.eval(<<~PY)
            def _err():
                yield 1
                raise ValueError("fiber error")
            _err()
          PY
          begin
            Rubyx.nb_stream(gen).to_a
          rescue RuntimeError => e
            error = e
          end
        end
      end
      expect(error).not_to be_nil, "expected a RuntimeError to be raised"
      expect(error.message).to match(/ValueError|fiber error/)
    end
  end

  # ========== Enumerable methods in fibers ==========

  describe 'Enumerable in fibers' do
    it '#map works inside Async block' do
      result = nil
      Async do
        gen = fast_range(5)
        result = Rubyx.nb_stream(gen).map { |x| x * 10 }
      end
      expect(result).to eq([0, 10, 20, 30, 40])
    end

    it '#select works inside Async block' do
      result = nil
      Async do
        gen = fast_range(10)
        result = Rubyx.nb_stream(gen).select(&:even?)
      end
      expect(result).to eq([0, 2, 4, 6, 8])
    end

    it '#reduce works inside Async block' do
      result = nil
      Async do
        gen = fast_range(5)
        result = Rubyx.nb_stream(gen).reduce(0) { |sum, x| sum + x }
      end
      expect(result).to eq(10)
    end
  end
end
