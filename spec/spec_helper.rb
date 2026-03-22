require 'rspec'
require 'fileutils'
require 'timeout'

# Add lib/ to the load path
$LOAD_PATH.unshift File.expand_path('../lib', __dir__)

begin
  require 'rubyx'
rescue LoadError => e
  warn "Extension not loaded: #{e.message}"
  warn 'Skipping Ruby integration tests.'
  RSpec.configure { |c| c.filter_run_excluding ruby_integration: true }
end

# Initialize Python if Rubyx loaded successfully
if defined?(Rubyx) && Rubyx.respond_to?(:init)
  # Find python3: prefer project .venv, then system
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

    sys_paths = `#{python3} -c "import site; print('\\n'.join(site.getsitepackages()))" 2>/dev/null`.strip.split("\n").select { |p| !p.empty? }

    Rubyx.init(python_dl, python_home, python_exe, sys_paths)
  else
    warn 'Could not detect Python paths. Skipping Ruby integration tests.'
    RSpec.configure { |c| c.filter_run_excluding ruby_integration: true }
  end
end
