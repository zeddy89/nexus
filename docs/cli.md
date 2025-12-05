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

### nexus run

Execute a playbook.

```bash
nexus run <PLAYBOOK> [OPTIONS]

Arguments:
  <PLAYBOOK>  Path to the playbook file

Required Options:
  -i, --inventory <FILE>  Path to inventory file

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

Advanced Options:
      --checkpoint            Enable checkpoints for resume
      --resume                Resume from last checkpoint
      --resume-from <FILE>    Resume from specific checkpoint
      --callback <SPEC>       Load callback plugin (repeatable)
      --tui                   Enable live TUI dashboard
```

**Examples:**

```bash
# Basic execution
nexus run site.yml -i inventory.yaml

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
