"""Async utilities — demonstrating async functions and async streaming."""

import asyncio


async def fetch_url(url):
    """Async HTTP fetch using built-in urllib (no deps needed).

    Usage from Ruby:
        ctx = Rubyx.context
        ctx.eval("from services.async_utils import fetch_url")
        result = ctx.await("fetch_url('https://httpbin.org/json')")
    """
    import urllib.request
    loop = asyncio.get_event_loop()
    response = await loop.run_in_executor(
        None,
        lambda: urllib.request.urlopen(url).read().decode('utf-8')
    )
    return response


async def fetch_multiple(urls):
    """Fetch multiple URLs concurrently.

    Usage from Ruby:
        future = ctx.async_await("fetch_multiple(urls)", urls: ["http://...", "http://..."])
        results = future.value
    """
    tasks = [fetch_url(url) for url in urls]
    return await asyncio.gather(*tasks)


async def delayed_result(value, delay=0.1):
    """Simulate an async operation with delay.

    Usage from Ruby:
        result = Rubyx.await(coro)  # blocks until done
        # or
        future = Rubyx.async_await(coro)  # non-blocking
        future.value  # blocks only when you need the result
    """
    await asyncio.sleep(delay)
    return value


async def async_countdown(n):
    """Async generator — streams countdown values.

    Usage from Ruby:
        ctx.eval("from services.async_utils import async_countdown")
        gen = ctx.eval("async_countdown(5)")
        Rubyx.stream(gen).each { |i| puts i.to_ruby }
    """
    for i in range(n, 0, -1):
        await asyncio.sleep(0.01)
        yield i
    yield 0


async def async_pipeline(items, *transforms):
    """Process items through async transform pipeline, yielding each result.

    Each transform is an async function: async def(item) -> item
    """
    for item in items:
        result = item
        for fn in transforms:
            result = await fn(result)
        yield result
