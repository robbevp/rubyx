"""LLM streaming simulation — demonstrates streaming responses like ChatGPT/Claude.

For real usage, add 'openai' or 'anthropic' to pyproject.toml dependencies.
This module works without any deps for demonstration.
"""

import asyncio


def fake_llm_stream(prompt, delay=0.05):
    """Sync generator that simulates LLM token streaming.

    Usage from Ruby:
        llm = Rubyx.import('services.llm_stream')
        Rubyx.stream(llm.fake_llm_stream("Tell me about Ruby")).each do |token|
          print token.to_ruby
        end
    """
    import time
    response = (
        f"Great question about: '{prompt}'\n\n"
        "Ruby is a dynamic, open source programming language with a focus on "
        "simplicity and productivity. It has an elegant syntax that is natural "
        "to read and easy to write.\n\n"
        "With rubyx-py, you can seamlessly bridge Ruby and Python, "
        "bringing the best of both worlds to your application."
    )
    words = response.split(' ')
    for i, word in enumerate(words):
        time.sleep(delay)
        yield word + (' ' if i < len(words) - 1 else '')


async def fake_async_llm_stream(prompt, delay=0.03):
    """Async generator simulating LLM streaming (like real API clients).

    Usage from Ruby:
        ctx = Rubyx.context
        ctx.eval("from services.llm_stream import fake_async_llm_stream")
        gen = ctx.eval("fake_async_llm_stream('Hello')")
        Rubyx.stream(gen).each { |token| print token.to_ruby }
    """
    response = (
        f"Answering: '{prompt}'\n\n"
        "Python's rich ecosystem of ML and AI libraries — PyTorch, "
        "scikit-learn, transformers, langchain — is now accessible "
        "directly from your Rails application. No microservices, "
        "no REST APIs, no serialization overhead. Just call Python "
        "functions as if they were Ruby methods."
    )
    words = response.split(' ')
    for i, word in enumerate(words):
        await asyncio.sleep(delay)
        yield word + (' ' if i < len(words) - 1 else '')


# === Real LLM examples (uncomment after adding deps to pyproject.toml) ===

# def openai_stream(prompt, model="gpt-4"):
#     """Stream from OpenAI. Add 'openai' to pyproject.toml deps."""
#     from openai import OpenAI
#     client = OpenAI()  # uses OPENAI_API_KEY env var
#     stream = client.chat.completions.create(
#         model=model,
#         messages=[{"role": "user", "content": prompt}],
#         stream=True,
#     )
#     for chunk in stream:
#         if chunk.choices[0].delta.content:
#             yield chunk.choices[0].delta.content

# async def anthropic_stream(prompt, model="claude-sonnet-4-20250514"):
#     """Stream from Anthropic. Add 'anthropic' to pyproject.toml deps."""
#     import anthropic
#     client = anthropic.AsyncAnthropic()  # uses ANTHROPIC_API_KEY env var
#     async with client.messages.stream(
#         model=model,
#         max_tokens=1024,
#         messages=[{"role": "user", "content": prompt}],
#     ) as stream:
#         async for text in stream.text_stream:
#             yield text
