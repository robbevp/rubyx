module Rubyx
  class Error < StandardError; end

  class PythonError < Error; end

  class ImportError < PythonError; end

  module Uv
    class Error < Rubyx::Error; end

    class SetupError < Error; end

    class InitError < Error; end
  end
end