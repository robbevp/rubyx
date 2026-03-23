# frozen_string_literal: true

# LLM text generation via Hugging Face Transformers.
# Model is loaded once at boot in config/initializers/rubyx.rb
class LlmController < ApplicationController
  include ActionController::Live

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
