require_relative 'spec_helper'

RSpec.describe 'Complete Integration', ruby_integration: true do
  # ========== Module name validation ==========

  describe 'module name validation' do
    it 'raises InvalidModuleNameError for names with spaces' do
      expect { Rubyx.import('not a module') }.to raise_error(Rubyx::InvalidModuleNameError)
    end

    it 'raises InvalidModuleNameError for names with special characters' do
      expect { Rubyx.import('bad!module') }.to raise_error(Rubyx::InvalidModuleNameError)
    end

    it 'raises InvalidModuleNameError for empty string' do
      expect { Rubyx.import('') }.to raise_error(Rubyx::InvalidModuleNameError)
    end

    it 'raises InvalidModuleNameError for names starting with a number' do
      expect { Rubyx.import('1module') }.to raise_error(Rubyx::InvalidModuleNameError)
    end

    it 'allows simple module names' do
      expect { Rubyx.import('os') }.not_to raise_error
    end

    it 'allows dotted module names' do
      expect { Rubyx.import('os.path') }.not_to raise_error
    end

    it 'accepts symbols' do
      expect { Rubyx.import(:os) }.not_to raise_error
    end
  end

  # ========== Eval wrapper ==========

  describe 'Rubyx.eval wrapper' do
    it 'returns a RubyxObject for expressions' do
      result = Rubyx.eval('1 + 2')
      expect(result).to be_a(RubyxObject)
    end

    it 'returns a RubyxObject for string expressions' do
      result = Rubyx.eval('"hello"')
      expect(result).not_to be_nil
    end

    it 'raises on syntax errors' do
      expect { Rubyx.eval('def class for') }.to raise_error(Exception)
    end

    it 'raises on undefined variables' do
      expect { Rubyx.eval('undefined_xyz') }.to raise_error(StandardError)
    end
  end

  # ========== Import standard library ==========

  describe 'standard library imports' do
    it 'imports os module' do
      os = Rubyx.import('os')
      expect(os).to be_a(RubyxObject)
    end

    it 'imports json module' do
      json = Rubyx.import('json')
      expect(json).to be_a(RubyxObject)
    end

    it 'imports sys module' do
      sys = Rubyx.import('sys')
      expect(sys).to be_a(RubyxObject)
    end

    it 'imports math module' do
      math = Rubyx.import('math')
      expect(math).to be_a(RubyxObject)
    end

    it 'raises on nonexistent module' do
      expect { Rubyx.import('nonexistent_xyz_123') }.to raise_error(StandardError)
    end

    it 'can import the same module twice' do
      m1 = Rubyx.import('os')
      m2 = Rubyx.import('os')
      expect(m1).to be_a(RubyxObject)
      expect(m2).to be_a(RubyxObject)
    end
  end

  # ========== Local Python module imports ==========

  describe 'local Python module imports' do
    before(:all) do
      examples_dir = File.expand_path('../examples/python', __dir__)
      if Dir.exist?(examples_dir)
        Rubyx.eval("import sys; sys.path.insert(0, '#{examples_dir}')")
      end
    end

    it 'imports calculator module from examples/python' do
      calc = Rubyx.import('calculator')
      expect(calc).to be_a(RubyxObject)
    end

    it 'imports data_utils module from examples/python' do
      utils = Rubyx.import('data_utils')
      expect(utils).to be_a(RubyxObject)
    end

    it 'can import multiple local modules' do
      calc = Rubyx.import('calculator')
      utils = Rubyx.import('data_utils')
      expect(calc).to be_a(RubyxObject)
      expect(utils).to be_a(RubyxObject)
    end

    it 'uses calculator functions via eval' do
      result = Rubyx.eval('import calculator; calculator.add(3, 4)')
      expect(result).not_to be_nil
    end

    it 'uses data_utils functions via eval' do
      result = Rubyx.eval("import data_utils\ndata_utils.clean_text('  Hello   WORLD  ')")
      expect(result).not_to be_nil
    end

    it 'calls fibonacci via eval and streams result' do
      gen = Rubyx.eval("import calculator\niter(calculator.fibonacci(8))")
      results = Rubyx.stream(gen).to_a
      expect(results).to eq([0, 1, 1, 2, 3, 5, 8, 13])
    end
  end

  # ========== Streaming integration ==========

  describe 'streaming from eval' do
    it 'streams a sync generator' do
      gen = Rubyx.eval('(x ** 2 for x in range(5))')
      stream = Rubyx.stream(gen)
      expect(stream.to_a).to eq([0, 1, 4, 9, 16])
    end

    it 'lazily takes from a large generator' do
      gen = Rubyx.eval('(x for x in range(1000000))')
      result = Rubyx.stream(gen).take(3)
      expect(result).to eq([0, 1, 2])
    end

    it 'streams string characters' do
      gen = Rubyx.eval('iter("hello")')
      result = Rubyx.stream(gen).to_a
      expect(result).to eq(%w[h e l l o])
    end
  end

  # ========== Context integration ==========

  describe 'persistent context' do
    it 'creates a context' do
      ctx = Rubyx.context
      expect(ctx).to be_a(Rubyx::Context)
    end

    it 'evaluates code in context and returns RubyxObject' do
      ctx = Rubyx.context
      result = ctx.eval('42')
      expect(result).not_to be_nil
    end

    it 'persists variables across eval calls' do
      ctx = Rubyx.context
      ctx.eval('x = 10')
      ctx.eval('x = x + 5')
      # Verify via a streaming trick: eval returns RubyxObject,
      # but streaming an iter gives us Ruby primitives
      gen = ctx.eval('iter([x])')
      result = Rubyx.stream(gen).first
      expect(result).to eq(15)
    end

    it 'persists function definitions' do
      ctx = Rubyx.context
      ctx.eval('def double(n): return n * 2')
      gen = ctx.eval('iter([double(21)])')
      result = Rubyx.stream(gen).first
      expect(result).to eq(42)
    end
  end

  # ========== Error hierarchy ==========

  describe 'error class hierarchy' do
    it 'Rubyx::Error inherits from StandardError' do
      expect(Rubyx::Error).to be < StandardError
    end

    it 'Rubyx::PythonError inherits from Rubyx::Error' do
      expect(Rubyx::PythonError).to be < Rubyx::Error
    end

    it 'Rubyx::ImportError inherits from Rubyx::PythonError' do
      expect(Rubyx::ImportError).to be < Rubyx::PythonError
    end

    it 'Rubyx::InvalidModuleNameError inherits from Rubyx::Error' do
      expect(Rubyx::InvalidModuleNameError).to be < Rubyx::Error
    end

    it 'Rubyx::Uv::Error inherits from Rubyx::Error' do
      expect(Rubyx::Uv::Error).to be < Rubyx::Error
    end

    it 'Rubyx::Uv::SetupError inherits from Rubyx::Uv::Error' do
      expect(Rubyx::Uv::SetupError).to be < Rubyx::Uv::Error
    end

    it 'Rubyx::Uv::InitError inherits from Rubyx::Uv::Error' do
      expect(Rubyx::Uv::InitError).to be < Rubyx::Uv::Error
    end
  end
end
