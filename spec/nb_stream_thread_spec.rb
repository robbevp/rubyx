require_relative 'spec_helper'

RSpec.describe 'NonBlockingStream thread tests', ruby_integration: true do
  # Define Python helper functions once — eval'd into a fresh namespace each time
  def slow_range(n, delay = 0.1)
    Rubyx.eval(<<~PY)
      import time
      def _slow_range():
          for i in range(#{n}):
              time.sleep(#{delay})
              yield i
      _slow_range()
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

  # ========== GVL release: other threads run during streaming ==========

  describe 'GVL release' do
    it 'allows other Ruby threads to run while streaming' do
      counter = 0
      stop = false

      # Counter thread — should increment freely during recv()
      counter_thread = Thread.new do
        until stop
          counter += 1
          sleep 0.01
        end
      end

      # Stream thread — each recv() releases GVL
      gen = slow_range(5, 0.1)
      Rubyx.nb_stream(gen).each { |_| }

      stop = true
      counter_thread.join

      # Counter should have incremented many times during ~0.5s of streaming
      expect(counter).to be > 10,
        "Counter was #{counter}, expected > 10 (threads were blocked)"
    end

    it 'allows multiple stream threads to run concurrently' do
      results = [nil, nil]

      t1 = Thread.new do
        gen = slow_range(5, 0.1)
        results[0] = Rubyx.nb_stream(gen).to_a
      end

      t2 = Thread.new do
        gen = slow_range(5, 0.1)
        results[1] = Rubyx.nb_stream(gen).to_a
      end

      start = Time.now
      t1.join
      t2.join
      elapsed = Time.now - start

      expect(results[0]).to eq([0, 1, 2, 3, 4])
      expect(results[1]).to eq([0, 1, 2, 3, 4])

      # Two 0.5s streams should complete in ~0.5s, not ~1.0s
      expect(elapsed).to be < 1.0,
        "Two concurrent streams took #{elapsed}s, expected < 1.0s (threads not concurrent)"
    end
  end

  # ========== Thread#kill support via unblock function ==========
  #
  # NOTE: Thread#kill aborts the consumer but the producer thread may still
  # hold the Python GIL, which can corrupt subsequent Python calls in the
  # same process. Run this test in isolation:
  #   bundle exec rspec spec/nb_stream_thread_spec.rb -e "Thread#kill"

  describe 'Thread#kill', :isolate do
    it 'unblocks a streaming thread promptly' do
      gen = slow_range(1000, 0.1) # Would take 100s

      t = Thread.new do
        Rubyx.nb_stream(gen).each { |_| }
      end

      sleep 0.2 # Let it start
      start = Time.now
      t.kill
      t.join

      elapsed = Time.now - start
      expect(elapsed).to be < 0.2,
        "Thread#kill took #{elapsed}s, expected < 0.2s"
    end
  end

  # ========== Basic streaming correctness ==========

  describe 'basic streaming' do
    it 'streams all values correctly' do
      gen = fast_range(10)
      result = Rubyx.nb_stream(gen).to_a
      expect(result).to eq((0..9).to_a)
    end

    it 'preserves item order for 100 items' do
      gen = fast_range(100)
      result = Rubyx.nb_stream(gen).to_a
      expect(result).to eq((0..99).to_a)
    end

    it 'handles empty generator' do
      gen = Rubyx.eval(<<~PY)
        def _empty():
            return
            yield
        _empty()
      PY
      result = Rubyx.nb_stream(gen).to_a
      expect(result).to eq([])
    end

    it 'streams mixed types' do
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
      expect(result).to eq([42, 'hello', 3.14, true, nil])
    end
  end

  # ========== Error propagation ==========

  describe 'error propagation' do
    it 'propagates Python errors raised during iteration' do
      gen = Rubyx.eval(<<~PY)
        def _error_gen():
            yield 1
            yield 2
            raise ValueError("test error from generator")
        _error_gen()
      PY
      stream = Rubyx.nb_stream(gen)
      expect { stream.to_a }.to raise_error(RuntimeError, /ValueError|test error/)
    end
  end

  # ========== Streaming from multiple threads sequentially ==========

  describe 'sequential multi-thread usage' do
    it 'works correctly when used from different threads sequentially' do
      results = []

      3.times do
        t = Thread.new do
          gen = fast_range(5)
          Rubyx.nb_stream(gen).to_a
        end
        results << t.value
      end

      results.each do |r|
        expect(r).to eq([0, 1, 2, 3, 4])
      end
    end
  end
end
