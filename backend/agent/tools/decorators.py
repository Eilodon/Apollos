from __future__ import annotations

from collections.abc import Callable
from typing import TypeVar

FuncT = TypeVar('FuncT', bound=Callable[..., object])

try:
    from google.adk.tools import tool
except Exception:

    def tool(func: FuncT) -> FuncT:
        return func
