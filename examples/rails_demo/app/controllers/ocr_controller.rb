# frozen_string_literal: true

# Document OCR via GLM-OCR (self-hosted with Ollama).
# Model is loaded once at boot in config/initializers/rubyx.rb
class OcrController < ApplicationController
  include ActionController::Live

  DOCS_DIR = Rails.root.join('app/python/docs')

  # GET /ocr/parse?file=Statement_of_Accounts_2024_2025_AF.pdf
  def parse
    ocr = Rubyx.import('services.ocr')
    path = resolve_file_path
    return unless path

    result = ocr.parse_document(path)
    render json: result.to_ruby
  end

  # GET /ocr/markdown?file=invoice.pdf
  def markdown
    ocr = Rubyx.import('services.ocr')
    path = resolve_file_path
    return unless path

    text = ocr.parse_to_markdown(path)
    render json: { file: params[:file], markdown: text.to_ruby }
  end

  # GET /ocr/json_result?file=invoice.pdf
  def json_result
    ocr = Rubyx.import('services.ocr')
    path = resolve_file_path
    return unless path

    data = ocr.parse_to_json(path)
    render json: { file: params[:file], result: data.to_ruby }
  end

  # GET /ocr/stream_pages?file=report.pdf
  def stream_pages
    ocr = Rubyx.import('services.ocr')
    path = resolve_file_path
    return unless path

    response.headers['Content-Type'] = 'text/event-stream'
    response.headers['Cache-Control'] = 'no-cache'
    response.headers['X-Accel-Buffering'] = 'no'

    Rubyx.stream(ocr.stream_pages(path)).each do |page_data|
      response.stream.write("data: #{page_data.to_json}\n\n")
    end
    response.stream.write("data: [DONE]\n\n")
  rescue ActionController::Live::ClientDisconnected
    # Client disconnected
  ensure
    response.stream.close
  end

  # GET /ocr/info?file=report.pdf
  def info
    ocr = Rubyx.import('services.ocr')
    path = resolve_file_path
    return unless path

    result = ocr.pdf_info(path)
    render json: result.to_ruby
  end

  # GET /ocr/files
  def files
    exts = %w[.pdf .png .jpg .jpeg .tiff .bmp]
    docs = Dir.children(DOCS_DIR).select { |f| exts.include?(File.extname(f).downcase) }
    render json: { files: docs }
  end

  private

  def resolve_file_path
    filename = params[:file] || Dir.children(DOCS_DIR).first
    path = DOCS_DIR.join(filename).to_s

    unless File.exist?(path)
      render json: { error: "File not found: #{filename}" }, status: :not_found
      return nil
    end
    path
  end
end
