#!/usr/bin/env ruby
# frozen_string_literal: true

# Demo: Using Python modules from Ruby via rubyx-py
#
# Prerequisites:
#   1. Build the extension: cargo build --release
#   2. Run: ruby examples/calculator_demo.rb

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
require 'rubyx'

# Auto-detect Python from .venv or system
project_root = File.expand_path('..', __dir__)
python3 = [
  File.join(project_root, '.venv', 'bin', 'python3'),
  `which python3 2>/dev/null`.strip,
].find { |p| !p.empty? && File.exist?(p) }

abort 'Python 3 not found' unless python3

python_info = `#{python3} -c "
import sysconfig, sys, os, platform
libdir = sysconfig.get_config_var('LIBDIR')
ver = f'{sys.version_info.major}.{sys.version_info.minor}'
ext = 'dylib' if platform.system() == 'Darwin' else 'so'
print(os.path.join(libdir, f'libpython{ver}.{ext}'))
print(sys.base_prefix)
print(sys.executable)
" 2>/dev/null`.strip.split("\n")

abort 'Could not detect Python paths' unless python_info.length == 3

python_dl, python_home, python_exe = python_info
sys_paths = `#{python3} -c "import site; print('\\n'.join(site.getsitepackages()))" 2>/dev/null`.strip.split("\n").reject(&:empty?)

Rubyx.init(python_dl, python_home, python_exe, sys_paths)

# Inject examples/python/ into sys.path
python_dir = File.expand_path('python', __dir__)
Rubyx.eval("import sys; sys.path.insert(0, '#{python_dir}')")

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
