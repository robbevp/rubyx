require 'rbconfig'
require_relative 'rubyx/version'
require_relative 'rubyx/error'

# Load the native extension
begin
  ruby_version = RUBY_VERSION.match(/\d+\.\d+/)[0]
  require "rubyx/#{ruby_version}/rubyx"
rescue LoadError
  begin
    require 'rubyx/rubyx'
  rescue LoadError
    # dev
    dev_root = File.expand_path('..', __dir__)
    unless File.exist?(File.join(dev_root, 'Cargo.toml'))
      raise LoadError,
            "Could not load rubyx native extension. Install the rubyx-py gem."
    end

    lib_ext = case RbConfig::CONFIG['host_os']
              when /darwin/ then 'dylib'
              when /linux/ then 'so'
              when /mingw|mswin/ then 'dll'
              else 'so'
              end
    bundle_ext = RbConfig::CONFIG['host_os'] =~ /darwin/ ? 'bundle' : lib_ext

    lib_path = File.join(dev_root, "target/release/librubyx.#{lib_ext}")
    bundle_path = File.join(dev_root, "target/release/rubyx.#{bundle_ext}")

    unless File.exist?(lib_path)
      raise LoadError,
            "Native extension not built. Run: cargo build --release"
    end

    if !File.exist?(bundle_path) || File.mtime(lib_path) > File.mtime(bundle_path)
      require 'fileutils'
      FileUtils.cp(lib_path, bundle_path)
    end
    require bundle_path
  end
end

require_relative 'rubyx/uv'
require_relative 'rubyx/railtie' if defined?(::Rails::Railtie)

module Rubyx
  # Import a Python module by name.
  #
  # @param module_name [String] Python module name (e.g., "os", "numpy", "my_module.sub")
  # @return [RubyxObject] Wrapped Python module
  # @raise [InvalidModuleNameError] if the name contains invalid characters
  def self.import(module_name)
    name = module_name.to_s
    unless name.match?(VALID_MODULE_NAME_PATTERN)
      raise InvalidModuleNameError,
            "Invalid Python module name: '#{name}'. " \
              "Module names must contain only alphanumeric characters, underscores, and dots."
    end
    _import(name)
  end

  # Evaluate Python code and return the result.
  #
  # @param code [String] Python code to evaluate
  # @return [Object] The result converted to a Ruby value
  class << self
    public define_method(:eval) { |code| Rubyx._eval(code.to_s) }
  end

  # Convenience method: setup Python environment via uv and initialize.
  #
  # @param pyproject_toml [String] Content of pyproject.toml
  # @param options [Hash] Options passed to Uv.setup and Uv.init
  # @return [Hash] Resolved paths from Uv.init
  def self.uv_init(pyproject_toml, **options)
    setup_keys = %i[force uv_version project_dir uv_args uv_path]
    init_keys = %i[uv_version project_dir]

    Uv.setup(pyproject_toml, **options.slice(*setup_keys))
    Uv.init(pyproject_toml, **options.slice(*init_keys))
  end
end
