# frozen_string_literal: true

# YouTube video downloader via yt-dlp.
# Included in base dependencies — no extra install needed.
class YoutubeController < ApplicationController
  include ActionController::Live

  # GET /youtube/info?url=https://youtube.com/watch?v=...
  def info
    yt = Rubyx.import('services.youtube')
    url = params[:url]

    return render json: { error: "url parameter required" }, status: :bad_request unless url

    info = yt.video_info(url)
    render json: info.to_ruby
  end

  # POST /youtube/download  body: { "url": "...", "format": "bestaudio" }
  def download
    yt = Rubyx.import('services.youtube')
    url = params[:url]
    format = params[:format] || "bestaudio[ext=m4a]/bestaudio/best"
    output_dir = Rails.root.join('tmp/downloads').to_s

    return render json: { error: "url parameter required" }, status: :bad_request unless url

    result = yt.download(url, output_dir, format)
    render json: result.to_ruby
  end

  # GET /youtube/download_stream?url=...
  def download_stream
    yt = Rubyx.import('services.youtube')
    url = params[:url]
    output_dir = Rails.root.join('tmp/downloads').to_s

    return render json: { error: "url parameter required" }, status: :bad_request unless url

    response.headers['Content-Type'] = 'text/event-stream'
    response.headers['Cache-Control'] = 'no-cache'
    response.headers['X-Accel-Buffering'] = 'no'

    gen = yt.download_with_progress(url, output_dir)
    Rubyx.stream(gen).each do |update|
      response.stream.write("data: #{update}\n\n")
    end
    response.stream.write("data: [DONE]\n\n")
  rescue ActionController::Live::ClientDisconnected
    # Client disconnected
  ensure
    response.stream.close
  end

  # GET /youtube/formats?url=...
  def formats
    yt = Rubyx.import('services.youtube')
    url = params[:url]

    return render json: { error: "url parameter required" }, status: :bad_request unless url

    formats = yt.list_formats(url)
    render json: { formats: formats.to_ruby }
  end
end
