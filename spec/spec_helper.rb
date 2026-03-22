require 'rspec'
require 'rbconfig'
require 'fileutils'
require 'timeout'

lib_ext = case RbConfig::CONFIG['host_os']
          when /darwin/ then 'dylib'
          when /linux/ then 'so'
          when /mingw|mswin/ then 'dll'
          else 'so'
          end

bundle_ext = case RbConfig::CONFIG['host_os']
             when /darwin/ then 'bundle'
             else lib_ext
             end

lib_path = File.expand_path("../target/release/librubyx.#{lib_ext}", __dir__)
bundle_path = File.expand_path("../target/release/rubyx.#{bundle_ext}", __dir__)

if File.exist?(lib_path) && (!File.exist?(bundle_path) || File.mtime(lib_path) > File.mtime(bundle_path))
  FileUtils.cp(lib_path, bundle_path)
end

# Add lib/ to the load path so require 'rubyx/uv' works
$LOAD_PATH.unshift File.expand_path('../lib', __dir__)

if File.exist?(bundle_path)
  # Load the native extension first (defines Rubyx module with _import, _eval, etc.)
  require bundle_path
  # Then load the Ruby wrappers (import, eval, uv_init, error classes)
  require 'rubyx/version'
  require 'rubyx/error'
  require 'rubyx/uv'

  # Define the Ruby-side wrappers that call the Rust _import/_eval methods
  module Rubyx
    def self.import(module_name)
      name = module_name.to_s
      unless name.match?(VALID_MODULE_NAME_PATTERN)
        raise InvalidModuleNameError,
              "Invalid Python module name: '#{name}'. " \
              "Module names must contain only alphanumeric characters, underscores, and dots."
      end
      _import(name)
    end

    class << self
      public define_method(:eval) { |code| _eval(code.to_s) }
    end

    def self.uv_init(pyproject_toml, **options)
      Uv.setup(pyproject_toml, **options)
      Uv.init(pyproject_toml, **options)
    end
  end
else
  warn 'Extension not built. Run: cargo build --release'
  warn 'Skipping Ruby integration tests.'
  RSpec.configure { |c| c.filter_run_excluding ruby_integration: true }
end

# Initialize Python via Rubyx.init if the extension loaded and Rubyx.init is available
if defined?(Rubyx) && Rubyx.respond_to?(:init)
  # Find python3: prefer project .venv, then activated venv, then system
  project_root = File.expand_path('..', __dir__)
  python3 = [
    File.join(project_root, '.venv', 'bin', 'python3'),
    `which python3 2>/dev/null`.strip,
  ].find { |p| !p.empty? && File.exist?(p) }

  if python3
    python_info = `#{python3} -c "
import sysconfig, sys, os, platform
libdir = sysconfig.get_config_var('LIBDIR')
ver = f'{sys.version_info.major}.{sys.version_info.minor}'
ext = 'dylib' if platform.system() == 'Darwin' else 'so'
print(os.path.join(libdir, f'libpython{ver}.{ext}'))
print(sys.base_prefix)
print(sys.executable)
" 2>/dev/null`.strip.split("\n")
  end

  if python_info && python_info.length == 3
    python_dl, python_home, python_exe = python_info

    # Detect venv site-packages for sys_paths injection
    sys_paths = `#{python3} -c "import site; print('\\n'.join(site.getsitepackages()))" 2>/dev/null`.strip.split("\n").select { |p| !p.empty? }

    Rubyx.init(python_dl, python_home, python_exe, sys_paths)
  else
    warn 'Could not detect Python paths. Skipping Ruby integration tests.'
    RSpec.configure { |c| c.filter_run_excluding ruby_integration: true }
  end
end
