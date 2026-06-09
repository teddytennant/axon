"""Command-line entry points for the Axon Python hosts.

Two console scripts are installed (see pyproject ``[project.scripts]``):

- ``axon-serve-mcp`` -> :func:`serve_mcp` — MCP stdio server (primary path).
- ``axon-serve``     -> :func:`serve_aap` — Axon Agent Protocol (experimental).

Both load an agent from a ``module:ClassName`` spec via ``--module`` and run a
newline-delimited JSON-RPC 2.0 loop on stdio. Also runnable as a module:
``python -m axon.cli serve-mcp --module examples.echo_agent:EchoAgent``.
"""

from __future__ import annotations

import argparse
import sys
from typing import Optional

from . import aap_server, mcp_server
from .agent import load_agent


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="axon",
        description="Serve a Python Axon agent over MCP or the Axon Agent Protocol.",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    for name, help_text in (
        ("serve-mcp", "Serve the agent as an MCP stdio server."),
        ("serve", "Serve the agent over the Axon Agent Protocol (experimental)."),
    ):
        sp = sub.add_parser(name, help=help_text)
        sp.add_argument(
            "--module",
            required=True,
            metavar="module:ClassName",
            help="Agent to load, e.g. examples.echo_agent:EchoAgent",
        )

    return parser


def _run(argv: Optional[list[str]], protocol: str) -> int:
    parser = argparse.ArgumentParser(prog=f"axon-serve{'-mcp' if protocol == 'mcp' else ''}")
    parser.add_argument(
        "--module",
        required=True,
        metavar="module:ClassName",
        help="Agent to load, e.g. examples.echo_agent:EchoAgent",
    )
    args = parser.parse_args(argv)
    agent = load_agent(args.module)
    if protocol == "mcp":
        mcp_server.serve(agent)
    else:
        aap_server.serve(agent)
    return 0


def serve_mcp(argv: Optional[list[str]] = None) -> int:
    """Console entry point for ``axon-serve-mcp``."""
    return _run(argv, "mcp")


def serve_aap(argv: Optional[list[str]] = None) -> int:
    """Console entry point for ``axon-serve``."""
    return _run(argv, "aap")


def main(argv: Optional[list[str]] = None) -> int:
    """Module entry point: ``python -m axon.cli <serve-mcp|serve> --module ...``."""
    parser = _build_parser()
    args = parser.parse_args(argv)
    agent = load_agent(args.module)
    if args.command == "serve-mcp":
        mcp_server.serve(agent)
    else:
        aap_server.serve(agent)
    return 0


if __name__ == "__main__":
    sys.exit(main())
