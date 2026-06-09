"""Agent authoring: the :class:`Agent` base class and :func:`capability`.

An agent subclasses :class:`Agent`, sets a ``name``, and decorates async
methods with :func:`capability`. Each decorated method becomes a handler for
one ``namespace:name:vN`` capability and is exposed by the MCP and AAP hosts.

    class EchoAgent(Agent):
        name = "echo"

        @capability("echo", "ping", version=1)
        async def ping(self, payload: bytes) -> bytes:
            return payload
"""

from __future__ import annotations

import inspect
from typing import Any, Awaitable, Callable, Optional

from .types import Capability, TaskRequest, TaskResponse, TaskStatus

# Attribute stamped onto a method by @capability, holding its Capability and
# optional human description. Read by Agent.__init_subclass__ to build the map.
_CAP_ATTR = "__axon_capability__"

HandlerFn = Callable[..., Awaitable[bytes]]


class CapabilityHandler:
    """Binding of a :class:`Capability` to an agent method.

    ``description`` defaults to the method docstring's first line and is used
    as the MCP tool description. ``input_schema`` is an optional JSON Schema
    advertised to MCP clients; when absent a permissive object schema is used.
    """

    __slots__ = ("capability", "description", "input_schema", "func", "method_name")

    def __init__(
        self,
        capability: Capability,
        func: HandlerFn,
        description: str,
        input_schema: Optional[dict[str, Any]],
    ) -> None:
        self.capability = capability
        self.func = func
        self.method_name = func.__name__
        self.description = description
        self.input_schema = input_schema


def capability(
    namespace: str,
    name: str,
    version: int = 1,
    *,
    description: Optional[str] = None,
    input_schema: Optional[dict[str, Any]] = None,
) -> Callable[[HandlerFn], HandlerFn]:
    """Mark an async agent method as the handler for a capability.

    The method must be ``async`` and accept ``payload: bytes`` (raw task
    payload), returning ``bytes``. ``description`` and ``input_schema`` feed
    the MCP tool advertisement.
    """

    def decorator(func: HandlerFn) -> HandlerFn:
        if not inspect.iscoroutinefunction(func):
            raise TypeError(
                f"@capability handler {func.__name__!r} must be `async def`"
            )
        cap = Capability(namespace=namespace, name=name, version=version)
        desc = description or _first_doc_line(func) or cap.tag()
        setattr(
            func,
            _CAP_ATTR,
            CapabilityHandler(cap, func, desc, input_schema),
        )
        return func

    return decorator


class Agent:
    """Base class for Python agents.

    Subclasses set a class-level ``name`` and decorate async methods with
    :func:`capability`. The registry of handlers is built once per subclass.
    """

    name: str = "agent"

    # capability tag -> CapabilityHandler, populated per-subclass.
    _handlers: dict[str, CapabilityHandler] = {}

    def __init_subclass__(cls, **kwargs: Any) -> None:
        super().__init_subclass__(**kwargs)
        handlers: dict[str, CapabilityHandler] = {}
        # Walk the full MRO so inherited handlers are included; subclass
        # definitions win on tag collisions.
        for klass in reversed(cls.__mro__):
            for value in vars(klass).values():
                handler = getattr(value, _CAP_ATTR, None)
                if isinstance(handler, CapabilityHandler):
                    handlers[handler.capability.tag()] = handler
        cls._handlers = handlers

    def capabilities(self) -> list[Capability]:
        """All capabilities this agent provides."""
        return [h.capability for h in self._handlers.values()]

    def handlers(self) -> list[CapabilityHandler]:
        """All capability handlers, in registration order."""
        return list(self._handlers.values())

    def handler_for(self, cap: Capability) -> Optional[CapabilityHandler]:
        """Find the handler satisfying ``cap`` (exact tag, then version match)."""
        exact = self._handlers.get(cap.tag())
        if exact is not None:
            return exact
        for handler in self._handlers.values():
            if handler.capability.matches(cap):
                return handler
        return None

    async def invoke(self, cap: Capability, payload: bytes) -> bytes:
        """Run the handler for ``cap`` against ``payload``, returning bytes."""
        handler = self.handler_for(cap)
        if handler is None:
            raise NoCapabilityError(cap.tag())
        return await handler.func(self, payload)

    async def handle(self, request: TaskRequest) -> TaskResponse:
        """Dispatch a :class:`TaskRequest`, mirroring the Rust Agent trait.

        Never raises: failures are mapped onto the appropriate
        :class:`TaskStatus`, matching the Rust runtime's response contract.
        """
        try:
            payload = await self.invoke(request.capability, request.payload)
            return TaskResponse(
                request_id=request.id,
                status=TaskStatus.success(),
                payload=payload,
            )
        except NoCapabilityError:
            return TaskResponse(
                request_id=request.id,
                status=TaskStatus.no_capability(),
            )
        except Exception as exc:  # noqa: BLE001 — surfaced as TaskStatus.Error
            return TaskResponse(
                request_id=request.id,
                status=TaskStatus.error(str(exc)),
            )


class NoCapabilityError(Exception):
    """Raised when an agent has no handler for a requested capability."""

    def __init__(self, tag: str) -> None:
        super().__init__(f"no handler for capability {tag}")
        self.tag = tag


def _first_doc_line(func: Callable[..., Any]) -> Optional[str]:
    doc = inspect.getdoc(func)
    if not doc:
        return None
    return doc.strip().splitlines()[0]


def load_agent(spec: str) -> Agent:
    """Instantiate an agent from a ``"module.path:ClassName"`` spec."""
    import importlib

    if ":" not in spec:
        raise ValueError(
            f"agent spec must be 'module:ClassName', got {spec!r}"
        )
    module_name, _, class_name = spec.partition(":")
    module = importlib.import_module(module_name)
    cls = getattr(module, class_name, None)
    if cls is None:
        raise ImportError(f"{class_name!r} not found in module {module_name!r}")
    if not (isinstance(cls, type) and issubclass(cls, Agent)):
        raise TypeError(f"{spec!r} is not an axon.Agent subclass")
    return cls()
