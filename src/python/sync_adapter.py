import asyncio
from asyncio import AbstractEventLoop
from typing import AsyncGenerator


class AsyncToSync:
    def __init__(self, async_gen: AsyncGenerator):
        self._agen = async_gen
        self._loop: AbstractEventLoop = asyncio.new_event_loop()


    def __iter__(self):
        return self

    def __next__(self):
        try:
            coro = self._agen.__anext__()
            return self._loop.run_until_complete(coro)
        except StopAsyncIteration:
            self._loop.close()
            raise StopIteration
        except BaseException:
            self._loop.close()
            raise

    def close(self):
        try:
            self._loop.run_until_complete(self._agen.aclose())
        except Exception:
            pass
        self._loop.close()
