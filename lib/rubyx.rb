require_relative 'rubyx/version'
require_relative 'rubyx/error'

begin
  ruby_version = RUBY_VERSION.match(/\d+\.\d+/)[0]
  require "rubyx/#{ruby_version}/rubyx"
rescue LoadError
  require 'rubyx/rubyx'
end

require_relative 'rubyx/uv'

module Rubyx
  # Convenience method: setup Python environment via uv and initialize.
  #
  # @param pyproject_toml [String] Content of pyproject.toml
  # @param options [Hash] Options passed to Uv.setup and Uv.init
  # @return [Hash] Resolved paths from Uv.init
  def self.uv_init(pyproject_toml, **options)
    Uv.setup(pyproject_toml, **options)
    Uv.init(pyproject_toml, **options)
  end
end
