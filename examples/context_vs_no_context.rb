require_relative "../spec/spec_helper"

STDOUT.sync = true

venv_site = File.expand_path("../.venv/lib/python3.12/site-packages", __dir__)

setup_code = <<~PYTHON
  import warnings
  warnings.filterwarnings("ignore")
  import os
  os.environ["TOKENIZERS_PARALLELISM"] = "false"

  from transformers import AutoModelForCausalLM, AutoTokenizer
  import torch

  model_name = "Qwen/Qwen3.5-0.8B"
  tokenizer = AutoTokenizer.from_pretrained(model_name)
  model = AutoModelForCausalLM.from_pretrained(model_name)
  model.eval()

  def predict(prompt, max_new_tokens=5):
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
PYTHON

prompt = "Hello world"
n_calls = 3

# Without persistent context: reload model every call
puts "=== WITHOUT persistent context (Rubyx.eval) ==="
puts "Each call must reload the model from scratch.\n\n"

no_ctx_times = []
n_calls.times do |i|
  start = Time.now

  # Every Rubyx.eval gets fresh globals -- model must be reloaded
  code = <<~RUBY_PY
    import sys
    sys.path.insert(0, '#{venv_site}')
    #{setup_code}
    iter([predict('#{prompt}', max_new_tokens=5)])
  RUBY_PY
  result = Rubyx.eval(code)
  text = Rubyx.stream(result).to_a.first

  elapsed = Time.now - start
  no_ctx_times << elapsed
  puts "  Call #{i + 1}: #{text.inspect} (%.1fs — includes model reload)" % elapsed
end

puts

# With persistent context: load model once
puts "=== WITH persistent context (Rubyx::Context) ==="
puts "Model is loaded once, reused for all calls.\n\n"

ctx = Rubyx.context
ctx.eval("import sys; sys.path.insert(0, '#{venv_site}')")

load_start = Time.now
ctx.eval(setup_code)
load_time = Time.now - load_start
puts "  Model loaded: %.1fs (one time cost)\n\n" % load_time

ctx_times = []
n_calls.times do |i|
  start = Time.now

  result = ctx.eval("iter([predict('#{prompt}', max_new_tokens=5)])")
  text = Rubyx.stream(result).to_a.first

  elapsed = Time.now - start
  ctx_times << elapsed
  puts "  Call #{i + 1}: #{text.inspect} (%.1fs — inference only)" % elapsed
end

puts
puts "=== Comparison ==="
puts
avg_no_ctx = no_ctx_times.sum / no_ctx_times.size
avg_ctx = ctx_times.sum / ctx_times.size
puts "Without context (avg): %.1fs per call (reload model every time)" % avg_no_ctx
puts "With context (avg):    %.1fs per call (inference only)" % avg_ctx
puts "Model load (one time): %.1fs" % load_time
puts
puts "Speedup per call: %.1fx faster" % (avg_no_ctx / avg_ctx)
puts "Over #{n_calls} calls: %.0fs saved" % ((avg_no_ctx - avg_ctx) * n_calls)
