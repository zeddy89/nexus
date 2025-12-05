# Nexus

[![CI](https://github.com/zeddy89/nexus/actions/workflows/ci.yml/badge.svg)](https://github.com/zeddy89/nexus/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

**Next-generation infrastructure automation. Ansible, but faster and simpler.**

## Why Nexus?

If you've used Ansible, you know these pain points:

- **Slow execution** — Python's multiprocessing can't match true async parallelism
- **Verbose YAML** — Simple tasks require too much boilerplate
- **Inventory overhead** — Sometimes you just want to run against a few hosts
- **No dry-run preview** — You can't see what will change before running

Nexus fixes all of these with a fast Rust runtime and streamlined syntax.

## Installation

```sh
curl -fsSL https://raw.githubusercontent.com/zeddy89/nexus/main/scripts/install.sh | sh
```

Or install with Cargo:

```sh
cargo install nexus
```

## Quick Example

```yaml
# webserver.nx.yaml
hosts: webservers

tasks:
  - package: install nginx
  - service: enable nginx --now
  - firewall: allow http https
  - template: templates/nginx.conf -> /etc/nginx/nginx.conf
    notify: reload nginx

handlers:
  - service: reload nginx
```

Run it:

```sh
nexus run webserver.nx.yaml -i inventory.yaml

# Or without an inventory file
nexus run webserver.nx.yaml --hosts "web1,web2,web3"

# Preview changes first (like terraform plan)
nexus plan webserver.nx.yaml -i inventory.yaml
```

## Smart Actions Syntax

Nexus uses a natural, human-readable syntax:

```yaml
tasks:
  # Package management
  - package: install nginx vim git
  - package: remove apache2

  # Service control
  - service: enable nginx --now
  - service: restart postgresql

  # Firewall rules
  - firewall: allow http https ssh
  - firewall: allow 8080/tcp

  # File operations
  - file: create /etc/app/config.yaml
    content: |
      setting: value
    mode: "0644"

  - template: templates/app.conf -> /etc/app/app.conf

  # Commands
  - command: /usr/bin/myapp --init
  - shell: cat /etc/hosts | grep webserver
```

## Features

| Feature | Description |
|---------|-------------|
| **Async Execution** | True parallelism with Rust's async runtime |
| **Smart Actions** | Natural syntax like `package: install nginx` |
| **Inventory-less Mode** | Run with `--hosts` — no inventory file needed |
| **Execution Planning** | Preview changes with `nexus plan` |
| **Network Discovery** | Scan subnets with `nexus discover` |
| **Ansible Migration** | Convert playbooks with `nexus convert` |
| **Live TUI Dashboard** | Real-time monitoring with `--tui` |
| **Checkpoint/Resume** | Resume failed runs from where they stopped |
| **Vault Encryption** | AES-256-GCM encrypted secrets |

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
