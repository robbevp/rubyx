require_relative "../spec/spec_helper"

STDOUT.sync = true

puts "=== Eager Loading Example ==="
puts "Creating persistent context..."

ctx = Rubyx.context

venv_site = File.expand_path("../.venv/lib/python3.12/site-packages", __dir__)
ctx.eval("import sys; sys.path.insert(0, '#{venv_site}')")

puts "Loading Qwen3.5-0.8B model and tokenizer (first time may download ~1.5GB)..."
load_start = Time.now

ctx.eval(<<~PYTHON)
  import warnings
  warnings.filterwarnings("ignore")
  import os
  os.environ["TOKENIZERS_PARALLELISM"] = "false"

  from transformers import AutoModelForCausalLM, AutoTokenizer
  import torch

  model_name = "Qwen/Qwen3.5-0.8B"
  tokenizer = AutoTokenizer.from_pretrained(model_name)
  model = AutoModelForCausalLM.from_pretrained(model_name, torch_dtype=torch.float32)
  model.eval()

  def predict(prompt, max_new_tokens=30):
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

load_time = Time.now - load_start
puts "Model loaded in %.1fs" % load_time
puts

# Run inference multiple times -- no model reload!
prompts = [
  "The capital of France is",
  "def fibonacci(n):",
  "The meaning of life is",
]

prompts.each do |prompt|
  puts "--- Prompt: #{prompt.inspect} ---"
  start = Time.now

  escaped = prompt.gsub("'", "\\\\'")
  result = ctx.eval("iter([predict('#{escaped}')])")
  text = Rubyx.stream(result).to_a.first

  inference_time = Time.now - start
  puts "  Result: #{text}"
  puts "  Time: %.2fs" % inference_time
  puts
end

puts "=== Summary ==="
puts "Model loaded once: %.1fs" % load_time
puts "Each inference call reused the loaded model -- no reload needed."
