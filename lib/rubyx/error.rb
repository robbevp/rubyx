module Rubyx
  VALID_MODULE_NAME_PATTERN = /\A[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)*\z/

  class Error < StandardError; end

  class PythonError < Error; end

  class ImportError < PythonError; end

  class InvalidModuleNameError < Error; end

  module Uv
    class Error < Rubyx::Error; end

    class SetupError < Error; end

    class InitError < Error; end
  end

end