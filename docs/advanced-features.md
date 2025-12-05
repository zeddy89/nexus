# Advanced Features

This guide covers Nexus's advanced capabilities for complex automation scenarios.

## Vault (Encrypted Secrets)

Nexus uses AES-256-GCM encryption with Argon2 key derivation for secure secrets.

### Encrypting Files

```bash
# Encrypt a file (prompts for password)
nexus vault encrypt secrets.yml

# With password file
nexus vault encrypt secrets.yml --vault-password-file .vault_pass
```

### Encrypted File Format

```
$NEXUS_VAULT;1.0;AES256
[base64-encoded-encrypted-content]
```

### Using Encrypted Files

```bash
# Run playbook with vault password
nexus run playbook.yml -i inventory.yaml --ask-vault-pass

# With password file
nexus run playbook.yml -i inventory.yaml --vault-password-file .vault_pass
```

### Best Practices

- Never commit `.vault_pass` or vault passwords to git
- Use `--vault-password-file` in CI/CD pipelines
- Encrypt only sensitive files, not entire playbooks

## Checkpoints (Resume Failed Runs)

Nexus can save execution state and resume from failures.

### Enabling Checkpoints

```bash
nexus run playbook.yml -i inventory.yaml --checkpoint
```

### Resuming from Checkpoint

```bash
# Resume from last checkpoint
nexus run playbook.yml -i inventory.yaml --resume

# Resume from specific checkpoint
nexus run playbook.yml -i inventory.yaml --resume-from ~/.nexus/checkpoints/abc123.json
```

### Checkpoint Contents

- Completed tasks per host
- Variable state
- Registered results
- Handler notifications
- Playbook hash (detects modifications)

### Managing Checkpoints

```bash
# List all checkpoints
nexus checkpoint list

# View checkpoint details
nexus checkpoint show <file>

# Clean old checkpoints
nexus checkpoint clean --older-than 7
```

## Roles

Roles are reusable collections of tasks, handlers, variables, and templates.

### Role Structure

```
roles/
  webserver/
    tasks/main.yml        # Main task list
    handlers/main.yml     # Handler definitions
    defaults/main.yml     # Default variables (lowest priority)
    vars/main.yml         # Role variables (higher priority)
    templates/            # Jinja2 templates
    files/                # Static files
    meta/main.yml         # Role metadata and dependencies
```

### Using Roles

```yaml
roles:
  # Simple reference
  - common

  # With variable overrides
  - role: webserver
    vars:
      nginx_port: 8080
    tags:
      - nginx
    when: ${vars.install_nginx}
```

### Role Dependencies

In `roles/webserver/meta/main.yml`:

```yaml
dependencies:
  - common
  - role: ssl
    vars:
      cert_path: /etc/ssl/certs
```

### Role Variables

Variables are merged in this order (later overrides):

1. `defaults/main.yml` (lowest)
2. Inventory variables
3. Playbook variables
4. `vars/main.yml`
5. Role reference vars (highest)

## Retry and Circuit Breakers

### Basic Retry

```yaml
- name: Flaky operation
  command: curl http://api.example.com/health
  retry:
    attempts: 5
    delay: 10s
```

### Retry Strategies

```yaml
# Fixed delay
retry:
  attempts: 3
  delay: 5s

# Exponential backoff
retry:
  attempts: 5
  delay:
    type: exponential
    base: 1s
    max: 60s
    jitter: true    # Randomize to prevent thundering herd

# Linear backoff
retry:
  attempts: 5
  delay:
    type: linear
    base: 2s
    increment: 2s
    max: 30s
```

### Retry Conditions

```yaml
- name: Wait for service
  command: curl -f http://localhost:8080/health
  register: health
  retry:
    attempts: 30
    delay: 10s
    until: ${health.exit_code == 0}       # Success condition
    retry_when: ${health.exit_code != 0}  # Retry condition
```

### Circuit Breakers

Prevent cascading failures across hosts:

```yaml
- name: Call external API
  command: curl http://api.example.com/data
  retry:
    attempts: 3
    circuit_breaker:
      failure_threshold: 3    # Open after 3 failures
      success_threshold: 2    # Close after 2 successes
      reset_timeout: 60s      # Try again after 60s
```

## Blocks (Error Handling)

```yaml
tasks:
  - block:
      - name: Dangerous operation
        command: /opt/scripts/migrate.sh

      - name: Verify migration
        command: /opt/scripts/verify.sh

    rescue:
      - name: Rollback on failure
        command: /opt/scripts/rollback.sh

      - name: Alert team
        command: curl -X POST https://alerts.example.com/migration-failed

    always:
      - name: Cleanup temp files
        file: /tmp/migration-*
        state: absent
```

### Block Conditions

```yaml
- block:
    - name: Production tasks
      command: deploy.sh
  when: ${vars.environment == "production"}
  become: true
```

## Async Tasks

Run tasks in the background without waiting.

### Fire and Forget

```yaml
- name: Long running process
  command: /opt/scripts/backup.sh
  async: 7200     # Timeout after 2 hours
  poll: 0         # Don't wait
  register: backup_job
```

### Poll for Completion

```yaml
- name: Start migration
  command: /opt/scripts/migrate.sh
  async: 3600
  poll: 0
  register: migration

- name: Wait for migration
  async_status:
    job_id: ${migration.ansible_job_id}
  register: result
  until: ${result.finished}
  retry:
    attempts: 60
    delay: 60s
```

## Serial Execution (Rolling Updates)

Deploy to hosts in batches:

```yaml
# Fixed batch size
serial: 2                    # 2 hosts at a time

# Percentage
serial: "25%"                # 25% of hosts

# Progressive batches
serial: [1, 5, "100%"]       # 1, then 5, then rest
```

### Example Rolling Deployment

```yaml
hosts: webservers
serial: 2

tasks:
  - name: Remove from load balancer
    command: /opt/scripts/lb-remove.sh ${host.name}
    delegate_to: localhost

  - name: Deploy application
    command: /opt/scripts/deploy.sh

  - name: Health check
    command: curl -f http://localhost/health
    retry:
      attempts: 10
      delay: 5s

  - name: Add back to load balancer
    command: /opt/scripts/lb-add.sh ${host.name}
    delegate_to: localhost
```

## Delegation

Run tasks on a different host than the current target.

```yaml
- name: Update DNS on controller
  command: /opt/scripts/dns-update.sh ${host.name} ${host.address}
  delegate_to: dns-server

- name: Run locally
  command: echo "Managing ${host.name} from control machine"
  delegate_to: localhost
```

## Callback Plugins

Extend Nexus with custom callbacks.

### Using Callbacks

```bash
nexus run playbook.yml -i inventory.yaml --callback json_log:/var/log/nexus.json
```

### Built-in Callbacks

| Name | Description |
|------|-------------|
| `json_log` | Write events to JSON file |
| `timer` | Track task execution times |

### Event Types

- `playbook_start`: Playbook begins
- `play_start`: Play begins
- `task_start`: Task starts on host
- `task_complete`: Task finishes successfully
- `task_failed`: Task fails
- `task_skipped`: Task skipped
- `handler_start`: Handler triggered
- `handler_complete`: Handler finishes
- `playbook_complete`: Playbook ends with recap

## Lookup Plugins

Retrieve data from external sources.

```yaml
vars:
  # Read file contents
  ssh_key: ${lookup('file', '~/.ssh/id_ed25519.pub')}

  # Get environment variable
  home_dir: ${lookup('env', 'HOME')}

  # Execute command
  current_date: ${lookup('pipe', 'date +%Y-%m-%d')}

  # Generate/retrieve password
  db_password: ${lookup('password', '/tmp/db_pass length=32')}

  # Render template string
  greeting: ${lookup('template', 'Hello {{ vars.name }}!')}

  # Find first existing file
  config: ${lookup('first_found', ['config.local.yml', 'config.yml'])}
```

## TUI Dashboard

Real-time execution monitoring.

```bash
nexus run playbook.yml -i inventory.yaml --tui
```

### Dashboard Features

- Host status indicators (waiting, running, ok, failed)
- Current task per host
- Progress tracking
- Live output display

### Status Symbols

| Symbol | Color | Meaning |
|--------|-------|---------|
| ○ | Gray | Waiting |
| ⟳ | Yellow | Running |
| ✓ | Green | Completed |
| ✗ | Red | Failed |

## Execution Planning (Terraform-style)

Preview changes before applying:

```bash
nexus plan playbook.yml -i inventory.yaml --diff
```

### Plan Output

```
Plan: 5 to add, 2 to change, 1 to destroy.

+ [web1] Install nginx
+ [web1] Create /var/www/html
~ [web1] Update /etc/nginx/nginx.conf
  - server_name: old.example.com
  + server_name: new.example.com
- [web1] Remove /etc/nginx/sites-enabled/default

Do you want to apply these changes? [y/N]
```

### Auto-approve

```bash
nexus plan playbook.yml -i inventory.yaml -y
```

## JSON Output

Machine-readable output for CI/CD pipelines.

```bash
nexus run playbook.yml -i inventory.yaml --output-format json
```

### Output Format (NDJSON)

Each line is a JSON object:

```json
{"timestamp":"2025-01-01T12:00:00Z","event":"playbook_start","playbook":"site.yml","hosts_count":5}
{"timestamp":"2025-01-01T12:00:01Z","event":"task_complete","host":"web1","task":"Install nginx","status":"changed"}
{"timestamp":"2025-01-01T12:05:00Z","event":"playbook_complete","total_duration_ms":300000,"has_failures":false}
```
