module Rubyx
  module Rails
    class Error < Rubyx::Error; end

    class Configuration
      attr_accessor :pyproject_path, :pyproject_content, :auto_init,
                    :force_reinit, :uv_version, :debug, :python_paths,
                    :uv_path, :uv_args

      def initialize
        @pyproject_path = nil
        @pyproject_content = nil
        @auto_init = false
        @force_reinit = false
        @uv_version = Rubyx::Uv::DEFAULT_UV_VERSION
        @debug = false
        @python_paths = []
        @uv_path = nil
        @uv_args = []
      end
    end

    class << self
      def configuration
        @configuration ||= Configuration.new
      end

      def configure
        yield configuration
      end

      def init!
        return if initialized?

        config = configuration

        pyproject_toml = resolve_pyproject(config)
        project_dir = resolve_project_dir(config)

        options = {
          force: config.force_reinit,
          uv_version: config.uv_version,
          project_dir: project_dir,
          uv_args: config.uv_args,
        }
        options[:uv_path] = config.uv_path if config.uv_path

        Rubyx.uv_init(pyproject_toml, **options)

        inject_python_paths(config.python_paths)

        @initialized = true

        if config.debug
          ::Rails.logger.info "[Rubyx] Python initialized (project_dir: #{project_dir})"
        end
      rescue => e
        @initialized = false
        ::Rails.logger.error "[Rubyx] Failed to initialize Python: #{e.message}" if defined?(::Rails.logger)
        raise
      end

      def ensure_initialized!
        return if initialized?

        init!
      end

      def initialized?
        @initialized == true
      end

      private

      def resolve_pyproject(config)
        if config.pyproject_path && File.exist?(config.pyproject_path.to_s)
          File.read(config.pyproject_path.to_s)
        elsif config.pyproject_content
          config.pyproject_content
        else
          raise Error, "No pyproject.toml configured. Set pyproject_path or pyproject_content in config/initializers/rubyx.rb"
        end
      end

      def resolve_project_dir(config)
        if config.pyproject_path
          File.dirname(config.pyproject_path.to_s)
        elsif defined?(::Rails) && ::Rails.respond_to?(:root)
          ::Rails.root.to_s
        else
          Dir.pwd
        end
      end

      def inject_python_paths(paths)
        return if paths.nil? || paths.empty?

        paths.each do |path|
          expanded = File.expand_path(path)
          Rubyx.eval("import sys; sys.path.insert(0, '#{expanded}')") if Dir.exist?(expanded)
        end
      end
    end
  end
end
