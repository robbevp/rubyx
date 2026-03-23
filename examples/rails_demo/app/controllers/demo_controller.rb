# frozen_string_literal: true

# Demonstrates all rubyx-py capabilities in Rails.
# Run: rails routes to see all endpoints.
class DemoController < ApplicationController
  include ActionController::Live # Required for SSE streaming
  # ===========================================================================
  # SYNC — Simple function calls and classes
  # ===========================================================================

  # GET /demo/text_analysis?text=Hello+world+hello
  def text_analysis
    tp = Rubyx.import('services.text_processing')

    analyzer = tp.TextAnalyzer(params[:text] || "Hello world from rubyx-py")
    render json: {
      summary: analyzer.summary.to_ruby,
      most_common: analyzer.most_common(3).to_ruby
    }
  end

  # GET /demo/slugify?text=Hello World! This is great
  def slugify
    tp = Rubyx.import('services.text_processing')
    slug = tp.slugify(params[:text] || "Hello World!")
    render json: { slug: slug.to_ruby }
  end

  # GET /demo/extract_emails?text=contact+alice@example.com+or+bob@test.org
  def extract_emails
    tp = Rubyx.import('services.text_processing')
    emails = tp.extract_emails(params[:text] || "email me at test@example.com")
    render json: { emails: emails.to_ruby }
  end

  # ===========================================================================
  # SYNC — Eval with globals (pass Ruby data into Python)
  # ===========================================================================

  # POST /demo/eval_globals  body: { "x": 10, "y": 20 }
  def eval_globals
    x = params[:x]&.to_i || 0
    y = params[:y]&.to_i || 0
    result = Rubyx.eval("x ** 2 + y ** 2", x: x, y: y)
    render json: { expression: "x**2 + y**2", x: x, y: y, result: result.to_ruby }
  end

  # ===========================================================================
  # SYNC STREAMING — Generator → Rubyx.stream
  # ===========================================================================

  # GET /demo/stream_csv
  def stream_csv
    dt = Rubyx.import('services.data_transform')

    csv_data = "name,age,city\nAlice,30,NYC\nBob,25,LA\nCharlie,35,Chicago"
    records = dt.csv_to_records(csv_data)

    render json: { records: records.to_ruby }
  end

  # GET /demo/stream_transform
  def stream_transform
    dt = Rubyx.import('services.data_transform')

    items = (1..5).map { |i| { "id" => i, "value" => i * 10 } }
    # Use a Python lambda via eval to transform
    ctx = Rubyx.context
    ctx.eval("transform = lambda r: {**r, 'doubled': r['value'] * 2}")
    transform = ctx.eval("transform")

    results = []
    gen = dt.stream_transform(items, transform)
    Rubyx.stream(gen).each { |r| results << r }

    render json: { results: results }
  end

  # ===========================================================================
  # SYNC STREAMING — LLM-style token stream (SSE)
  # ===========================================================================

  # GET /demo/llm_stream?prompt=Tell+me+about+Ruby
  def llm_stream
    llm = Rubyx.import('services.llm_stream')
    prompt = params[:prompt] || "Tell me about Ruby"

    # Server-Sent Events for real-time token streaming
    response.headers['Content-Type'] = 'text/event-stream'
    response.headers['Cache-Control'] = 'no-cache'
    response.headers['X-Accel-Buffering'] = 'no'

    gen = llm.fake_llm_stream(prompt, 0.05)
    Rubyx.stream(gen).each do |token|
      response.stream.write("data: #{token}\n\n")
    end
    response.stream.write("data: [DONE]\n\n")
  rescue ActionController::Live::ClientDisconnected
    # Client disconnected — generator cleanup is automatic
  ensure
    response.stream.close
  end

  # ===========================================================================
  # ASYNC — Rubyx.await (blocking) and Context#await
  # ===========================================================================

  # GET /demo/async_fetch?url=https://httpbin.org/json
  def async_fetch
    ctx = Rubyx.context
    ctx.eval("from services.async_utils import fetch_url")

    url = params[:url] || "https://httpbin.org/json"
    result = ctx.await("fetch_url(url)", url: url)

    render json: { url: url, response_length: result.to_ruby.length }
  end

  # GET /demo/async_delayed
  def async_delayed
    ctx = Rubyx.context
    ctx.eval("from services.async_utils import delayed_result")

    result = ctx.await("delayed_result(msg, 0.1)", msg: "Hello from async!")
    render json: { result: result.to_ruby }
  end

  # ===========================================================================
  # ASYNC NON-BLOCKING — Rubyx.async_await (returns Future)
  # ===========================================================================

  # GET /demo/async_future
  def async_future
    ctx = Rubyx.context
    ctx.eval("from services.async_utils import delayed_result")

    # Fire off async work — Ruby thread is free
    future = ctx.async_await("delayed_result(42, 0.2)")

    # Do other Ruby work while Python runs in background
    ruby_work = (1..1000).sum

    # Now collect the result (blocks only if not ready yet)
    py_result = future.value

    render json: {
      python_result: py_result,
      ruby_work: ruby_work,
      message: "Both ran concurrently!"
    }
  end

  # ===========================================================================
  # ML — Persistent model objects
  # ===========================================================================

  # POST /demo/predict  body: { "x_data": [1,2,3,4,5], "y_data": [2,4,6,8,10], "predict_x": 6 }
  def predict
    ml = Rubyx.import('ml.predictor')

    x_data = (params[:x_data] || [1, 2, 3, 4, 5]).map(&:to_f)
    y_data = (params[:y_data] || [2, 4, 6, 8, 10]).map(&:to_f)
    predict_x = (params[:predict_x] || 6).to_f

    model = ml.SimplePredictor(x_data, y_data)
    prediction = model.predict(predict_x)

    render json: {
      prediction: prediction.to_ruby,
      summary: model.summary.to_ruby
    }
  end

  # POST /demo/cluster  body: { "data": [[1,1],[1,2],[10,10],[10,11]], "k": 2 }
  def cluster
    ml = Rubyx.import('ml.predictor')

    data = (params[:data] || [[1, 1], [1, 2], [10, 10], [10, 11]]).map { |p| p.map(&:to_f) }
    k = (params[:k] || 2).to_i

    km = ml.KMeansSimple(k)
    km.fit(data)

    assignments = data.map { |point| km.predict(point).to_ruby }

    render json: {
      centroids: km.centroids.to_ruby,
      assignments: assignments
    }
  end

  # ===========================================================================
  # CONTEXT — Stateful Python sessions
  # ===========================================================================

  # GET /demo/context
  def context_demo
    ctx = Rubyx.context

    # Build up state across multiple eval calls
    ctx.eval("import math")
    ctx.eval("data = []")
    ctx.eval("data.append(math.pi)")
    ctx.eval("data.append(math.e)")
    ctx.eval("data.append(math.sqrt(2))")

    result = ctx.eval("{'values': data, 'sum': sum(data)}")
    render json: result.to_ruby
  end

  # POST /demo/context_with_globals  body: { "radius": 5 }
  def context_with_globals
    ctx = Rubyx.context
    ctx.eval("import math")

    radius = (params[:radius] || 5).to_f
    result = ctx.eval("{'area': math.pi * r**2, 'circumference': 2 * math.pi * r}", r: radius)

    render json: result.to_ruby
  end
end
