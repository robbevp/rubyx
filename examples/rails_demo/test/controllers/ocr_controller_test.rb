# frozen_string_literal: true

require "test_helper"

class OcrControllerTest < ActionDispatch::IntegrationTest
  # Requires:
  #   1. uv sync --extra ai
  #   2. ollama pull glm-ocr

  DOCS_DIR = Rails.root.join("app/python/docs")

  # ===========================================================================
  # File listing (no model needed)
  # ===========================================================================

  test "files lists documents in docs directory" do
    get "/ocr/files"
    assert_response :success

    json = JSON.parse(response.body)
    assert json["files"].is_a?(Array)
  end

  test "files includes PDF files" do
    get "/ocr/files"
    json = JSON.parse(response.body)

    pdfs = json["files"].select { |f| f.end_with?(".pdf") }
    assert pdfs.length > 0, "Expected at least one PDF in app/python/docs/"
  end

  # ===========================================================================
  # File not found handling
  # ===========================================================================

  test "parse returns 404 for missing file" do
    get "/ocr/parse", params: { file: "nonexistent.pdf" }
    assert_response :not_found

    json = JSON.parse(response.body)
    assert json["error"].include?("not found")
  end

  test "markdown returns 404 for missing file" do
    get "/ocr/markdown", params: { file: "nonexistent.pdf" }
    assert_response :not_found
  end

  test "info returns 404 for missing file" do
    get "/ocr/info", params: { file: "nonexistent.pdf" }
    assert_response :not_found
  end

  # ===========================================================================
  # Model loading (requires vLLM server running)
  # ===========================================================================

  test "load connects to Ollama" do
    post "/ocr/load", params: { device: "cpu" }
    assert_response :success

    json = JSON.parse(response.body)
    assert json["status"].include?("GLM-OCR")
    assert json["status"].include?("Ollama")
  end

  # ===========================================================================
  # Document parsing (requires vLLM server running)
  # ===========================================================================

  test "parse returns markdown and json for PDF" do
    post "/ocr/load"
    pdf = "test.pdf"

    get "/ocr/parse", params: { file: pdf }
    assert_response :success

    json = JSON.parse(response.body)
    assert json["markdown"].is_a?(String)
    assert json["markdown"].length > 0
  end

  test "markdown returns text for PDF" do
    post "/ocr/load"
    pdf = "test.pdf"

    get "/ocr/markdown", params: { file: pdf }
    assert_response :success

    json = JSON.parse(response.body)
    assert json["markdown"].is_a?(String)
    assert_equal pdf, json["file"]
  end

  test "json_result returns structured data" do
    post "/ocr/load"
    pdf = "test.pdf"

    get "/ocr/json_result", params: { file: pdf }
    assert_response :success

    json = JSON.parse(response.body)
    assert json["result"].is_a?(Array)
  end

  # ===========================================================================
  # PDF info (uses PyMuPDF, no vLLM needed)
  # ===========================================================================

  test "info returns page count and metadata" do
    pdf = "test.pdf"

    get "/ocr/info", params: { file: pdf }
    assert_response :success

    json = JSON.parse(response.body)
    assert json["page_count"].is_a?(Integer)
    assert json["page_count"] > 0
    assert json["metadata"].is_a?(Hash)
  end

  # ===========================================================================
  # Streaming (requires vLLM server running)
  # ===========================================================================

  test "stream_pages returns SSE with page data" do
    post "/ocr/load"
    pdf = "test.pdf"

    get "/ocr/stream_pages", params: { file: pdf }
    assert_response :success
    assert_match %r{text/event-stream}, response.content_type
    assert response.body.include?("data: ")
    assert response.body.include?("[DONE]")
  end

  test "stream_pages returns 404 for missing file" do
    get "/ocr/stream_pages", params: { file: "nonexistent.pdf" }
    assert_response :not_found
  end
end
