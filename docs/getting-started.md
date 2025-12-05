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

Nexus offers multiple ways to run playbooks. Choose the approach that best fits your needs.

### Option A: Traditional Inventory (Best for Large Infrastructures)

**1. Create an Inventory**

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

**2. Create a Playbook**

Create a file called `hello.nx.yaml`:

```yaml
hosts: all

tasks:
  - name: Say hello
    command: echo "Hello from Nexus on ${host.name}!"

  - name: Show system info
    command: uname -a
```

**3. Run the Playbook**

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

### Option B: Embedded Hosts (Best for Portable Playbooks)

**1. Create a Self-contained Playbook**

Create a file called `hello-embedded.nx.yaml`:

```yaml
hosts:
  - name: server1
    address: 192.168.1.10
    user: your-ssh-user
  - name: server2
    address: 192.168.1.11
    user: your-ssh-user

tasks:
  - name: Say hello
    command: echo "Hello from Nexus on ${host.name}!"

  - name: Show system info
    command: uname -a
```

**2. Run the Playbook (No Inventory Needed)**

```bash
# Just run it directly
nexus run hello-embedded.nx.yaml

# With private key
nexus run hello-embedded.nx.yaml --private-key ~/.ssh/id_ed25519
```

### Option C: Explicit Host List (Best for Quick Tasks)

**1. Create a Simple Playbook**

Create a file called `hello-simple.nx.yaml`:

```yaml
# No hosts specified - will be provided at runtime
tasks:
  - name: Say hello
    command: echo "Hello from Nexus!"

  - name: Show system info
    command: uname -a
```

**2. Run with Host List**

```bash
# Specify hosts on command line
nexus run hello-simple.nx.yaml --hosts 192.168.1.10,192.168.1.11

# With connection details
nexus run hello-simple.nx.yaml --hosts 192.168.1.10 --user admin --private-key ~/.ssh/id_ed25519
```

### Option D: Network Discovery (Best for Dynamic Environments)

**1. Create a Playbook**

Use the same `hello-simple.nx.yaml` from Option C.

**2. Discover and Run**

```bash
# Discover hosts on subnet and run playbook
nexus run hello-simple.nx.yaml --discover 192.168.1.0/24

# With custom probe and timeout
nexus run hello-simple.nx.yaml --discover 192.168.1.0/24 --probe ssh --timeout 5s

# Just discover (don't run playbook)
nexus discover --subnet 192.168.1.0/24 --save-to discovered-inventory.yaml
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

## Choosing the Right Approach

| Approach | Best For | Pros | Cons |
|----------|----------|------|------|
| **Traditional Inventory** | Large infrastructures (>50 hosts) | Central management, complex grouping | Requires maintenance |
| **Embedded Hosts** | Small, stable host sets | Portable, self-documented | Not scalable |
| **Explicit Host List** | One-off tasks | Quick and simple | Manual host entry |
| **Network Discovery** | Dynamic environments | Automatic, up-to-date | Requires network access |

## Migrating from Ansible?

If you have existing Ansible playbooks, you can convert them automatically:

```bash
# Convert a single playbook
nexus convert site.yml -o site.nx.yml

# Preview conversion first
nexus convert site.yml --dry-run

# Convert entire project
nexus convert ansible-project/ -o nexus-project/ --all
```

See the [Ansible Migration Guide](ansible-migration.md) for complete migration instructions, conversion mappings, and best practices.

## Next Steps

### Core Documentation
- Read the [Playbook Syntax](playbook-syntax.md) guide for the full reference
- Learn about [Modules](modules.md) for package, service, and file management
- Set up [Inventory](inventory.md) for your infrastructure

### Migration and Features
- **Coming from Ansible?** Read the [Ansible Migration Guide](ansible-migration.md) to convert your playbooks
- Explore [Inventory-less Execution](inventory-less-execution.md) for dynamic host targeting
- Learn about [Network Discovery](network-discovery.md) for automatic host detection
- Check out the [CLI Reference](cli.md) for all available commands

### Examples
- Explore the [examples/](../examples/) directory for more playbooks
- Check out the [Advanced Features](advanced-features.md) guide for roles, blocks, and async tasks
