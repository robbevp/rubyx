
require_relative "../spec/spec_helper"

STDOUT.sync = true

puts "=== Lazy Loading Example ==="
puts "Creating persistent context with lazy model loader..."

ctx = Rubyx.context

venv_site = File.expand_path("../.venv/lib/python3.12/site-packages", __dir__)
ctx.eval("import sys; sys.path.insert(0, '#{venv_site}')")

ctx.eval(<<~PYTHON)
  import warnings
  warnings.filterwarnings("ignore")
  import os
  os.environ["TOKENIZERS_PARALLELISM"] = "false"

  from transformers import AutoModelForCausalLM, AutoTokenizer
  import torch
  import time

  _model_cache = {}

  def get_model(name):
      """Load model + tokenizer on first use, cache for reuse."""
      if name not in _model_cache:
          start = time.time()
          tokenizer = AutoTokenizer.from_pretrained(name)
          model = AutoModelForCausalLM.from_pretrained(name)
          model.eval()
          elapsed = time.time() - start
          _model_cache[name] = (model, tokenizer)
      return _model_cache[name]

  def predict(model_name, prompt, max_new_tokens=10):
      model, tokenizer = get_model(model_name)
      inputs = tokenizer(prompt, return_tensors="pt")
      with torch.no_grad():
          output = model.generate(
              **inputs,
              max_new_tokens=max_new_tokens,
              do_sample=False,
              temperature=None,
              top_p=None,
          )
      return tokenizer.decode(output[0], skip_special_tokens=True)

  def loaded_models():
      return list(_model_cache.keys())
PYTHON

puts "Infrastructure ready. No models loaded yet."
puts

model = "Qwen/Qwen3.5-0.8B"

puts "--- First call (triggers lazy load) ---"
start = Time.now
escaped_model = model.gsub("'", "\\\\'")
result = ctx.eval("iter([predict('#{escaped_model}', 'Hello, my name is', max_new_tokens=10)])")
text = Rubyx.stream(result).to_a.first
first_time = Time.now - start
puts "  Result: #{text}"
puts "  Time: %.2fs (includes model loading)" % first_time
puts

puts "--- Second call (cached) ---"
start = Time.now
result = ctx.eval("iter([predict('#{escaped_model}', 'The best programming language is', max_new_tokens=10)])")
text = Rubyx.stream(result).to_a.first
second_time = Time.now - start
puts "  Result: #{text}"
puts "  Time: %.2fs (model already cached)" % second_time
puts

puts "--- Third call (cached) ---"
start = Time.now
result = ctx.eval("iter([predict('#{escaped_model}', 'Ruby is great because', max_new_tokens=10)])")
text = Rubyx.stream(result).to_a.first
third_time = Time.now - start
puts "  Result: #{text}"
puts "  Time: %.2fs (model already cached)" % third_time
puts

result = ctx.eval("iter(loaded_models())")
models = Rubyx.stream(result).to_a
puts "Cached models: #{models.inspect}"
puts
puts "=== Summary ==="
puts "First call (cold):  %.2fs (includes model loading)" % first_time
puts "Second call (warm): %.2fs" % second_time
puts "Third call (warm):  %.2fs" % third_time
if second_time > 0
  puts "Speedup: %.1fx faster after first call" % (first_time / second_time)
end
