require 'digest'
require 'fileutils'
require 'open-uri'
require 'stringio'
require 'rubygems/package'
require 'zlib'

module Rubyx
  module Uv
    DEFAULT_UV_VERSION = '0.10.2'.freeze

    class << self
      # Download uv (if needed), write pyproject.toml, and run `uv sync`.
      #
      # @param pyproject_toml [String] Content of pyproject.toml
      # @param force [Boolean] Force re-setup even if .venv exists
      # @param uv_version [String] Version of uv to download
      # @param project_dir [String, Symbol, nil] Where to create the project
      #   - nil: use Dir.pwd
      #   - :cache: use a hash-based cache directory
      #   - String: use the specified path
      # @param uv_args [Array<String>] Extra arguments to pass to `uv sync`
      # @param uv_path [String, nil] Path to an existing uv binary. When set,
      #   auto-download is skipped entirely.
      # @return [String] The resolved project directory path
      def setup(pyproject_toml, force: false, uv_version: DEFAULT_UV_VERSION,
                project_dir: nil, uv_args: [], uv_path: nil)
        proj_dir = resolve_project_dir(pyproject_toml, uv_version, project_dir)

        venv_dir = File.join(proj_dir, '.venv')
        pyproject_path = File.join(proj_dir, 'pyproject.toml')

        needs_setup = force || !Dir.exist?(venv_dir)

        if !needs_setup && File.exist?(pyproject_path)
          needs_setup = File.read(pyproject_path).strip != pyproject_toml.strip
        end

        if needs_setup
          FileUtils.rm_rf(venv_dir) if force
          FileUtils.mkdir_p(proj_dir)
          File.write(pyproject_path, pyproject_toml)

          success = run_uv!(
            ['sync', '--managed-python', '--no-config', '--project', proj_dir, *uv_args],
            chdir: proj_dir,
            env: { 'UV_PYTHON_INSTALL_DIR' => python_install_dir(uv_version) },
            uv_version: uv_version,
            uv_path: uv_path
          )

          unless success
            FileUtils.rm_rf(venv_dir)
            raise SetupError, 'uv sync failed to setup Python environment'
          end
        end

        proj_dir
      end

      # Parse pyvenv.cfg, resolve platform paths, and call Rubyx.init.
      #
      # @param pyproject_toml [String] Content of pyproject.toml (used to resolve project_dir)
      # @param uv_version [String] Version of uv (used to resolve project_dir)
      # @param project_dir [String, Symbol, nil] Same as setup
      # @return [Hash] Resolved paths (:root_dir, :project_dir, :python_dl, etc.)
      def init(pyproject_toml, uv_version: DEFAULT_UV_VERSION, project_dir: nil)
        proj_dir = resolve_project_dir(pyproject_toml, uv_version, project_dir)

        venv_dir = File.join(proj_dir, '.venv')
        raise InitError, "Not set up. Call Rubyx::Uv.setup first." unless Dir.exist?(venv_dir)

        cfg_path = File.join(venv_dir, 'pyvenv.cfg')
        raise InitError, "pyvenv.cfg not found at #{cfg_path}" unless File.exist?(cfg_path)

        pyvenv_cfg = File.read(cfg_path)
        home_line = pyvenv_cfg.lines.find { |l| l.start_with?('home = ') }
        raise InitError, "Could not find 'home' in pyvenv.cfg" unless home_line

        home_path = home_line.sub('home = ', '').strip
        root_dir = File.dirname(home_path) # Parent of bin/

        paths = platform_paths(root_dir, proj_dir)
        validate_paths!(paths)

        sys_paths = []
        sys_paths << paths[:venv_packages] if paths[:venv_packages]
        sys_paths << proj_dir if project_dir && project_dir != :cache

        # Call the Rust init
        Rubyx.init(
          paths[:python_dl],
          paths[:python_home],
          paths[:python_exe],
          sys_paths
        )

        { root_dir: root_dir, project_dir: proj_dir, **paths }
      end

      private

      # Download the uv binary from GitHub releases.
      def download_uv!(uv_version)
        archive_type, archive_name = archive_name_for_platform
        url = "https://github.com/astral-sh/uv/releases/download/#{uv_version}/#{archive_name}"

        warn "Downloading uv #{uv_version}..."

        archive_data = URI.open(url, 'rb', &:read)
        uv_binary = extract_uv(archive_type, archive_data)

        path = default_uv_path(uv_version)
        FileUtils.mkdir_p(File.dirname(path))
        File.binwrite(path, uv_binary)
        File.chmod(0o755, path)

        path
      end

      def archive_name_for_platform
        case RUBY_PLATFORM
        when /arm64.*darwin/, /aarch64.*darwin/
          [:tar_gz, 'uv-aarch64-apple-darwin.tar.gz']
        when /x86_64.*darwin/, /darwin/
          [:tar_gz, 'uv-x86_64-apple-darwin.tar.gz']
        when /aarch64.*linux/
          [:tar_gz, 'uv-aarch64-unknown-linux-gnu.tar.gz']
        when /x86_64.*linux/, /linux/
          [:tar_gz, 'uv-x86_64-unknown-linux-gnu.tar.gz']
        when /mingw/, /mswin/, /cygwin/
          [:zip, 'uv-x86_64-pc-windows-msvc.zip']
        else
          raise SetupError, "Unsupported platform: #{RUBY_PLATFORM}"
        end
      end

      def extract_uv(type, data)
        case type
        when :tar_gz
          io = StringIO.new(data)
          gzip = Zlib::GzipReader.new(io)
          Gem::Package::TarReader.new(gzip) do |tar|
            tar.each do |entry|
              return entry.read if File.basename(entry.full_name) == 'uv'
            end
          end
          raise SetupError, 'uv binary not found in archive'
        when :zip
          require 'zip'
          Zip::File.open_buffer(data) do |zip|
            zip.each do |entry|
              return entry.get_input_stream.read if File.basename(entry.name, '.*') == 'uv'
            end
          end
          raise SetupError, 'uv binary not found in archive'
        end
      end

      # Run a uv command.
      #
      # @param uv_path [String, nil] Custom uv binary path. When nil, uses
      #   the auto-downloaded binary (downloading if needed).
      def run_uv!(args, chdir:, env:, uv_version:, uv_path: nil)
        path = if uv_path
                 raise SetupError, "uv not found at #{uv_path}" unless File.exist?(uv_path)
                 uv_path
               else
                 default = default_uv_path(uv_version)
                 download_uv!(uv_version) unless File.exist?(default)
                 default
               end

        require 'open3'
        full_env = env.transform_keys(&:to_s)
        success = nil
        Dir.chdir(chdir) do
          Open3.popen2e(full_env, path, *args) do |stdin, stdout_err, wait_thr|
            stdin.close
            stdout_err.each_line { |line| $stderr.print line }
            success = wait_thr.value.success?
          end
        end

        success
      end

      # Resolve platform-specific paths for libpython, home, exe, and site-packages.
      def platform_paths(root_dir, project_dir)
        case RUBY_PLATFORM
        when /darwin/
          {
            python_dl: find_lib(root_dir, 'lib/libpython3.*.dylib'),
            python_home: root_dir,
            python_exe: File.join(project_dir, '.venv/bin/python'),
            venv_packages: find_lib(project_dir, '.venv/lib/python3.*/site-packages'),
          }
        when /linux/
          {
            python_dl: find_lib(root_dir, 'lib/libpython3.*.so'),
            python_home: root_dir,
            python_exe: File.join(project_dir, '.venv/bin/python'),
            venv_packages: find_lib(project_dir, '.venv/lib/python3.*/site-packages'),
          }
        when /mingw/, /mswin/, /cygwin/
          {
            python_dl: find_lib(root_dir, 'python3*.dll'),
            python_home: root_dir,
            python_exe: File.join(project_dir, '.venv/Scripts/python.exe'),
            venv_packages: File.join(project_dir, '.venv/Lib/site-packages'),
          }
        else
          raise InitError, "Unsupported platform: #{RUBY_PLATFORM}"
        end
      end

      def find_lib(base_dir, pattern)
        matches = Dir.glob(File.join(base_dir, pattern))
        matches.min_by(&:length)
      end

      def validate_paths!(paths)
        paths.each do |key, path|
          next if path.nil? && key == :venv_packages
          raise InitError, "Path not found: #{key} (#{path})" unless path && File.exist?(path)
        end
      end

      # Determine where the project directory should be.
      def resolve_project_dir(pyproject_toml, uv_version, project_dir)
        case project_dir
        when nil
          Dir.pwd
        when :cache
          cache_id = Digest::MD5.hexdigest(pyproject_toml)
          File.join(cache_dir(uv_version), 'projects', cache_id)
        else
          File.expand_path(project_dir)
        end
      end

      # Path to the auto-downloaded uv binary.
      def default_uv_path(uv_version)
        File.join(cache_dir(uv_version), 'bin', 'uv')
      end

      # Root cache directory for this rubyx + uv version combination.
      def cache_dir(uv_version)
        File.join(
          ENV.fetch('XDG_CACHE_HOME', File.join(Dir.home, '.cache')),
          'rubyx', Rubyx::VERSION, 'uv', uv_version
        )
      end

      # Directory where uv installs managed Python distributions.
      def python_install_dir(uv_version)
        File.join(cache_dir(uv_version), 'python')
      end
    end
  end
end
