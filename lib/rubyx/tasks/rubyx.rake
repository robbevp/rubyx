namespace :rubyx do
  desc 'Initialize Python environment (downloads uv and Python if needed)'
  task init: :environment do
    Rubyx::Rails.init!
    puts '[Rubyx] Python environment initialized successfully.'
  end

  desc 'Check Python environment health'
  task check: :environment do
    puts 'Checking Python environment...'
    puts

    # Check uv
    system_uv = `which uv 2>/dev/null`.strip
    uv_available = !system_uv.empty? && File.exist?(system_uv)
    puts "uv available: #{uv_available ? "Yes (#{system_uv})" : 'No (will auto-download)'}"

    begin
      Rubyx::Rails.ensure_initialized!
      puts 'Python initialized: Yes'
    rescue => e
      puts "Python initialized: No (#{e.message})"
      puts
      puts 'Run `rake rubyx:init` to initialize.'
      exit 1
    end

    begin
      Rubyx.eval('1 + 1')
      puts 'Basic eval: OK'
    rescue => e
      puts "Basic eval: FAILED (#{e.message})"
      exit 1
    end

    begin
      gen = Rubyx.eval("import sys\niter([sys.version.split()[0]])")
      version = Rubyx.stream(gen).first
      puts "Import sys: OK (Python #{version})"
    rescue => e
      puts "Import sys: FAILED (#{e.message})"
      exit 1
    end

    puts
    puts 'All checks passed!'
  end

  desc 'Show Rubyx configuration and status'
  task status: :environment do
    config = Rubyx::Rails.configuration

    puts 'Rubyx Status'
    puts '=' * 40

    puts "Initialized: #{Rubyx::Rails.initialized?}"
    puts

    puts 'Configuration:'
    puts "  pyproject_path:    #{config.pyproject_path || '(not set)'}"
    puts "  pyproject_content: #{config.pyproject_content ? '(inline, %d bytes)' % config.pyproject_content.length : '(not set)'}"
    puts "  auto_init:         #{config.auto_init}"
    puts "  force_reinit:      #{config.force_reinit}"
    puts "  uv_version:        #{config.uv_version}"
    puts "  debug:             #{config.debug}"
    puts "  python_paths:      #{config.python_paths.inspect}"
    puts "  uv_path:           #{config.uv_path || '(auto-download)'}"
    puts "  uv_args:           #{config.uv_args.inspect}"
    puts

    if config.pyproject_path
      exists = File.exist?(config.pyproject_path.to_s)
      puts "pyproject.toml exists: #{exists}"
    end

    if config.pyproject_path
      venv_dir = File.join(File.dirname(config.pyproject_path.to_s), '.venv')
      puts ".venv exists: #{Dir.exist?(venv_dir)}"
    end

    system_uv = `which uv 2>/dev/null`.strip
    puts "System uv: #{!system_uv.empty? ? system_uv : '(not found)'}"
  end

  desc 'List installed Python packages'
  task packages: :environment do
    Rubyx::Rails.ensure_initialized!

    gen = Rubyx.eval(<<~PY)
      import pkg_resources
      packages = sorted([f"{d.project_name}=={d.version}" for d in pkg_resources.working_set])
      iter(packages)
    PY

    puts 'Installed Python packages:'
    Rubyx.stream(gen).each { |pkg| puts "  #{pkg}" }
  rescue => e
    begin
      gen = Rubyx.eval(<<~PY)
        from importlib.metadata import distributions
        packages = sorted([f"{d.metadata['Name']}=={d.metadata['Version']}" for d in distributions()])
        iter(packages)
      PY

      puts 'Installed Python packages:'
      Rubyx.stream(gen).each { |pkg| puts "  #{pkg}" }
    rescue => e2
      puts "Could not list packages: #{e2.message}"
    end
  end

  desc 'Clear the Rubyx cache (re-download uv + Python on next init)'
  task clear_cache: :environment do
    cache_dir = File.join(
      ENV.fetch('XDG_CACHE_HOME', File.join(Dir.home, '.cache')),
      'rubyx'
    )

    if Dir.exist?(cache_dir)
      require 'fileutils'
      FileUtils.rm_rf(cache_dir)
      puts "[Rubyx] Cache cleared: #{cache_dir}"
    else
      puts '[Rubyx] No cache directory found.'
    end
  end
end
