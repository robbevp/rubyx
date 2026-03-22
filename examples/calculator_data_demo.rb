#!/usr/bin/env ruby
# frozen_string_literal: true

# Demo: Using Python modules from Ruby via rubyx-py
#
# Prerequisites:
#   1. Build the extension: cargo build --release
#   2. Run: ruby examples/calculator_data_demo.rb

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
require 'rubyx'

# Initialize Python via UV
# Use system uv if available, otherwise auto-download
python_dir = File.expand_path('python', __dir__)
pyproject = File.read(File.join(python_dir, 'pyproject.toml'))

system_uv = `which uv 2>/dev/null`.strip
uv_opts = {}
uv_opts[:uv_path] = system_uv if !system_uv.empty? && File.exist?(system_uv)

Rubyx.uv_init(pyproject, project_dir: python_dir, **uv_opts)

# --- Calculator module ---

puts "=== Calculator Module ==="
puts

ctx = Rubyx.context
ctx.eval('import calculator')
ctx.eval('import data_utils')

result = Rubyx.stream(ctx.eval('iter([calculator.add(3, 4)])')).first
puts "3 + 4 = #{result}"

result = Rubyx.stream(ctx.eval('iter([calculator.multiply(6, 7)])')).first
puts "6 x 7 = #{result}"

result = Rubyx.stream(ctx.eval('iter([calculator.divide(10, 3)])')).first
puts "10 / 3 = #{result}"

puts
puts "Fibonacci(10):"
gen = ctx.eval('iter(calculator.fibonacci(10))')
fibs = Rubyx.stream(gen).to_a
puts fibs.join(', ')

puts

# --- Data Utils module ---

puts "=== Data Utils Module ==="
puts

gen = ctx.eval(<<~PY)
  import json
  freq = data_utils.word_frequency('Ruby and Python work great together. Ruby calls Python seamlessly.', 3)
  iter([json.dumps(freq)])
PY
freq_json = Rubyx.stream(gen).first
puts "Top 3 words: #{freq_json}"

gen = ctx.eval("iter([data_utils.clean_text('  Hello   WORLD!  ')])")
cleaned = Rubyx.stream(gen).first
puts "Cleaned text: '#{cleaned}'"

gen = ctx.eval("iter([data_utils.to_json({'language': 'Ruby', 'bridge': 'rubyx-py'})])")
json_str = Rubyx.stream(gen).first
puts "JSON: #{json_str}"

puts
puts "Done!"
