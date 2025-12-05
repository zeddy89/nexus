# CLI Reference

Complete reference for all Nexus command-line options.

## Global Options

```bash
nexus [OPTIONS] <COMMAND>

Options:
  -v, --verbose        Enable verbose output
  -q, --quiet          Quiet mode - only show errors
      --output-format  Output format: text (default) or json
  -h, --help           Print help
  -V, --version        Print version
```

## Commands

- [nexus run](#nexus-run) - Execute a playbook
- [nexus validate](#nexus-validate) - Validate playbook syntax
- [nexus plan](#nexus-plan) - Preview changes before applying
- [nexus parse](#nexus-parse) - Display parsed playbook structure
- [nexus inventory](#nexus-inventory) - List hosts in inventory
- [nexus vault](#nexus-vault) - Manage encrypted secrets
- [nexus checkpoint](#nexus-checkpoint) - Manage execution checkpoints
- [nexus convert](#nexus-convert) - Convert Ansible playbooks to Nexus format
- [nexus discover](#nexus-discover) - Discover hosts on the network

### nexus run

Execute a playbook.

```bash
nexus run <PLAYBOOK> [OPTIONS]

Arguments:
  <PLAYBOOK>  Path to the playbook file

Inventory Options (one or none required):
  -i, --inventory <FILE>  Path to inventory file
  -H, --hosts <HOSTS>     Comma-separated host list (IPs or hostnames)
      --discover <SUBNET> Discover hosts via network scan (CIDR notation)

Connection Options:
  -u, --user <USER>           SSH user (overrides inventory)
      --password <PASSWORD>   SSH password (insecure)
  -k, --ask-pass              Prompt for SSH password
      --private-key <FILE>    Path to SSH private key
      --timeout <SECONDS>     SSH connection timeout [default: 30]

Execution Options:
  -c, --check                 Dry run - don't make changes
  -D, --diff                  Show file differences
      --forks <N>             Max parallel hosts [default: 10]
  -l, --limit <PATTERN>       Limit to specific hosts
  -s, --sudo                  Run all tasks with sudo
  -K, --ask-sudo-pass         Prompt for sudo password

Tag Options:
  -t, --tags <TAGS>           Only run tasks with these tags
      --skip-tags <TAGS>      Skip tasks with these tags

Vault Options:
      --vault-password <PWD>       Vault password
      --vault-password-file <FILE> File containing vault password
      --ask-vault-pass             Prompt for vault password

Discovery Options (when using --discover):
      --probe <TYPE>          Probe type: ssh, ping, or tcp:port1,port2 [default: ssh]
      --fingerprint           Enable OS and service fingerprinting
      --timeout <DURATION>    Connection timeout per host [default: 2s]
      --parallel <N>          Max concurrent probe connections [default: 100]

Advanced Options:
      --checkpoint            Enable checkpoints for resume
      --resume                Resume from last checkpoint
      --resume-from <FILE>    Resume from specific checkpoint
      --callback <SPEC>       Load callback plugin (repeatable)
      --tui                   Enable live TUI dashboard
```

**Examples:**

```bash
# Basic execution with inventory
nexus run site.yml -i inventory.yaml

# Inventory-less: explicit host list
nexus run site.yml --hosts 192.168.1.10,192.168.1.11

# Inventory-less: network discovery
nexus run site.yml --discover 192.168.1.0/24

# Discovery with custom probe
nexus run site.yml --discover 192.168.1.0/24 --probe tcp:22,80 --timeout 5s

# With SSH key and verbose output
nexus run site.yml -i inventory.yaml --private-key ~/.ssh/id_ed25519 -v

# Dry run with diff
nexus run site.yml -i inventory.yaml --check --diff

# Run specific tags with sudo
nexus run site.yml -i inventory.yaml -t deploy,config -s -K

# Run with TUI dashboard
nexus run site.yml -i inventory.yaml --tui

# Resume interrupted playbook
nexus run site.yml -i inventory.yaml --resume
```

### nexus validate

Validate playbook syntax without executing.

```bash
nexus validate <PLAYBOOK>
```

**Example:**

```bash
nexus validate site.yml
# Output: ✓ Playbook is valid
#   Hosts: All
#   Tasks: 15
#   Handlers: 3
```

### nexus plan

Preview changes before applying (Terraform-style).

```bash
nexus plan <PLAYBOOK> [OPTIONS]

Required Options:
  -i, --inventory <FILE>  Path to inventory file

Options:
  -l, --limit <PATTERN>       Limit to specific hosts
  -u, --user <USER>           SSH user
  -k, --ask-pass              Prompt for SSH password
      --private-key <FILE>    SSH private key
      --diff                  Show full diffs
  -y, --yes                   Auto-approve (skip confirmation)
  -s, --sudo                  Run with sudo
      --vault-password <PWD>  Vault password
      --ask-vault-pass        Prompt for vault password
```

**Example:**

```bash
nexus plan site.yml -i inventory.yaml --diff

# Output shows:
# + Tasks to add/create
# ~ Tasks that will change
# - Tasks that will remove
# Then prompts: "Do you want to apply these changes?"
```

### nexus parse

Display parsed playbook structure.

```bash
nexus parse <PLAYBOOK> [OPTIONS]

Options:
  -f, --format <FORMAT>  Output format: yaml (default) or json
```

### nexus inventory

List hosts in inventory.

```bash
nexus inventory [OPTIONS]

Required Options:
  -i, --inventory <FILE>  Path to inventory file

Options:
  <PATTERN>   Host pattern to match [default: all]
  --vars      Show host variables
```

**Examples:**

```bash
# List all hosts
nexus inventory -i inventory.yaml

# List specific group
nexus inventory -i inventory.yaml webservers

# Show variables
nexus inventory -i inventory.yaml --vars
```

### nexus vault

Manage encrypted secrets.

```bash
nexus vault <SUBCOMMAND>

Subcommands:
  encrypt  Encrypt a file
  decrypt  Decrypt a file
  view     View decrypted content without modifying
```

**vault encrypt:**

```bash
nexus vault encrypt <FILE> [OPTIONS]

Options:
      --vault-password <PWD>       Vault password
      --vault-password-file <FILE> Password file
  -o, --output <FILE>              Output file (default: overwrite)
```

**vault decrypt:**

```bash
nexus vault decrypt <FILE> [OPTIONS]

Options:
      --vault-password <PWD>       Vault password
      --vault-password-file <FILE> Password file
  -o, --output <FILE>              Output file (default: overwrite)
```

**vault view:**

```bash
nexus vault view <FILE> [OPTIONS]

Options:
      --vault-password <PWD>       Vault password
      --vault-password-file <FILE> Password file
```

**Examples:**

```bash
# Encrypt a file (prompts for password)
nexus vault encrypt secrets.yml

# Decrypt with password file
nexus vault decrypt secrets.yml --vault-password-file .vault_pass

# View without decrypting file
nexus vault view secrets.yml --vault-password "mypassword"
```

### nexus checkpoint

Manage execution checkpoints.

```bash
nexus checkpoint <SUBCOMMAND>

Subcommands:
  list   List all saved checkpoints
  show   Show checkpoint details
  clean  Delete checkpoints
```

**checkpoint list:**

```bash
nexus checkpoint list
```

**checkpoint show:**

```bash
nexus checkpoint show <FILE>
```

**checkpoint clean:**

```bash
nexus checkpoint clean [OPTIONS]

Options:
  <PLAYBOOK>           Clean checkpoint for specific playbook
  --older-than <DAYS>  Delete checkpoints older than N days
```

**Examples:**

```bash
# List all checkpoints
nexus checkpoint list

# Show checkpoint details
nexus checkpoint show ~/.nexus/checkpoints/abc123.json

# Clean old checkpoints
nexus checkpoint clean --older-than 7

# Clean specific playbook checkpoint
nexus checkpoint clean site.yml
```

### nexus convert

Convert Ansible playbooks and roles to Nexus format.

#### Synopsis

```bash
nexus convert <source> [options]
```

#### Arguments

| Argument | Description |
|----------|-------------|
| `<source>` | Ansible playbook file or directory to convert |

#### Options

| Option | Description |
|--------|-------------|
| `-o, --output <path>` | Output file or directory |
| `--dry-run` | Preview conversion without writing files |
| `--interactive` | Approve each file conversion |
| `--all` | Convert entire project (playbooks, roles, inventory) |
| `--include-inventory` | Also convert inventory files |
| `--include-templates` | Convert Jinja2 templates to Nexus syntax |
| `--keep-jinja2` | Keep Jinja2 syntax in templates |
| `--report <file>` | Write detailed conversion report to file |
| `--strict` | Fail on any conversion warning |
| `--assess` | Assessment mode - analyze without converting |
| `-q, --quiet` | Minimal output |
| `-v, --verbose` | Detailed conversion log |

#### Examples

```bash
# Convert single playbook
nexus convert site.yml -o site.nx.yml

# Dry run to preview changes
nexus convert playbook.yml --dry-run

# Convert entire Ansible project
nexus convert ~/ansible/ -o ~/nexus/ --all

# Convert with detailed report
nexus convert site.yml -o site.nx.yml --report conversion-report.md

# Assessment mode (analyze without converting)
nexus convert ansible-project/ --assess

# Strict mode for CI/CD
nexus convert playbooks/ -o nexus/ --strict --quiet
```

#### Conversion Mappings

The converter automatically translates:

| Ansible | Nexus |
|---------|-------|
| `{{ variable }}` | `${variable}` |
| `{{ var \| default('x') }}` | `${var ?? 'x'}` |
| `yum/apt/dnf` | `package:` |
| `service/systemd` | `service:` |
| `copy` | `file: copy` |
| `template` | `file: template` |
| `debug` | `log:` |
| `shell` | `shell:` |
| `command` | `command:` |

#### See Also

- [Ansible Migration Guide](ansible-migration.md) for complete conversion documentation

### nexus discover

Discover hosts on the network via scanning.

```bash
nexus discover [OPTIONS]

Required Options:
  --subnet <CIDR>  Subnet to scan (e.g., 192.168.1.0/24)

Probe Options:
  --probe <TYPE>          Probe type: ssh, ping, or tcp:port1,port2 [default: ssh]
  --fingerprint           Enable OS and service fingerprinting
  --timeout <DURATION>    Connection timeout per host [default: 2s]
  --parallel <N>          Max concurrent probe connections [default: 100]

Output Options:
  --save-to <FILE>        Save discovered hosts to inventory file

Daemon Options:
  --daemon                Run as continuous monitoring daemon
  --watch <SUBNETS>       Comma-separated subnets to watch in daemon mode
  --interval <DURATION>   Scan interval for daemon mode [default: 5m]
  --notify-on-change <SPEC>  Notification method: webhook:URL, file:PATH, or stdout
```

**Examples:**

```bash
# Basic subnet scan
nexus discover --subnet 192.168.1.0/24

# Scan with ping probe
nexus discover --subnet 192.168.1.0/24 --probe ping

# Scan specific TCP ports
nexus discover --subnet 192.168.1.0/24 --probe tcp:22,80,443

# Enable OS fingerprinting
nexus discover --subnet 192.168.1.0/24 --fingerprint

# Save to inventory file
nexus discover --subnet 192.168.1.0/24 --save-to inventory.yaml

# Fast scan (aggressive)
nexus discover --subnet 10.0.0.0/16 --timeout 500ms --parallel 200

# Continuous monitoring daemon
nexus discover --daemon --watch 192.168.1.0/24,10.0.0.0/24 --interval 5m

# Daemon with webhook notifications
nexus discover --daemon \
  --watch 192.168.1.0/24 \
  --interval 10m \
  --notify-on-change webhook:https://alerts.example.com/network

# Daemon with file logging
nexus discover --daemon \
  --watch 192.168.1.0/24 \
  --interval 5m \
  --notify-on-change file:/var/log/nexus-discovery.log
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error (parse error, invalid arguments) |
| 2 | Task failure (one or more tasks failed) |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `NEXUS_VAULT_PASSWORD` | Default vault password |
| `NEXUS_INVENTORY` | Default inventory file |
| `NO_COLOR` | Disable colored output |
| `TERM` | Terminal type (affects color support) |
