"""Core protocol types mirroring axon-core/src/protocol.rs.

JSON field names match the Rust serde representation so payloads can be
read and written interchangeably across the FFI / process boundary.
"""

from __future__ import annotations

import base64
import uuid
from dataclasses import dataclass, field
from typing import Any, Union


@dataclass(frozen=True)
class Capability:
    """A capability advertised by an agent: ``namespace:name:vN``.

    Mirrors ``axon_core::protocol::Capability``.
    """

    namespace: str
    name: str
    version: int = 1

    def tag(self) -> str:
        """Canonical string form: ``"namespace:name:vN"``."""
        return f"{self.namespace}:{self.name}:v{self.version}"

    def matches(self, requested: "Capability") -> bool:
        """True if this capability satisfies ``requested`` (version >=)."""
        return (
            self.namespace == requested.namespace
            and self.name == requested.name
            and self.version >= requested.version
        )

    def to_json(self) -> dict[str, Any]:
        return {
            "namespace": self.namespace,
            "name": self.name,
            "version": self.version,
        }

    @classmethod
    def from_json(cls, data: dict[str, Any]) -> "Capability":
        return cls(
            namespace=data["namespace"],
            name=data["name"],
            version=int(data.get("version", 1)),
        )

    @classmethod
    def parse_tag(cls, tag: str) -> "Capability":
        """Parse a ``namespace:name:vN`` tag back into a Capability."""
        parts = tag.split(":")
        if len(parts) != 3 or not parts[2].startswith("v"):
            raise ValueError(f"invalid capability tag: {tag!r}")
        return cls(namespace=parts[0], name=parts[1], version=int(parts[2][1:]))


# TaskStatus is a Rust enum with one data-carrying variant (Error(String)).
# serde's default (externally tagged) representation encodes unit variants as
# bare strings ("Success") and the Error variant as {"Error": "msg"}.
class TaskStatus:
    """Status of a completed task. Mirrors ``axon_core::protocol::TaskStatus``.

    Use the constructors :meth:`success`, :meth:`error`, :meth:`timeout`,
    :meth:`no_capability`. Compare with :attr:`kind` (one of ``"Success"``,
    ``"Error"``, ``"Timeout"``, ``"NoCapability"``).
    """

    __slots__ = ("kind", "message")

    def __init__(self, kind: str, message: str | None = None) -> None:
        self.kind = kind
        self.message = message

    @classmethod
    def success(cls) -> "TaskStatus":
        return cls("Success")

    @classmethod
    def error(cls, message: str) -> "TaskStatus":
        return cls("Error", message)

    @classmethod
    def timeout(cls) -> "TaskStatus":
        return cls("Timeout")

    @classmethod
    def no_capability(cls) -> "TaskStatus":
        return cls("NoCapability")

    @property
    def is_success(self) -> bool:
        return self.kind == "Success"

    def to_json(self) -> Union[str, dict[str, Any]]:
        if self.kind == "Error":
            return {"Error": self.message or ""}
        return self.kind

    @classmethod
    def from_json(cls, data: Union[str, dict[str, Any]]) -> "TaskStatus":
        if isinstance(data, str):
            return cls(data)
        if isinstance(data, dict) and "Error" in data:
            return cls("Error", data["Error"])
        raise ValueError(f"invalid TaskStatus: {data!r}")

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, TaskStatus):
            return NotImplemented
        return self.kind == other.kind and self.message == other.message

    def __repr__(self) -> str:
        if self.kind == "Error":
            return f"TaskStatus.error({self.message!r})"
        return f"TaskStatus.{self.kind.lower()}()"


@dataclass
class TaskRequest:
    """A task request sent to an agent. Mirrors ``TaskRequest`` in Rust.

    ``payload`` is raw bytes; it is base64-encoded in the JSON form to match
    serde's handling of ``Vec<u8>`` when a human-readable encoding is used.
    """

    capability: Capability
    payload: bytes = b""
    timeout_ms: int = 30_000
    id: str = field(default_factory=lambda: str(uuid.uuid4()))

    def to_json(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "capability": self.capability.to_json(),
            "payload": base64.b64encode(self.payload).decode("ascii"),
            "timeout_ms": self.timeout_ms,
        }

    @classmethod
    def from_json(cls, data: dict[str, Any]) -> "TaskRequest":
        return cls(
            id=data.get("id", str(uuid.uuid4())),
            capability=Capability.from_json(data["capability"]),
            payload=_decode_payload(data.get("payload", "")),
            timeout_ms=int(data.get("timeout_ms", 30_000)),
        )


@dataclass
class TaskResponse:
    """A task response from an agent. Mirrors ``TaskResponse`` in Rust."""

    request_id: str
    status: TaskStatus
    payload: bytes = b""
    duration_ms: int = 0

    def to_json(self) -> dict[str, Any]:
        return {
            "request_id": self.request_id,
            "status": self.status.to_json(),
            "payload": base64.b64encode(self.payload).decode("ascii"),
            "duration_ms": self.duration_ms,
        }

    @classmethod
    def from_json(cls, data: dict[str, Any]) -> "TaskResponse":
        return cls(
            request_id=data["request_id"],
            status=TaskStatus.from_json(data["status"]),
            payload=_decode_payload(data.get("payload", "")),
            duration_ms=int(data.get("duration_ms", 0)),
        )


def _decode_payload(value: Any) -> bytes:
    """Decode a payload field that may be base64 str, list[int], or bytes."""
    if isinstance(value, bytes):
        return value
    if isinstance(value, str):
        return base64.b64decode(value) if value else b""
    if isinstance(value, list):
        # serde may emit Vec<u8> as a JSON array of integers.
        return bytes(value)
    raise ValueError(f"cannot decode payload of type {type(value).__name__}")
