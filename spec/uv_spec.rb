require_relative 'spec_helper'
require 'digest'
require 'tmpdir'

RSpec.describe Rubyx::Uv do
  let(:uv_version) { Rubyx::Uv::DEFAULT_UV_VERSION }

  # ========== Module structure ==========

  describe 'module structure' do
    it 'defines the Rubyx::Uv module' do
      expect(defined?(Rubyx::Uv)).to eq('constant')
    end

    it 'defines DEFAULT_UV_VERSION' do
      expect(Rubyx::Uv::DEFAULT_UV_VERSION).to be_a(String)
      expect(Rubyx::Uv::DEFAULT_UV_VERSION).to match(/\d+\.\d+\.\d+/)
    end

    it 'defines error classes' do
      expect(Rubyx::Uv::Error).to be < StandardError
      expect(Rubyx::Uv::SetupError).to be < Rubyx::Uv::Error
      expect(Rubyx::Uv::InitError).to be < Rubyx::Uv::Error
    end

    it 'responds to .setup' do
      expect(Rubyx::Uv).to respond_to(:setup)
    end

    it 'responds to .init' do
      expect(Rubyx::Uv).to respond_to(:init)
    end
  end

  # ========== resolve_project_dir ==========

  describe '.resolve_project_dir (private)' do
    it 'uses Dir.pwd when project_dir is nil' do
      result = Rubyx::Uv.send(:resolve_project_dir, 'content', uv_version, nil)
      expect(result).to eq(Dir.pwd)
    end

    it 'uses cache with MD5 hash when project_dir is :cache' do
      content = "some pyproject content"
      result = Rubyx::Uv.send(:resolve_project_dir, content, uv_version, :cache)
      expect(result).to include(Digest::MD5.hexdigest(content))
      expect(result).to include('projects')
    end

    it 'produces different cache paths for different content' do
      result_a = Rubyx::Uv.send(:resolve_project_dir, 'content_a', uv_version, :cache)
      result_b = Rubyx::Uv.send(:resolve_project_dir, 'content_b', uv_version, :cache)
      expect(result_a).not_to eq(result_b)
    end

    it 'produces same cache path for same content' do
      result1 = Rubyx::Uv.send(:resolve_project_dir, 'same', uv_version, :cache)
      result2 = Rubyx::Uv.send(:resolve_project_dir, 'same', uv_version, :cache)
      expect(result1).to eq(result2)
    end

    it 'expands path when project_dir is a string' do
      result = Rubyx::Uv.send(:resolve_project_dir, 'content', uv_version, '~/my/project')
      expect(result).to eq(File.expand_path('~/my/project'))
    end

    it 'expands relative paths' do
      result = Rubyx::Uv.send(:resolve_project_dir, 'content', uv_version, './relative/path')
      expect(result).to eq(File.expand_path('./relative/path'))
    end
  end

  # ========== cache_dir and path helpers ==========

  describe 'path helpers (private)' do
    it 'cache_dir includes rubyx version and uv version' do
      result = Rubyx::Uv.send(:cache_dir, '0.8.5')
      expect(result).to include(Rubyx::VERSION)
      expect(result).to include('0.8.5')
      expect(result).to include('rubyx')
    end

    it 'cache_dir respects XDG_CACHE_HOME' do
      original = ENV['XDG_CACHE_HOME']
      begin
        ENV['XDG_CACHE_HOME'] = '/tmp/custom_cache'
        result = Rubyx::Uv.send(:cache_dir, uv_version)
        expect(result).to start_with('/tmp/custom_cache')
      ensure
        if original
          ENV['XDG_CACHE_HOME'] = original
        else
          ENV.delete('XDG_CACHE_HOME')
        end
      end
    end

    it 'cache_dir defaults to ~/.cache when XDG_CACHE_HOME is unset' do
      original = ENV.delete('XDG_CACHE_HOME')
      begin
        result = Rubyx::Uv.send(:cache_dir, uv_version)
        expect(result).to start_with(File.join(Dir.home, '.cache'))
      ensure
        ENV['XDG_CACHE_HOME'] = original if original
      end
    end

    it 'default_uv_path is inside cache_dir' do
      result = Rubyx::Uv.send(:default_uv_path, uv_version)
      cache = Rubyx::Uv.send(:cache_dir, uv_version)
      expect(result).to start_with(cache)
      expect(result).to end_with('bin/uv')
    end

    it 'python_install_dir is inside cache_dir' do
      result = Rubyx::Uv.send(:python_install_dir, uv_version)
      cache = Rubyx::Uv.send(:cache_dir, uv_version)
      expect(result).to start_with(cache)
      expect(result).to end_with('python')
    end
  end

  # ========== archive_name_for_platform ==========

  describe '.archive_name_for_platform (private)' do
    it 'returns a valid archive type and name' do
      type, name = Rubyx::Uv.send(:archive_name_for_platform)
      expect([:tar_gz, :zip]).to include(type)
      expect(name).to be_a(String)
      expect(name).to match(/^uv-/)
    end

    it 'returns tar.gz for non-windows platforms' do
      unless RUBY_PLATFORM =~ /mingw|mswin|cygwin/
        type, _name = Rubyx::Uv.send(:archive_name_for_platform)
        expect(type).to eq(:tar_gz)
      end
    end
  end

  # ========== platform_paths ==========

  describe '.platform_paths (private)' do
    it 'returns a hash with required keys' do
      # Use the current venv as a stand-in for testing
      root_dir = '/tmp/fake_python_root'
      project_dir = '/tmp/fake_project'

      paths = Rubyx::Uv.send(:platform_paths, root_dir, project_dir)
      expect(paths).to be_a(Hash)
      expect(paths).to have_key(:python_dl)
      expect(paths).to have_key(:python_home)
      expect(paths).to have_key(:python_exe)
      expect(paths).to have_key(:venv_packages)
    end

    it 'sets python_home to root_dir' do
      paths = Rubyx::Uv.send(:platform_paths, '/some/root', '/some/project')
      expect(paths[:python_home]).to eq('/some/root')
    end

    it 'sets python_exe inside the venv' do
      paths = Rubyx::Uv.send(:platform_paths, '/some/root', '/some/project')
      expect(paths[:python_exe]).to include('.venv')
      expect(paths[:python_exe]).to include('python')
    end

    it 'uses correct extension for the current platform' do
      paths = Rubyx::Uv.send(:platform_paths, '/root', '/project')
      case RUBY_PLATFORM
      when /darwin/
        # python_dl glob pattern includes .dylib
        expect(paths[:python_exe]).to end_with('.venv/bin/python')
      when /linux/
        expect(paths[:python_exe]).to end_with('.venv/bin/python')
      when /mingw|mswin|cygwin/
        expect(paths[:python_exe]).to end_with('.venv/Scripts/python.exe')
      end
    end
  end

  # ========== find_lib ==========

  describe '.find_lib (private)' do
    it 'returns nil when no files match' do
      result = Rubyx::Uv.send(:find_lib, '/nonexistent', '*.xyz')
      expect(result).to be_nil
    end

    it 'finds matching files' do
      Dir.mktmpdir do |dir|
        FileUtils.touch(File.join(dir, 'libpython3.12.so'))
        result = Rubyx::Uv.send(:find_lib, dir, 'libpython3.*.so')
        expect(result).to end_with('libpython3.12.so')
      end
    end

    it 'prefers shorter names (no version suffix)' do
      Dir.mktmpdir do |dir|
        FileUtils.touch(File.join(dir, 'libpython3.12.so'))
        FileUtils.touch(File.join(dir, 'libpython3.12.so.1.0'))
        result = Rubyx::Uv.send(:find_lib, dir, 'libpython3.12.so*')
        expect(result).to end_with('libpython3.12.so')
      end
    end
  end

  # ========== validate_paths! ==========

  describe '.validate_paths! (private)' do
    it 'raises InitError for missing paths' do
      paths = { python_dl: '/nonexistent/libpython.so', python_home: '/tmp' }
      expect {
        Rubyx::Uv.send(:validate_paths!, paths)
      }.to raise_error(Rubyx::Uv::InitError, /python_dl/)
    end

    it 'allows nil venv_packages' do
      Dir.mktmpdir do |dir|
        fake_file = File.join(dir, 'libpython.so')
        FileUtils.touch(fake_file)
        paths = { python_dl: fake_file, python_home: dir, python_exe: fake_file, venv_packages: nil }
        expect { Rubyx::Uv.send(:validate_paths!, paths) }.not_to raise_error
      end
    end

    it 'passes when all paths exist' do
      Dir.mktmpdir do |dir|
        fake_file = File.join(dir, 'libpython.so')
        FileUtils.touch(fake_file)
        site_dir = File.join(dir, 'site-packages')
        FileUtils.mkdir_p(site_dir)
        paths = { python_dl: fake_file, python_home: dir, python_exe: fake_file, venv_packages: site_dir }
        expect { Rubyx::Uv.send(:validate_paths!, paths) }.not_to raise_error
      end
    end
  end

  # ========== run_uv! with uv_path ==========

  describe '.run_uv! with uv_path (private)' do
    it 'raises SetupError when custom uv_path does not exist' do
      expect {
        Rubyx::Uv.send(:run_uv!,
          ['--version'],
          chdir: Dir.pwd,
          env: {},
          uv_version: uv_version,
          uv_path: '/nonexistent/uv'
        )
      }.to raise_error(Rubyx::Uv::SetupError, /uv not found/)
    end

    it 'uses custom uv_path when provided and it exists' do
      # Find system uv if available
      system_uv = `which uv 2>/dev/null`.strip
      skip 'uv not installed on system' if system_uv.empty? || !File.exist?(system_uv)

      result = Rubyx::Uv.send(:run_uv!,
        ['--version'],
        chdir: Dir.pwd,
        env: {},
        uv_version: uv_version,
        uv_path: system_uv
      )
      expect(result).to be true
    end
  end

  # ========== init error handling ==========

  describe '.init error handling' do
    it 'raises InitError when .venv does not exist' do
      Dir.mktmpdir do |dir|
        expect {
          Rubyx::Uv.init('content', project_dir: dir)
        }.to raise_error(Rubyx::Uv::InitError, /Not set up/)
      end
    end

    it 'raises InitError when pyvenv.cfg is missing' do
      Dir.mktmpdir do |dir|
        FileUtils.mkdir_p(File.join(dir, '.venv'))
        expect {
          Rubyx::Uv.init('content', project_dir: dir)
        }.to raise_error(Rubyx::Uv::InitError, /pyvenv.cfg/)
      end
    end

    it 'raises InitError when pyvenv.cfg has no home line' do
      Dir.mktmpdir do |dir|
        venv_dir = File.join(dir, '.venv')
        FileUtils.mkdir_p(venv_dir)
        File.write(File.join(venv_dir, 'pyvenv.cfg'), "version_info = 3.12\n")
        expect {
          Rubyx::Uv.init('content', project_dir: dir)
        }.to raise_error(Rubyx::Uv::InitError, /home/)
      end
    end
  end

  # ========== setup idempotency ==========

  describe '.setup idempotency' do
    it 'skips setup when .venv exists and pyproject.toml matches' do
      Dir.mktmpdir do |dir|
        pyproject = "[project]\nname = \"test\"\nversion = \"0.1.0\"\n"

        # Create existing .venv and matching pyproject.toml
        FileUtils.mkdir_p(File.join(dir, '.venv'))
        File.write(File.join(dir, 'pyproject.toml'), pyproject)

        # Should not call run_uv! at all — just return the dir
        result = Rubyx::Uv.setup(pyproject, project_dir: dir)
        expect(result).to eq(File.expand_path(dir))
      end
    end

    it 'detects when pyproject.toml content changed' do
      Dir.mktmpdir do |dir|
        old_content = "[project]\nname = \"test\"\nversion = \"0.1.0\"\n"
        new_content = "[project]\nname = \"test\"\nversion = \"0.2.0\"\n"

        # Simulate previous setup
        FileUtils.mkdir_p(File.join(dir, '.venv'))
        File.write(File.join(dir, 'pyproject.toml'), old_content)

        # New content should trigger setup — which will fail without uv,
        # but we can verify it tried by catching the error
        expect {
          Rubyx::Uv.setup(new_content, project_dir: dir, uv_path: '/nonexistent/uv')
        }.to raise_error(Rubyx::Uv::SetupError)
      end
    end
  end

  # ========== Rubyx.uv_init convenience ==========

  describe 'Rubyx.uv_init' do
    it 'is defined as a convenience method' do
      expect(Rubyx).to respond_to(:uv_init)
    end
  end
end
