Rails.application.routes.draw do
  # Define your application routes per the DSL in https://guides.rubyonrails.org/routing.html

  # Reveal health status on /up that returns 200 if the app boots with no exceptions, otherwise 500.
  # Can be used by load balancers and uptime monitors to verify that the app is live.
  get "up" => "rails/health#show", as: :rails_health_check

  # Render dynamic PWA files from app/views/pwa/* (remember to link manifest in application.html.erb)
  # get "manifest" => "rails/pwa#manifest", as: :pwa_manifest
  # get "service-worker" => "rails/pwa#service_worker", as: :pwa_service_worker

  # rubyx-py demo endpoints
  scope :demo do
    # Sync — functions & classes
    get "text_analysis", to: "demo#text_analysis"
    get "slugify", to: "demo#slugify"
    get "extract_emails", to: "demo#extract_emails"

    # Sync — eval with globals
    post "eval_globals", to: "demo#eval_globals"

    # Sync streaming
    get "stream_csv", to: "demo#stream_csv"
    get "stream_transform", to: "demo#stream_transform"

    # LLM-style streaming (SSE)
    get "llm_stream", to: "demo#llm_stream"

    # Async
    get "async_fetch", to: "demo#async_fetch"
    get "async_delayed", to: "demo#async_delayed"
    get "async_future", to: "demo#async_future"

    # ML
    post "predict", to: "demo#predict"
    post "cluster", to: "demo#cluster"

    # Context
    get "context", to: "demo#context_demo"
    post "context_with_globals", to: "demo#context_with_globals"
  end

  # Part 2 — Real Python libraries (models loaded at boot via initializer)
  scope :llm do
    post "generate", to: "llm#generate"
    get "stream", to: "llm#stream"
  end

  scope :ocr do
    get "parse", to: "ocr#parse"
    get "markdown", to: "ocr#markdown"
    get "json_result", to: "ocr#json_result"
    get "stream_pages", to: "ocr#stream_pages"
    get "info", to: "ocr#info"
    get "files", to: "ocr#files"
  end

  scope :youtube do
    get "info", to: "youtube#info"
    post "download", to: "youtube#download"
    get "download_stream", to: "youtube#download_stream"
    get "formats", to: "youtube#formats"
  end
end
