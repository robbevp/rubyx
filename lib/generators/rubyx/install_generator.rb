module Rubyx
  module Generators
    class InstallGenerator < ::Rails::Generators::Base
      source_root File.expand_path('templates', __dir__)

      def create_pyproject
        copy_file 'pyproject.toml', 'pyproject.toml'
      end

      def create_initializer
        copy_file 'rubyx_initializer.rb', 'config/initializers/rubyx.rb'
      end

      def create_python_directory
        empty_directory 'app/python'
        copy_file 'example.py', 'app/python/example.py'
      end

      def add_gitignore
        append_to_file '.gitignore', "\n# Python (managed by rubyx-py)\n.venv/\n" if File.exist?('.gitignore')
      end
    end
  end
end