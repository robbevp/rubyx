require_relative 'rubyx/version'

begin
  ruby_version = RUBY_VERSION.match(/\d+\.\d+/)[0]
  require "rubyx/#{ruby_version}/rubyx"
rescue LoadError
  require 'rubyx/rubyx'
end

module Rubyx
  class Error < StandardError; end
  class PythonError < Error; end
  class ImportError < PythonError; end
end
