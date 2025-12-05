# Inventory Guide

The inventory defines which hosts Nexus manages and how to connect to them.

## Static Inventory (YAML)

### Simple Format

```yaml
defaults:
  user: admin

all:
  children:
    webservers:
      hosts:
        web1:
          ansible_host: 192.168.1.10
        web2:
          ansible_host: 192.168.1.11
      vars:
        http_port: 80

    databases:
      hosts:
        db1:
          ansible_host: 192.168.1.20
          ansible_user: postgres
```

### Full Example

```yaml
# Default SSH user for all hosts
defaults:
  user: ubuntu

all:
  children:
    # Web servers group
    webservers:
      hosts:
        web1:
          ansible_host: 192.168.1.10
          ansible_port: 22
          environment: production
        web2:
          ansible_host: 192.168.1.11
          environment: production
        web3:
          ansible_host: 192.168.1.12
          environment: staging
      vars:
        http_port: 80
        document_root: /var/www/html

    # Database servers group
    databases:
      hosts:
        db1:
          ansible_host: 192.168.1.20
          role: primary
        db2:
          ansible_host: 192.168.1.21
          role: replica
      vars:
        postgres_version: 15

    # Load balancers
    loadbalancers:
      hosts:
        lb1:
          ansible_host: 192.168.1.5
      vars:
        vip: 192.168.1.100

    # Logical groupings (children only, no direct hosts)
    production:
      children:
        - webservers
        - databases
      vars:
        monitoring_enabled: true

    staging:
      hosts:
        staging1:
          ansible_host: 192.168.2.10
```

## Host Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ansible_host` | IP address or hostname | Host name |
| `ansible_port` | SSH port | 22 |
| `ansible_user` | SSH username | From defaults |
| `ansible_connection` | Connection type | ssh |

Custom variables can be added and accessed via `${host.vars.variable_name}`.

## Host Patterns

Target specific hosts or groups in playbooks:

```yaml
# All hosts
hosts: all

# Single group
hosts: webservers

# Multiple groups (union)
hosts: webservers:databases

# Intersection (hosts in both groups)
hosts: webservers:&production

# Exclusion (webservers except staging)
hosts: webservers:!staging

# Combine patterns
hosts: webservers:databases:&production:!maintenance
```

### Limiting at Runtime

```bash
# Limit to specific hosts
nexus run playbook.yml -i inventory.yaml --limit web1,web2

# Limit to a group
nexus run playbook.yml -i inventory.yaml --limit webservers
```

## Dynamic Inventory

Nexus supports executable scripts that return JSON inventory data.

### Script Requirements

1. Must be executable (`chmod +x inventory.sh`)
2. Must accept `--list` argument
3. Must return valid JSON

### Script Example

```bash
#!/bin/bash
# dynamic-inventory.sh

if [ "$1" = "--list" ]; then
cat <<'EOF'
{
  "webservers": {
    "hosts": ["web1", "web2"],
    "vars": {
      "http_port": 80
    }
  },
  "databases": {
    "hosts": ["db1"],
    "vars": {
      "db_port": 5432
    }
  },
  "_meta": {
    "hostvars": {
      "web1": {
        "ansible_host": "192.168.1.10",
        "ansible_user": "ubuntu"
      },
      "web2": {
        "ansible_host": "192.168.1.11",
        "ansible_user": "ubuntu"
      },
      "db1": {
        "ansible_host": "192.168.1.20",
        "ansible_user": "postgres"
      }
    }
  }
}
EOF
fi
```

### Using Dynamic Inventory

```bash
nexus run playbook.yml -i ./dynamic-inventory.sh
```

### JSON Format Reference

```json
{
  "group_name": {
    "hosts": ["host1", "host2"],
    "vars": {
      "group_var": "value"
    },
    "children": ["child_group"]
  },
  "_meta": {
    "hostvars": {
      "host1": {
        "ansible_host": "192.168.1.10",
        "custom_var": "value"
      }
    }
  }
}
```

## Variable Inheritance

Variables are inherited in this order (later overrides earlier):

1. `all` group vars
2. Parent group vars
3. Direct group vars
4. Host-specific vars

```yaml
all:
  vars:
    environment: default    # Lowest priority
  children:
    production:
      vars:
        environment: prod   # Overrides all
      children:
        webservers:
          vars:
            http_port: 80   # Adds to production vars
          hosts:
            web1:
              environment: prod-special  # Highest priority
```

## Playbook-embedded Hosts

As an alternative to inventory files, you can define hosts directly in your playbooks. This is useful for self-contained playbooks, environment-specific configurations, or when managing small, stable host sets.

### Basic Syntax

Instead of using a pattern (like `hosts: all` or `hosts: webservers`), provide a list of host definitions:

```yaml
hosts:
  - name: web-server-01
    address: 192.168.1.10
    user: admin
  - name: web-server-02
    address: 192.168.1.11
    user: admin
  - name: db-server-01
    address: 192.168.1.20
    user: postgres

tasks:
  - name: Configure servers
    command: echo "Configuring ${host.name}"
```

### With Optional Parameters

```yaml
hosts:
  - name: web1
    address: 192.168.1.10
    user: deploy
    port: 2222              # Custom SSH port (default: 22)
    vars:
      role: webserver
      environment: production

  - name: db1
    address: 192.168.1.20
    user: dbadmin
    port: 22
    vars:
      role: database
      db_port: 5432

tasks:
  - name: Access host variables
    command: echo "${host.name} is a ${host.vars.role} in ${host.vars.environment}"
```

### Host Parameters

| Parameter | Description | Required | Default |
|-----------|-------------|----------|---------|
| `name` | Host identifier (used in task output) | Yes | - |
| `address` | IP address or hostname | Yes | - |
| `user` | SSH username | No | Current user |
| `port` | SSH port | No | 22 |
| `vars` | Custom host-specific variables | No | {} |

### Accessing Embedded Host Variables

Custom variables defined in the `vars` section can be accessed using `${host.vars.variable_name}`:

```yaml
hosts:
  - name: app-server
    address: 10.0.1.10
    user: deploy
    vars:
      app_port: 8080
      app_env: production
      app_version: "2.1.0"
      ssl_enabled: true

tasks:
  - name: Deploy application
    command: /opt/deploy.sh --version ${host.vars.app_version}

  - name: Configure application
    file: /etc/app/config.json
    content: |
      {
        "port": ${host.vars.app_port},
        "environment": "${host.vars.app_env}",
        "ssl": ${host.vars.ssl_enabled}
      }
```

### Using with Host Patterns

Even with embedded hosts, you can still use conditional execution based on variables:

```yaml
hosts:
  - name: web1
    address: 192.168.1.10
    vars:
      group: webservers
  - name: web2
    address: 192.168.1.11
    vars:
      group: webservers
  - name: db1
    address: 192.168.1.20
    vars:
      group: databases

tasks:
  - name: Only on web servers
    command: systemctl restart nginx
    when: ${host.vars.group == "webservers"}

  - name: Only on database servers
    command: systemctl restart postgresql
    when: ${host.vars.group == "databases"}
```

### Running Playbooks with Embedded Hosts

No inventory file is needed:

```bash
# Just run the playbook directly
nexus run playbook.yml

# With connection options
nexus run playbook.yml --private-key ~/.ssh/id_ed25519

# Limit to specific hosts
nexus run playbook.yml --limit web1,web2
```

### When to Use Embedded Hosts

**Good for:**
- Self-contained, portable playbooks
- Environment-specific deployments (dev, staging, prod)
- Small host sets (< 20 hosts)
- Playbooks with dedicated infrastructure
- Documentation-as-code approaches

**Avoid when:**
- Managing large infrastructure (> 50 hosts)
- Multiple playbooks share the same hosts
- Need complex host grouping and inheritance
- Want central inventory management

See [Inventory-less Execution](inventory-less-execution.md) for more details on running playbooks without inventory files.

## Viewing Inventory

```bash
# List all hosts
nexus inventory -i inventory.yaml

# List hosts in a group
nexus inventory -i inventory.yaml webservers

# Show host variables
nexus inventory -i inventory.yaml --vars
```
