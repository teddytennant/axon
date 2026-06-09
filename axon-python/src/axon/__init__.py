"""Axon Python agent SDK.

Write mesh agents in Python and expose them to an Axon node either as an MCP
stdio server (``axon-serve-mcp``) or over the native Axon Agent Protocol
(``axon-serve``).
"""

from .agent import Agent, CapabilityHandler, NoCapabilityError, capability, load_agent
from .types import Capability, TaskRequest, TaskResponse, TaskStatus

__version__ = "0.1.0"

__all__ = [
    "Agent",
    "Capability",
    "CapabilityHandler",
    "NoCapabilityError",
    "TaskRequest",
    "TaskResponse",
    "TaskStatus",
    "capability",
    "load_agent",
    "__version__",
]
