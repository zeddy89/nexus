# Nexus

**Next-Generation Infrastructure Automation**

Nexus is a modern infrastructure automation tool written in Rust that addresses core limitations of Ansible while maintaining a familiar YAML-based playbook syntax. It's designed for speed, reliability, and sophisticated orchestration.

## Key Features

- **High Performance**: Built in Rust with async/await for true parallelism
- **Familiar Syntax**: YAML playbooks similar to Ansible for easy migration
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

# Run a playbook
./target/release/nexus run examples/01-hello-world.nx.yaml -i examples/inventory.yaml

# Preview changes (Terraform-style)
./target/release/nexus plan examples/05-webserver-setup.nx.yaml -i examples/inventory.yaml

# Run with live TUI dashboard
./target/release/nexus run playbook.nx.yaml -i inventory.yaml --tui
```

## Documentation

- [Getting Started](docs/getting-started.md) - Installation and first playbook
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

## Why Nexus?

| Feature | Ansible | Nexus |
|---------|---------|-------|
| Language | Python | Rust |
| Parallelism | Multi-process | Async/await |
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
