# frozen_string_literal: true

require "test_helper"

class YoutubeControllerTest < ActionDispatch::IntegrationTest
  RICKROLL_URL = "https://www.youtube.com/watch?v=dQw4w9WgXcQ"

  # ===========================================================================
  # Validation
  # ===========================================================================

  test "info requires url param" do
    get "/youtube/info"
    assert_response :bad_request

    json = JSON.parse(response.body)
    assert_equal "url parameter required", json["error"]
  end

  test "download requires url param" do
    post "/youtube/download"
    assert_response :bad_request
  end

  test "formats requires url param" do
    get "/youtube/formats"
    assert_response :bad_request
  end

  test "download_stream requires url param" do
    get "/youtube/download_stream"
    assert_response :bad_request
  end

  # ===========================================================================
  # Integration — video info
  # ===========================================================================

  test "info returns video metadata for rickroll" do
    get "/youtube/info", params: { url: RICKROLL_URL }
    assert_response :success

    json = JSON.parse(response.body)
    assert_match(/never gonna give you up/i, json["title"])
    assert json["duration"].is_a?(Integer)
    assert json["duration"] > 200 # ~3:32
    assert json["view_count"].is_a?(Integer)
    assert json["view_count"] > 1_000_000_000 # 1B+ views
    assert json["uploader"].is_a?(String)
    assert json["thumbnail"].is_a?(String)
  end

  # ===========================================================================
  # Integration — formats
  # ===========================================================================

  test "formats returns available formats for rickroll" do
    get "/youtube/formats", params: { url: RICKROLL_URL }
    assert_response :success

    json = JSON.parse(response.body)
    assert json["formats"].is_a?(Array)
    assert json["formats"].length > 10 # many formats available
    assert json["formats"][0]["format_id"].is_a?(String)
  end

  # ===========================================================================
  # Integration — actual download
  # ===========================================================================

  test "download_stream returns SSE progress for rickroll" do
    get "/youtube/download_stream", params: { url: RICKROLL_URL }
    assert_response :success
    assert_match %r{text/event-stream}, response.content_type
    assert response.body.include?("data: ")
  end

  test "download rickroll audio" do
    output_dir = Rails.root.join("tmp/downloads").to_s
    FileUtils.rm_rf(output_dir) # clean slate

    post "/youtube/download",
      params: { url: RICKROLL_URL, format: "worstaudio" }.to_json,
      headers: { "Content-Type" => "application/json" }
    assert_response :success

    json = JSON.parse(response.body)
    assert_match(/never gonna give you up/i, json["title"])
    assert json["filename"].is_a?(String)
    assert json["filesize"].is_a?(Integer)
    assert json["filesize"] > 0
    assert File.exist?(json["filename"]), "Downloaded file should exist"

    # Cleanup
    FileUtils.rm_rf(output_dir)
  end
end
