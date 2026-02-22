# Quick Start

## Start a Node

```bash
# Start with TUI dashboard (default)
axon-cli start

# Start headless
axon-cli start --headless

# Start with a specific LLM provider
axon-cli start --provider xai --api-key $XAI_API_KEY

# Connect to a bootstrap peer
axon-cli start --peer 192.168.1.100:4242
```

## Send a Task

From another terminal:

```bash
# Echo test
axon-cli send --peer 127.0.0.1:4242 --namespace echo --name ping -d "hello"

# LLM chat
axon-cli send --peer 127.0.0.1:4242 --namespace llm --name chat -d "What is Rust?"

# System info
axon-cli send --peer 127.0.0.1:4242 --namespace system --name info
```

## Multi-Node Mesh

Start two nodes on the same LAN — they discover each other automatically via mDNS:

```bash
# Terminal 1
axon-cli start --listen 0.0.0.0:4242

# Terminal 2
axon-cli start --listen 0.0.0.0:4243

# Send a task to node 1 from node 2
axon-cli send --peer 127.0.0.1:4242 --namespace echo --name ping -d "mesh works"
```

## Check Identity

```bash
axon-cli identity
# Identity file: ~/.config/axon/identity.key
# Peer ID: a1b2c3d4...
# Short ID: a1b2c3d4
```
