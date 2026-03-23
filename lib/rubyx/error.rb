module Rubyx
  VALID_MODULE_NAME_PATTERN = /\A[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)*\z/

  class Error < StandardError; end

  class PythonError < Error; end

  class ImportError < PythonError; end

  class InvalidModuleNameError < Error; end

  class KeyError < Error; end

  class IndexError < Error; end

  class ValueError < Error; end

  class AttributeError < Error; end

  class TypeError < Error; end

  module Uv
    class Error < Rubyx::Error; end

    class SetupError < Error; end

    class InitError < Error; end
  end

end