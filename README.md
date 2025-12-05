# Nexus

**Next-Generation Infrastructure Automation**

Nexus is a modern infrastructure automation tool written in Rust that addresses core limitations of Ansible while maintaining a familiar YAML-based playbook syntax. It's designed for speed, reliability, and sophisticated orchestration.

## Key Features

- **High Performance**: Built in Rust with async/await for true parallelism
- **Familiar Syntax**: YAML playbooks similar to Ansible for easy migration
- **Ansible Migration**: Built-in `nexus convert` command to automatically convert existing Ansible playbooks, roles, and projects
- **Inventory-less Execution**: Run playbooks without inventory files using CLI flags, embedded hosts, or implicit localhost
- **Network Discovery**: Built-in host discovery with `nexus discover` - scan networks for SSH hosts, fingerprint OS, and auto-generate inventory
- **Advanced Orchestration**: Circuit breakers, checkpoints, retry with backoff
- **Terraform-style Planning**: Preview changes before applying (`nexus plan`)
- **Live TUI Dashboard**: Real-time execution monitoring
- **Vault Encryption**: Secure secrets with AES-256-GCM
- **Rich Expression Language**: Variables, filters, functions, and conditionals

## Quick Start

```bash
# Build from source
cargo build --release

# Validate a playbook
./target/release/nexus validate examples/01-hello-world.nx.yaml

# Run a playbook with inventory
./target/release/nexus run examples/01-hello-world.nx.yaml -i examples/inventory.yaml

# Run without inventory - specify hosts inline
./target/release/nexus run playbook.nx.yaml --hosts "server1.example.com,server2.example.com"

# Discover hosts on a network
./target/release/nexus discover --subnet 10.20.30.0/24

# Convert Ansible playbook to Nexus
./target/release/nexus convert ansible-playbook.yml -o nexus-playbook.nx.yaml

# Preview changes (Terraform-style)
./target/release/nexus plan examples/05-webserver-setup.nx.yaml -i examples/inventory.yaml

# Run with live TUI dashboard
./target/release/nexus run playbook.nx.yaml -i inventory.yaml --tui
```

## Documentation

- [Getting Started](docs/getting-started.md) - Installation and first playbook
- [Ansible Migration](docs/ansible-migration.md) - Convert existing Ansible infrastructure to Nexus
- [Playbook Syntax](docs/playbook-syntax.md) - Complete playbook reference
- [Modules Reference](docs/modules.md) - Built-in module documentation
- [Inventory Guide](docs/inventory.md) - Static and dynamic inventory
- [CLI Reference](docs/cli.md) - All command-line options
- [Advanced Features](docs/advanced-features.md) - Vault, checkpoints, roles, and more

## Example Playbook

```yaml
# webserver.nx.yaml
hosts: webservers

vars:
  http_port: 80
  document_root: /var/www/html

tasks:
  - name: Install nginx
    package: nginx
    state: installed
    sudo: true

  - name: Start nginx
    service: nginx
    state: running
    enabled: true
    sudo: true

  - name: Deploy index page
    file: ${vars.document_root}/index.html
    content: |
      <h1>Hello from ${host.name}!</h1>
    sudo: true
    notify: reload_nginx

handlers:
  - name: reload_nginx
    service: nginx
    state: reloaded
    sudo: true
```

## Command Execution

Nexus provides two ways to execute commands on remote hosts:

```yaml
# command: - Direct execution (secure, no shell features)
- name: Run command directly
  command: /usr/bin/myapp --config /etc/myapp.conf

# shell: - Shell execution (supports $variables, pipes, redirects)
- name: Run with shell features
  shell: echo "User: $USER" | tee /tmp/user.txt
```

| Module | Use Case |
|--------|----------|
| `command:` | Simple commands, security-sensitive operations |
| `shell:` | Pipes, redirects, environment variables, complex scripts |

**Security Note**: Use `command:` by default for better security. Only use `shell:` when you specifically need shell features like pipes, redirects, or variable expansion.

## Inventory-less Execution

Nexus breaks free from the traditional requirement of inventory files, offering flexible ways to specify target hosts. This makes Nexus ideal for ad-hoc operations, cloud-init scripts, and dynamic environments.

### CLI Inline Hosts

Specify hosts directly on the command line using the `-H` or `--hosts` flag:

```bash
# Comma-separated hostnames
nexus run deploy.nx.yaml --hosts "server1.example.com,server2.example.com"

# IP addresses with custom user
nexus run deploy.nx.yaml -H "192.168.1.10,192.168.1.11" --user admin

# Mix of hostnames and IPs
nexus run patch.nx.yaml --hosts "web1.local,10.0.1.5,db.example.com"
```

### Implicit Localhost

When a playbook targets `localhost`, no inventory is needed at all:

```bash
nexus run local-setup.nx.yaml
```

```yaml
# local-setup.nx.yaml
hosts: localhost

tasks:
  - name: Install development tools
    package: git,vim,tmux
    state: installed
    sudo: true
```

### Playbook-embedded Hosts

Define hosts directly in your playbook for self-contained automation:

```yaml
name: Self-contained deployment
hosts:
  - name: web1
    address: 192.168.1.10
    user: admin
  - name: web2
    address: 192.168.1.11
    user: admin

tasks:
  - name: Show hostname
    command: hostname

  - name: Check uptime
    command: uptime
```

This approach is perfect for:
- One-off automation scripts
- Infrastructure bootstrap playbooks
- Cloud-init user data
- Container initialization

## Ansible Migration

Migrate your existing Ansible playbooks to Nexus with a single command:

```bash
# Convert a single playbook
nexus convert playbook.yml -o playbook.nx.yml

# Convert an entire project (playbooks, roles, inventory)
nexus convert ansible-project/ -o nexus-project/ --all

# Preview conversion without writing files
nexus convert playbook.yml --dry-run

# Convert with verbose output
nexus convert playbook.yml -o playbook.nx.yml --verbose
```

The `nexus convert` command automatically translates:
- Ansible playbook syntax to Nexus format
- Module names and parameters
- Variable syntax and expressions
- Conditionals and loops
- Handlers and notifications
- Role structures and dependencies

See [Ansible Migration Guide](docs/ansible-migration.md) for detailed conversion mappings and compatibility information.

## Network Discovery

Nexus includes built-in network discovery capabilities that combine the power of network scanning with automation. Think "Nmap meets Ansible" - discover hosts and immediately automate against them.

### Basic Discovery

Scan a subnet for hosts with SSH services:

```bash
# Discover all SSH hosts on the network
nexus discover --subnet 10.20.30.0/24

# Scan a smaller range
nexus discover --subnet 192.168.1.0/28
```

### Advanced Scanning

```bash
# Scan specific ports (TCP)
nexus discover --subnet 10.20.30.0/24 --probe tcp:22,80,443

# Use ping-based discovery (faster, less detailed)
nexus discover --subnet 10.20.30.0/24 --probe ping

# Enable OS fingerprinting via SSH banners
nexus discover --subnet 10.20.30.0/24 --fingerprint

# Adjust parallelism and timeouts
nexus discover --subnet 10.20.30.0/24 --parallel 100 --timeout 5
```

### Save and Use Discovered Hosts

```bash
# Save discovered hosts to an inventory file
nexus discover --subnet 10.20.30.0/24 --save-to discovered.yaml

# Then use the generated inventory
nexus run playbook.nx.yaml -i discovered.yaml
```

### Live Discovery with Playbook Execution

Discover and automate in a single command:

```bash
# Discover hosts and immediately run a playbook against them
nexus run patch-all.nx.yaml --discover 10.20.30.0/24

# Combine with other discovery options
nexus run security-scan.nx.yaml --discover 192.168.1.0/24 --fingerprint
```

### Discovery Features

- **Probe Types**:
  - `ssh` (default) - Scans for SSH services on port 22
  - `ping` - ICMP echo requests for fast host enumeration
  - `tcp:port1,port2,...` - Custom TCP port scanning

- **Hostname Resolution**: Automatic reverse DNS lookup for discovered IPs

- **OS Fingerprinting**: Detect operating system from SSH banners when `--fingerprint` is enabled

- **Concurrent Scanning**: Configurable parallelism with `--parallel N` (default: 50)

- **Timeout Control**: Set per-host timeout with `--timeout N` seconds (default: 3)

### Use Cases

- **Dynamic Cloud Environments**: Discover and configure newly-launched instances
- **Network Auditing**: Find all SSH-accessible hosts in your network
- **Mass Updates**: Discover all hosts and apply patches in one command
- **Infrastructure Mapping**: Generate inventory from live network state
- **Disaster Recovery**: Quickly rediscover and reconfigure hosts after network changes

## Why Nexus?

| Feature | Ansible | Nexus |
|---------|---------|-------|
| Language | Python | Rust |
| Parallelism | Multi-process | Async/await |
| Migration Path | N/A | Built-in (`nexus convert`) |
| Inventory-less Execution | Limited | CLI hosts, embedded hosts, implicit localhost |
| Network Discovery | External tools | Built-in (`nexus discover`) |
| Execution Planning | Limited | Terraform-style |
| Checkpoint/Resume | No | Yes |
| Circuit Breakers | No | Yes |
| Live TUI | No | Yes |
| Connection Pooling | Limited | Built-in |

## Project Structure

```
nexus/
├── src/
│   ├── main.rs          # CLI entry point
│   ├── lib.rs           # Library exports
│   ├── parser/          # Playbook parsing (YAML, expressions)
│   ├── executor/        # Task execution engine
│   ├── modules/         # Built-in modules (package, service, file, etc.)
│   ├── inventory/       # Host and group management
│   ├── output/          # Terminal, JSON, and TUI output
│   ├── runtime/         # Expression evaluation
│   ├── vault/           # Secret encryption
│   └── plugins/         # Callbacks and lookups
├── examples/            # Example playbooks and inventory
├── docs/                # Documentation
└── tests/               # Integration tests
```

## License

MIT License - see LICENSE file for details.
