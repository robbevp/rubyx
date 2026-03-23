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

require_relative 'rubyx/context'
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
  # @param globals [Hash] Ruby values to inject as Python globals
  # @return [RubyxObject] The result as a wrapped Python object
  # @example
  #   Rubyx.eval("x + y", x: 10, y: 20)
  class << self
    public define_method(:eval) { |code, **globals|
      if globals.empty?
        Rubyx._eval(code.to_s)
      else
        Rubyx._eval_with_globals(code.to_s, globals)
      end
    }
  end

  # Run a Python coroutine with asyncio.run() (blocking).
  # Accepts either a RubyxObject (coroutine) or a code string with globals.
  #
  # @param code_or_coroutine [String, RubyxObject] Python code or coroutine object
  # @param globals [Hash] Ruby values to inject as Python globals (only with code string)
  # @return [RubyxObject] The awaited result
  # @example
  #   Rubyx.await("fetch(url)", url: "https://example.com")
  def self.await(code_or_coroutine, **globals)
    if code_or_coroutine.is_a?(String)
      if globals.empty?
        _await_with_globals(code_or_coroutine, {})
      else
        _await_with_globals(code_or_coroutine, globals)
      end
    else
      raise ArgumentError, "cannot pass globals with a coroutine object" unless globals.empty?
      _await(code_or_coroutine)
    end
  end

  # Run a Python coroutine on a background thread (non-blocking).
  # Accepts either a RubyxObject (coroutine) or a code string with globals.
  #
  # @param code_or_coroutine [String, RubyxObject] Python code or coroutine object
  # @param globals [Hash] Ruby values to inject as Python globals (only with code string)
  # @return [Rubyx::Future] A future that resolves to the result
  # @example
  #   future = Rubyx.async_await("fetch(url)", url: "https://example.com")
  #   future.value
  def self.async_await(code_or_coroutine, **globals)
    if code_or_coroutine.is_a?(String)
      if globals.empty?
        _async_await_with_globals(code_or_coroutine, {})
      else
        _async_await_with_globals(code_or_coroutine, globals)
      end
    else
      raise ArgumentError, "cannot pass globals with a coroutine object" unless globals.empty?
      _async_await(code_or_coroutine)
    end
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
