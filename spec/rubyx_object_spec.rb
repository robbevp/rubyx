require_relative 'spec_helper'

RSpec.describe 'RubyxObject', ruby_integration: true do
  # ========== to_s ==========

  describe '#to_s' do
    it 'returns string representation of integer' do
      obj = Rubyx.eval('42')
      expect(obj.to_s).to eq('42')
    end

    it 'returns string value for Python string' do
      obj = Rubyx.eval('"hello world"')
      expect(obj.to_s).to eq('hello world')
    end

    it 'returns "None" for Python None' do
      obj = Rubyx.eval('None')
      expect(obj.to_s).to eq('None')
    end

    it 'returns "True" for Python True' do
      obj = Rubyx.eval('True')
      expect(obj.to_s).to eq('True')
    end

    it 'returns "False" for Python False' do
      obj = Rubyx.eval('False')
      expect(obj.to_s).to eq('False')
    end

    it 'returns float string' do
      obj = Rubyx.eval('3.14')
      expect(obj.to_s).to start_with('3.14')
    end

    it 'returns list representation' do
      obj = Rubyx.eval('[1, 2, 3]')
      expect(obj.to_s).to eq('[1, 2, 3]')
    end

    it 'returns dict representation' do
      obj = Rubyx.eval("{'a': 1}")
      expect(obj.to_s).to eq("{'a': 1}")
    end

    it 'works with print' do
      obj = Rubyx.eval('42')
      expect { print obj }.to output('42').to_stdout
    end

    it 'works with string interpolation' do
      obj = Rubyx.eval('"world"')
      expect("hello #{obj}").to eq('hello world')
    end
  end

  # ========== inspect ==========

  describe '#inspect' do
    it 'returns repr for integer' do
      obj = Rubyx.eval('42')
      expect(obj.inspect).to eq('42')
    end

    it 'returns repr for string (with quotes)' do
      obj = Rubyx.eval('"hello"')
      expect(obj.inspect).to eq("'hello'")
    end

    it 'returns repr for None' do
      obj = Rubyx.eval('None')
      expect(obj.inspect).to eq('None')
    end

    it 'returns repr for list' do
      obj = Rubyx.eval('[1, 2, 3]')
      expect(obj.inspect).to eq('[1, 2, 3]')
    end

    it 'differs from to_s for strings' do
      obj = Rubyx.eval('"test"')
      expect(obj.to_s).to eq('test')       # Python str()
      expect(obj.inspect).to eq("'test'")  # Python repr()
    end
  end

  # ========== to_ruby ==========

  describe '#to_ruby' do
    it 'converts Python int to Ruby Integer' do
      obj = Rubyx.eval('42')
      result = obj.to_ruby
      expect(result).to eq(42)
      expect(result).to be_a(Integer)
    end

    it 'converts Python float to Ruby Float' do
      obj = Rubyx.eval('3.14')
      result = obj.to_ruby
      expect(result).to be_within(0.001).of(3.14)
      expect(result).to be_a(Float)
    end

    it 'converts Python str to Ruby String' do
      obj = Rubyx.eval('"hello"')
      result = obj.to_ruby
      expect(result).to eq('hello')
      expect(result).to be_a(String)
    end

    it 'converts Python True to Ruby true' do
      obj = Rubyx.eval('True')
      result = obj.to_ruby
      expect(result).to eq(true)
    end

    it 'converts Python False to Ruby false' do
      obj = Rubyx.eval('False')
      result = obj.to_ruby
      expect(result).to eq(false)
    end

    it 'converts Python None to Ruby nil' do
      obj = Rubyx.eval('None')
      result = obj.to_ruby
      expect(result).to be_nil
    end

    it 'converts Python list to Ruby Array' do
      obj = Rubyx.eval('[1, 2, 3]')
      result = obj.to_ruby
      expect(result).to eq([1, 2, 3])
      expect(result).to be_a(Array)
    end

    it 'converts Python dict to Ruby Hash' do
      obj = Rubyx.eval("{'key': 'value', 'num': 42}")
      result = obj.to_ruby
      expect(result).to be_a(Hash)
      expect(result['key']).to eq('value')
      expect(result['num']).to eq(42)
    end

    it 'converts nested structures' do
      obj = Rubyx.eval("{'items': [1, 2, 3], 'nested': {'a': True}}")
      result = obj.to_ruby
      expect(result['items']).to eq([1, 2, 3])
      expect(result['nested']).to eq({ 'a' => true })
    end

    it 'raises for unconvertible types (modules)' do
      obj = Rubyx.import('os')
      expect { obj.to_ruby }.to raise_error(RuntimeError)
    end
  end

  # ========== method_missing ==========

  describe '#method_missing' do
    it 'calls Python methods on imported modules' do
      ctx = Rubyx.context
      ctx.eval('import json')
      json_mod = ctx.eval('json')
      result = json_mod.loads('[1, 2, 3]')
      expect(result).to be_a(RubyxObject)
      expect(result.to_ruby).to eq([1, 2, 3])
    end

    it 'reads Python attributes' do
      ctx = Rubyx.context
      ctx.eval('import sys')
      sys_mod = ctx.eval('sys')
      version = sys_mod.version
      expect(version).to be_a(RubyxObject)
      expect(version.to_s).not_to be_empty
    end

    it 'raises for nonexistent attributes' do
      obj = Rubyx.import('sys')
      expect { obj.nonexistent_attr_xyz_123 }.to raise_error(Exception)
    end

    it 'calls methods with string arguments' do
      ctx = Rubyx.context
      ctx.eval("import json")
      json_mod = ctx.eval("json")
      result = json_mod.loads('[1, 2, 3]')
      expect(result.to_ruby).to eq([1, 2, 3])
    end
  end

  # ========== class identity ==========

  describe 'class identity' do
    it 'is a RubyxObject' do
      obj = Rubyx.eval('42')
      expect(obj).to be_a(RubyxObject)
    end

    it 'eval results are RubyxObject' do
      expect(Rubyx.eval('1 + 1')).to be_a(RubyxObject)
    end

    it 'import results are RubyxObject' do
      expect(Rubyx.import('os')).to be_a(RubyxObject)
    end
  end

  # ========== integration with streaming ==========

  describe 'integration with streaming' do
    it 'to_ruby works on streamed values (they are already Ruby)' do
      gen = Rubyx.eval('iter([1, 2, 3])')
      results = Rubyx.stream(gen).to_a
      # Streamed values are already Ruby primitives
      expect(results).to eq([1, 2, 3])
      expect(results.first).to be_a(Integer)
    end

    it 'to_ruby on eval result matches streamed value' do
      eval_result = Rubyx.eval('42')
      stream_result = Rubyx.stream(Rubyx.eval('iter([42])')).first

      expect(eval_result.to_ruby).to eq(stream_result)
    end
  end
end
