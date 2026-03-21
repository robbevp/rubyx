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

if File.exist?(bundle_path)
  require bundle_path
else
  warn 'Extension not built. Run: cargo build --release'
  warn 'Skipping Ruby integration tests.'
  RSpec.configure { |c| c.filter_run_excluding ruby_integration: true }
end

# Initialize Python via Rubyx.init if the extension loaded and Rubyx.init is available
if defined?(Rubyx) && Rubyx.respond_to?(:init)
  python_info = `python3 -c "
import sysconfig, sys, os, platform
libdir = sysconfig.get_config_var('LIBDIR')
ver = f'{sys.version_info.major}.{sys.version_info.minor}'
ext = 'dylib' if platform.system() == 'Darwin' else 'so'
print(os.path.join(libdir, f'libpython{ver}.{ext}'))
print(sys.base_prefix)
print(sys.executable)
" 2>/dev/null`.strip.split("\n")

  if python_info.length == 3
    python_dl, python_home, python_exe = python_info

    # Detect venv site-packages for sys_paths injection
    sys_paths = `python3 -c "import site; print('\\n'.join(site.getsitepackages()))" 2>/dev/null`.strip.split("\n").select { |p| !p.empty? }

    Rubyx.init(python_dl, python_home, python_exe, sys_paths)
  else
    warn 'Could not detect Python paths. Skipping Ruby integration tests.'
    RSpec.configure { |c| c.filter_run_excluding ruby_integration: true }
  end
end
