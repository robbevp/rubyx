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
