Rubyx::Rails.configure do |config|
  # Path to your Python project's pyproject.toml
  config.pyproject_path = Rails.root.join('pyproject.toml')

  # Auto-initialize Python when Rails boots
  # Set to false for forking servers (Puma workers) — use on_worker_boot instead
  config.auto_init = true

  # Directories to add to Python's sys.path (makes .py files importable)
  config.python_paths = [Rails.root.join('app/python').to_s]

  # Use system uv instead of auto-downloading (optional)
  config.uv_path = `which uv`.strip

  # Extra arguments for uv sync (optional)
  config.uv_args = %w[--extra ai]
end