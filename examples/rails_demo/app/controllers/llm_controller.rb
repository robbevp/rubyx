# frozen_string_literal: true

# LLM text generation via Hugging Face Transformers.
# Requires: uv sync --extra ml
class LlmController < ApplicationController
  include ActionController::Live

  # POST /llm/load?model=Qwen/Qwen2.5-0.5B-Instruct&device=cpu
  def load
    llm = Rubyx.import('services.llm')
    model_name = params[:model] || "Qwen/Qwen2.5-0.5B-Instruct"
    device = params[:device] || "cpu"

    result = llm.load_model(model_name, device)
    render json: { status: result.to_ruby }
  end

  # POST /llm/generate  body: { "prompt": "What is Ruby?", "max_tokens": 100 }
  def generate
    llm = Rubyx.import('services.llm')
    prompt = params[:prompt] || "Hello"
    max_tokens = (params[:max_tokens] || 256).to_i
    temperature = (params[:temperature] || 0.7).to_f

    result = llm.generate(prompt, max_tokens, temperature)
    render json: { prompt: prompt, response: result.to_ruby }
  end

  # GET /llm/stream?prompt=Tell+me+about+Ruby
  def stream
    llm = Rubyx.import('services.llm')
    prompt = params[:prompt] || "Hello"
    max_tokens = (params[:max_tokens] || 256).to_i

    response.headers['Content-Type'] = 'text/event-stream'
    response.headers['Cache-Control'] = 'no-cache'
    response.headers['X-Accel-Buffering'] = 'no'

    gen = llm.stream_generate(prompt, max_tokens)
    Rubyx.stream(gen).each do |token|
      response.stream.write("data: #{token}\n\n")
    end
    response.stream.write("data: [DONE]\n\n")
  rescue ActionController::Live::ClientDisconnected
    # Client disconnected
  ensure
    response.stream.close
  end
end
