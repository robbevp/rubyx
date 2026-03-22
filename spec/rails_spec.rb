require_relative 'spec_helper'
require 'tmpdir'
require_relative '../lib/rubyx/rails'

RSpec.describe Rubyx::Rails do
  # Reset configuration between tests
  before(:each) do
    Rubyx::Rails.instance_variable_set(:@configuration, nil)
    Rubyx::Rails.instance_variable_set(:@initialized, nil)
  end

  # ========== Configuration ==========

  describe 'Configuration' do
    it 'has sensible defaults' do
      config = Rubyx::Rails::Configuration.new
      expect(config.pyproject_path).to be_nil
      expect(config.pyproject_content).to be_nil
      expect(config.auto_init).to eq(false)
      expect(config.force_reinit).to eq(false)
      expect(config.uv_version).to eq(Rubyx::Uv::DEFAULT_UV_VERSION)
      expect(config.debug).to eq(false)
      expect(config.python_paths).to eq([])
      expect(config.uv_path).to be_nil
      expect(config.uv_args).to eq([])
    end

    it 'allows setting all options' do
      config = Rubyx::Rails::Configuration.new
      config.pyproject_path = '/path/to/pyproject.toml'
      config.pyproject_content = '[project]'
      config.auto_init = true
      config.force_reinit = true
      config.uv_version = '0.9.0'
      config.debug = true
      config.python_paths = ['/app/python']
      config.uv_path = '/usr/local/bin/uv'
      config.uv_args = ['--extra', 'ml']

      expect(config.pyproject_path).to eq('/path/to/pyproject.toml')
      expect(config.pyproject_content).to eq('[project]')
      expect(config.auto_init).to eq(true)
      expect(config.force_reinit).to eq(true)
      expect(config.uv_version).to eq('0.9.0')
      expect(config.debug).to eq(true)
      expect(config.python_paths).to eq(['/app/python'])
      expect(config.uv_path).to eq('/usr/local/bin/uv')
      expect(config.uv_args).to eq(['--extra', 'ml'])
    end
  end

  # ========== configure block ==========

  describe '.configure' do
    it 'yields the configuration' do
      Rubyx::Rails.configure do |config|
        config.auto_init = true
        config.debug = true
      end

      expect(Rubyx::Rails.configuration.auto_init).to eq(true)
      expect(Rubyx::Rails.configuration.debug).to eq(true)
    end

    it 'returns the same configuration instance' do
      config1 = Rubyx::Rails.configuration
      config2 = Rubyx::Rails.configuration
      expect(config1).to be(config2)
    end

    it 'allows multiple configure calls to accumulate settings' do
      Rubyx::Rails.configure { |c| c.auto_init = true }
      Rubyx::Rails.configure { |c| c.debug = true }

      expect(Rubyx::Rails.configuration.auto_init).to eq(true)
      expect(Rubyx::Rails.configuration.debug).to eq(true)
    end
  end

  # ========== initialized? ==========

  describe '.initialized?' do
    it 'returns false before init!' do
      expect(Rubyx::Rails.initialized?).to eq(false)
    end
  end

  # ========== ensure_initialized! ==========

  describe '.ensure_initialized!' do
    it 'raises when no pyproject configured' do
      expect { Rubyx::Rails.ensure_initialized! }.to raise_error(Rubyx::Rails::Error, /pyproject/)
    end

    it 'does not raise if already initialized' do
      Rubyx::Rails.instance_variable_set(:@initialized, true)
      expect { Rubyx::Rails.ensure_initialized! }.not_to raise_error
    end
  end

  # ========== resolve_pyproject (private) ==========

  describe 'resolve_pyproject (private)' do
    it 'reads from pyproject_path when file exists' do
      Dir.mktmpdir do |dir|
        path = File.join(dir, 'pyproject.toml')
        File.write(path, "[project]\nname = \"test\"\n")

        config = Rubyx::Rails::Configuration.new
        config.pyproject_path = path

        result = Rubyx::Rails.send(:resolve_pyproject, config)
        expect(result).to include('[project]')
        expect(result).to include('name = "test"')
      end
    end

    it 'uses pyproject_content when path is nil' do
      config = Rubyx::Rails::Configuration.new
      config.pyproject_content = "[project]\nname = \"inline\"\n"

      result = Rubyx::Rails.send(:resolve_pyproject, config)
      expect(result).to include('name = "inline"')
    end

    it 'prefers pyproject_path over pyproject_content' do
      Dir.mktmpdir do |dir|
        path = File.join(dir, 'pyproject.toml')
        File.write(path, "from_file")

        config = Rubyx::Rails::Configuration.new
        config.pyproject_path = path
        config.pyproject_content = "from_inline"

        result = Rubyx::Rails.send(:resolve_pyproject, config)
        expect(result).to eq("from_file")
      end
    end

    it 'raises when neither path nor content is set' do
      config = Rubyx::Rails::Configuration.new

      expect {
        Rubyx::Rails.send(:resolve_pyproject, config)
      }.to raise_error(Rubyx::Rails::Error, /No pyproject/)
    end

    it 'raises when pyproject_path does not exist' do
      config = Rubyx::Rails::Configuration.new
      config.pyproject_path = '/nonexistent/pyproject.toml'

      expect {
        Rubyx::Rails.send(:resolve_pyproject, config)
      }.to raise_error(Rubyx::Rails::Error, /No pyproject/)
    end
  end

  # ========== resolve_project_dir (private) ==========

  describe 'resolve_project_dir (private)' do
    it 'uses directory of pyproject_path' do
      config = Rubyx::Rails::Configuration.new
      config.pyproject_path = '/my/rails/app/pyproject.toml'

      result = Rubyx::Rails.send(:resolve_project_dir, config)
      expect(result).to eq('/my/rails/app')
    end

    it 'falls back when pyproject_path is nil' do
      # Without Rails.root available, this would use whatever fallback is defined
      config = Rubyx::Rails::Configuration.new
      config.pyproject_path = nil

      # Should not raise — returns some directory
      result = Rubyx::Rails.send(:resolve_project_dir, config)
      expect(result).to be_a(String)
    end
  end

  # ========== inject_python_paths (private) ==========

  describe 'inject_python_paths (private)' do
    it 'does nothing with empty array' do
      expect {
        Rubyx::Rails.send(:inject_python_paths, [])
      }.not_to raise_error
    end

    it 'does nothing with nil' do
      expect {
        Rubyx::Rails.send(:inject_python_paths, nil)
      }.not_to raise_error
    end

    it 'injects existing directories into sys.path' do
      Dir.mktmpdir do |dir|
        Rubyx::Rails.send(:inject_python_paths, [dir])

        # Verify via Python
        gen = Rubyx.eval("import sys\niter([str('#{dir}' in sys.path)])")
        result = Rubyx.stream(gen).first
        expect(result).to eq('True')
      end
    end

    it 'skips non-existent directories' do
      expect {
        Rubyx::Rails.send(:inject_python_paths, ['/nonexistent/path/xyz'])
      }.not_to raise_error
    end
  end

  # ========== Error class ==========

  describe 'Error' do
    it 'inherits from Rubyx::Error' do
      expect(Rubyx::Rails::Error).to be < Rubyx::Error
    end
  end
end
