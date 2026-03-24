# frozen_string_literal: true

require "test_helper"

class DemoControllerTest < ActionDispatch::IntegrationTest
  # ===========================================================================
  # Sync — functions & classes
  # ===========================================================================

  test "text_analysis returns word count and summary" do
    get "/demo/text_analysis", params: { text: "hello world hello ruby python" }
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal 5, json["summary"]["word_count"]
    assert_equal 4, json["summary"]["unique_words"] # hello(x2) world ruby python
    assert json["most_common"].is_a?(Array)
  end

  test "text_analysis works with default text" do
    get "/demo/text_analysis"
    assert_response :success

    json = JSON.parse(response.body)
    assert json["summary"]["word_count"] > 0
  end

  test "slugify converts text to slug" do
    get "/demo/slugify", params: { text: "Hello World! This is Great" }
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal "hello-world-this-is-great", json["slug"]
  end

  test "slugify works with default text" do
    get "/demo/slugify"
    assert_response :success

    json = JSON.parse(response.body)
    assert json["slug"].is_a?(String)
  end

  test "extract_emails finds emails in text" do
    get "/demo/extract_emails", params: { text: "contact alice@example.com or bob@test.org" }
    assert_response :success

    json = JSON.parse(response.body)
    assert_includes json["emails"], "alice@example.com"
    assert_includes json["emails"], "bob@test.org"
  end

  test "extract_emails returns empty array when no emails" do
    get "/demo/extract_emails", params: { text: "no emails here" }
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal [], json["emails"]
  end

  # ===========================================================================
  # Sync — eval with globals
  # ===========================================================================

  test "eval_globals computes expression with Ruby values" do
    post "/demo/eval_globals", params: { x: 3, y: 4 }
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal 25, json["result"] # 3**2 + 4**2 = 9 + 16
  end

  test "eval_globals works with defaults" do
    post "/demo/eval_globals"
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal 0, json["result"] # 0**2 + 0**2
  end

  # ===========================================================================
  # Sync streaming — data transform
  # ===========================================================================

  test "stream_csv parses CSV into records" do
    get "/demo/stream_csv"
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal 3, json["records"].length
    assert_equal "Alice", json["records"][0]["name"]
    assert_equal "30", json["records"][0]["age"]
  end

  test "stream_transform applies transform to each record" do
    get "/demo/stream_transform"
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal 5, json["results"].length
    assert_equal 20, json["results"][0]["doubled"] # value=10, doubled=20
  end

  # ===========================================================================
  # Sync streaming — LLM-style SSE
  # ===========================================================================

  test "llm_stream returns SSE content type" do
    get "/demo/llm_stream", params: { prompt: "test" }
    assert_response :success
    assert_match %r{text/event-stream}, response.content_type
  end

  test "llm_stream body contains tokens" do
    get "/demo/llm_stream", params: { prompt: "test" }
    assert_response :success
    assert response.body.include?("data: ")
    assert response.body.include?("[DONE]")
  end

  test "llm_stream every non-empty line has data: prefix" do
    get "/demo/llm_stream", params: { prompt: "test" }
    assert_response :success

    lines = response.body.lines.map(&:strip)
    non_empty = lines.reject(&:empty?)
    non_empty.each do |line|
      assert line.start_with?("data: "), "SSE line missing 'data: ' prefix: #{line.inspect}"
    end
  end

  # ===========================================================================
  # Async — blocking await
  # ===========================================================================

  test "async_fetch fetches a URL" do
    get "/demo/async_fetch", params: { url: "https://httpbin.org/json" }
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal "https://httpbin.org/json", json["url"]
    assert json["response_length"].is_a?(Integer)
    assert json["response_length"] > 0
  end

  test "async_delayed returns awaited result" do
    get "/demo/async_delayed"
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal "Hello from async!", json["result"]
  end

  # ===========================================================================
  # Async — non-blocking future
  # ===========================================================================

  test "async_future runs Python and Ruby concurrently" do
    get "/demo/async_future"
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal 42, json["python_result"]
    assert_equal 500500, json["ruby_work"]
  end

  # ===========================================================================
  # ML — predictor
  # ===========================================================================

  test "predict returns linear regression prediction" do
    post "/demo/predict",
      params: { x_data: [1, 2, 3, 4, 5], y_data: [2, 4, 6, 8, 10], predict_x: 6 }.to_json,
      headers: { "Content-Type" => "application/json" }
    assert_response :success

    json = JSON.parse(response.body)
    assert_in_delta 12.0, json["prediction"], 0.01
    assert_in_delta 1.0, json["summary"]["r_squared"], 0.01
  end

  test "predict works with defaults" do
    post "/demo/predict"
    assert_response :success

    json = JSON.parse(response.body)
    assert json["prediction"].is_a?(Numeric)
    assert json["summary"]["slope"].is_a?(Numeric)
  end

  test "cluster returns centroids and assignments" do
    post "/demo/cluster",
      params: { data: [[1, 1], [1, 2], [10, 10], [10, 11]], k: 2 }.to_json,
      headers: { "Content-Type" => "application/json" }
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal 2, json["centroids"].length
    assert_equal 4, json["assignments"].length
  end

  # ===========================================================================
  # Context — stateful sessions
  # ===========================================================================

  test "context_demo accumulates state" do
    get "/demo/context"
    assert_response :success

    json = JSON.parse(response.body)
    assert_equal 3, json["values"].length
    assert_in_delta Math::PI, json["values"][0], 0.0001
    assert json["sum"] > 0
  end

  test "context_with_globals computes with injected values" do
    post "/demo/context_with_globals", params: { radius: 1 }
    assert_response :success

    json = JSON.parse(response.body)
    assert_in_delta Math::PI, json["area"], 0.0001         # pi * 1**2
    assert_in_delta 2 * Math::PI, json["circumference"], 0.0001 # 2 * pi * 1
  end

  test "context_with_globals works with default radius" do
    post "/demo/context_with_globals"
    assert_response :success

    json = JSON.parse(response.body)
    assert_in_delta Math::PI * 25, json["area"], 0.01 # pi * 5**2
  end
end
