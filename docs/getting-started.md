# Getting Started with Nexus

This guide will help you install Nexus and run your first playbook.

## Installation

### Building from Source

Nexus requires Rust 1.70 or later.

```bash
# Clone the repository
git clone https://github.com/yourusername/nexus.git
cd nexus

# Build release binary
cargo build --release

# The binary is at ./target/release/nexus
```

### Verify Installation

```bash
./target/release/nexus --version
./target/release/nexus --help
```

## Your First Playbook

### 1. Create an Inventory

Create a file called `inventory.yaml`:

```yaml
defaults:
  user: your-ssh-user

all:
  children:
    webservers:
      hosts:
        server1:
          ansible_host: 192.168.1.10
        server2:
          ansible_host: 192.168.1.11
```

### 2. Create a Playbook

Create a file called `hello.nx.yaml`:

```yaml
hosts: all

tasks:
  - name: Say hello
    command: echo "Hello from Nexus on ${host.name}!"

  - name: Show system info
    command: uname -a
```

### 3. Run the Playbook

```bash
# Test connectivity first (dry run)
nexus run hello.nx.yaml -i inventory.yaml --check

# Run for real
nexus run hello.nx.yaml -i inventory.yaml

# With SSH password prompt
nexus run hello.nx.yaml -i inventory.yaml -k

# With private key
nexus run hello.nx.yaml -i inventory.yaml --private-key ~/.ssh/id_ed25519
```

## Common Operations

### Validate Playbook Syntax

```bash
nexus validate playbook.nx.yaml
```

### Preview Changes (Terraform-style)

```bash
nexus plan playbook.nx.yaml -i inventory.yaml
```

### Run with Verbose Output

```bash
nexus run playbook.nx.yaml -i inventory.yaml -v
```

### Run with Sudo

```bash
nexus run playbook.nx.yaml -i inventory.yaml --sudo -K
```

### Limit to Specific Hosts

```bash
nexus run playbook.nx.yaml -i inventory.yaml --limit server1
```

### Run Only Tagged Tasks

```bash
nexus run playbook.nx.yaml -i inventory.yaml --tags deploy,config
```

## Next Steps

- Read the [Playbook Syntax](playbook-syntax.md) guide for the full reference
- Explore the [examples/](../examples/) directory for more playbooks
- Learn about [Modules](modules.md) for package, service, and file management
- Set up [Inventory](inventory.md) for your infrastructure
