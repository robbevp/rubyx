# frozen_string_literal: true

require "test_helper"

class OcrControllerTest < ActionDispatch::IntegrationTest
  # Models are loaded at boot in production/development.
  # In test env, we load once before all tests.

  DOCS_DIR = Rails.root.join("app/python/docs")

  @@ocr_loaded = false
  setup do
    unless @@ocr_loaded
      ocr = Rubyx.import('services.ocr')
      ocr.load(Rails.root.join('config/glmocr.yaml').to_s)
      @@ocr_loaded = true
    end
  end

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

  test "stream_pages returns 404 for missing file" do
    get "/ocr/stream_pages", params: { file: "nonexistent.pdf" }
    assert_response :not_found
  end

  # ===========================================================================
  # PDF info (uses PyMuPDF, no OCR model needed)
  # ===========================================================================

  test "info returns page count and metadata" do
    get "/ocr/info", params: { file: "test.pdf" }
    assert_response :success

    json = JSON.parse(response.body)
    assert json["page_count"].is_a?(Integer)
    assert json["page_count"] > 0
    assert json["metadata"].is_a?(Hash)
  end

  # ===========================================================================
  # Document parsing (requires Ollama + glm-ocr)
  # ===========================================================================

  test "parse returns markdown and json for PDF" do
    get "/ocr/parse", params: { file: "test.pdf" }
    assert_response :success

    json = JSON.parse(response.body)
    assert json["markdown"].is_a?(String)
    assert json["markdown"].length > 0
  end

  test "markdown returns text for PDF" do
    get "/ocr/markdown", params: { file: "test.pdf" }
    assert_response :success

    json = JSON.parse(response.body)
    assert json["markdown"].is_a?(String)
    assert_equal "test.pdf", json["file"]
  end

  test "json_result returns structured data" do
    get "/ocr/json_result", params: { file: "test.pdf" }
    assert_response :success

    json = JSON.parse(response.body)
    assert json["result"].is_a?(Array)
  end

  # ===========================================================================
  # Streaming
  # ===========================================================================

  test "stream_pages returns SSE with page data" do
    get "/ocr/stream_pages", params: { file: "test.pdf" }
    assert_response :success
    assert_match %r{text/event-stream}, response.content_type
    assert response.body.include?("data: ")
    assert response.body.include?("[DONE]")
  end
end
