"""LLM text generation using Hugging Face Transformers.

Install: uv sync --extra ml
Models: Qwen/Qwen2.5-0.5B-Instruct (small, fast, CPU-friendly)

Usage from Ruby:
    llm = Rubyx.import('services.llm')
    llm.load_model()  # one-time load

    # Sync
    result = llm.generate("What is Ruby?", max_tokens: 100)
    puts result.to_ruby

    # Streaming
    Rubyx.stream(llm.stream_generate("Explain Python")).each { |t| print t }
"""

_model = None
_tokenizer = None


def load_model(model_name="Qwen/Qwen2.5-0.5B-Instruct", device="cpu"):
    """Load model and tokenizer. Call once at boot."""
    global _model, _tokenizer
    from transformers import AutoModelForCausalLM, AutoTokenizer

    _tokenizer = AutoTokenizer.from_pretrained(model_name)
    _model = AutoModelForCausalLM.from_pretrained(
        model_name,
        torch_dtype="auto",
        device_map=device,
    )
    return f"Loaded {model_name} on {device}"


def generate(prompt, max_tokens=256, temperature=0.7, system_prompt="You are a helpful assistant."):
    """Generate a complete response (non-streaming)."""
    if _model is None:
        raise RuntimeError("Call load_model() first")

    messages = [
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": prompt},
    ]
    text = _tokenizer.apply_chat_template(messages, tokenize=False, add_generation_prompt=True)
    inputs = _tokenizer([text], return_tensors="pt").to(_model.device)

    outputs = _model.generate(
        **inputs,
        max_new_tokens=max_tokens,
        temperature=temperature,
        do_sample=temperature > 0,
    )
    # Decode only the generated tokens (skip input)
    generated = outputs[0][inputs.input_ids.shape[-1]:]
    return _tokenizer.decode(generated, skip_special_tokens=True)


def stream_generate(prompt, max_tokens=256, temperature=0.7, system_prompt="You are a helpful assistant."):
    """Stream tokens one by one via a generator.

    Usage from Ruby:
        gen = llm.stream_generate("Tell me about Ruby")
        Rubyx.stream(gen).each { |token| print token }
    """
    if _model is None:
        raise RuntimeError("Call load_model() first")
    from transformers import TextIteratorStreamer
    import threading

    messages = [
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": prompt},
    ]
    text = _tokenizer.apply_chat_template(messages, tokenize=False, add_generation_prompt=True)
    inputs = _tokenizer([text], return_tensors="pt").to(_model.device)

    streamer = TextIteratorStreamer(_tokenizer, skip_prompt=True, skip_special_tokens=True)

    thread = threading.Thread(
        target=_model.generate,
        kwargs={
            **{k: v for k, v in inputs.items()},
            "max_new_tokens": max_tokens,
            "temperature": temperature,
            "do_sample": temperature > 0,
            "streamer": streamer,
        },
    )
    thread.start()

    for token in streamer:
        if token:
            yield token

    thread.join()
