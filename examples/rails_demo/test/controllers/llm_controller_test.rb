# frozen_string_literal: true

require "test_helper"

class LlmControllerTest < ActionDispatch::IntegrationTest
  # Models are loaded at boot in production/development.
  # In test env, we load once before all tests.
  @@llm_loaded = false
  setup do
    unless @@llm_loaded
      llm = Rubyx.import('services.llm')
      llm.load_model("Qwen/Qwen2.5-0.5B-Instruct")
      @@llm_loaded = true
    end
  end

  # ===========================================================================
  # Generation
  # ===========================================================================

  test "generate returns a response" do
    post "/llm/generate",
      params: { prompt: "What is 1+1?", max_tokens: 20 }.to_json,
      headers: { "Content-Type" => "application/json" }
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal "What is 1+1?", json["prompt"]
    assert json["response"].is_a?(String)
    assert json["response"].length > 0
  end

  test "generate uses defaults" do
    post "/llm/generate"
    assert_response :success

    json = JSON.parse(response.body)
    assert json["response"].is_a?(String)
  end

  # ===========================================================================
  # Streaming
  # ===========================================================================

  test "stream returns SSE content type" do
    get "/llm/stream", params: { prompt: "Say hi", max_tokens: 10 }
    assert_response :success
    assert_match %r{text/event-stream}, response.content_type
  end
end
