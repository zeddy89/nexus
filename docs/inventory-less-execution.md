# Inventory-less Execution

Nexus supports running playbooks without traditional inventory files, making it ideal for dynamic environments, one-off tasks, and rapid deployment scenarios.

## Overview

Instead of maintaining static inventory files, Nexus can target hosts through:

1. Live network discovery (`--discover` flag)
2. Explicit host lists (`--hosts` / `-H` flag)
3. Playbook-embedded host definitions
4. Implicit localhost execution

## Host Source Priority

When multiple host sources are available, Nexus uses this priority order (highest to lowest):

```
1. CLI --discover flag       (live network scan)
2. CLI --hosts / -H flag     (explicit host list)
3. Inventory file            (--inventory / -i)
4. Playbook-embedded hosts   (hosts: list in playbook)
5. Implicit localhost        (hosts: localhost)
```

**Priority examples:**

```bash
# Priority 1: Discovery overrides everything
nexus run playbook.yml -i inventory.yaml --hosts server1,server2 --discover 192.168.1.0/24
# Result: Runs against discovered hosts (ignores inventory and --hosts)

# Priority 2: Explicit hosts override inventory and playbook
nexus run playbook.yml -i inventory.yaml --hosts server1,server2
# Result: Runs against server1 and server2 only

# Priority 3: Inventory file used
nexus run playbook.yml -i inventory.yaml
# Result: Uses inventory file hosts

# Priority 4: Playbook-embedded hosts (when no inventory specified)
nexus run playbook.yml
# Result: Uses hosts defined in playbook

# Priority 5: Implicit localhost
# (Special case when playbook has hosts: localhost)
```

## Method 1: Network Discovery

Execute playbooks against live-discovered hosts without creating inventory files.

### Basic Discovery Execution

```bash
# Discover and run
nexus run playbook.yml --discover 192.168.1.0/24
```

This performs:
1. Network scan of 192.168.1.0/24
2. Host discovery via SSH probe
3. Playbook execution on discovered hosts

### Discovery with Options

```bash
# With probe type and timeout
nexus run playbook.yml --discover 192.168.1.0/24 --probe ssh --timeout 5s

# With OS fingerprinting
nexus run playbook.yml --discover 192.168.1.0/24 --fingerprint

# With specific probe ports
nexus run playbook.yml --discover 192.168.1.0/24 --probe tcp:22,80,443
```

### Multiple Subnets

```bash
# Discover from multiple networks
nexus run playbook.yml --discover 192.168.1.0/24,10.0.0.0/24,172.16.0.0/20
```

### Use Cases

- Cloud auto-scaling groups
- Container orchestration environments
- Testing labs with dynamic IPs
- Ad-hoc network maintenance

**Example: Cloud deployment**

```bash
# Deploy to all instances in VPC
nexus run deploy.yml --discover 172.31.0.0/16 --probe ssh --timeout 3s
```

See [Network Discovery](network-discovery.md) for complete discovery options.

## Method 2: Explicit Host List

Specify hosts directly on the command line.

### Single Host

```bash
nexus run playbook.yml --hosts 192.168.1.10
# or
nexus run playbook.yml -H 192.168.1.10
```

### Multiple Hosts

```bash
# Comma-separated IPs
nexus run playbook.yml --hosts 192.168.1.10,192.168.1.11,192.168.1.12

# Mix of IPs and hostnames
nexus run playbook.yml --hosts web1.example.com,192.168.1.10,db.example.com
```

### With Connection Details

```bash
# Specify SSH user and key
nexus run playbook.yml \
  --hosts 192.168.1.10,192.168.1.11 \
  --user admin \
  --private-key ~/.ssh/id_ed25519
```

### Use Cases

- One-off administrative tasks
- Emergency fixes on specific servers
- Testing playbooks on select hosts
- Quick deployments without inventory setup

**Example: Emergency patch**

```bash
# Apply security patch to specific servers
nexus run security-patch.yml \
  --hosts prod-web-01,prod-web-02,prod-web-03 \
  --user admin \
  --sudo \
  -K
```

## Method 3: Playbook-embedded Hosts

Define hosts directly within playbook files.

### Basic Syntax

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
    port: 2222              # Custom SSH port
    vars:
      role: webserver
      environment: production

  - name: db1
    address: 192.168.1.20
    user: dbadmin
    port: 22
    vars:
      role: database
      environment: production

tasks:
  - name: Show host info
    command: echo "Host ${host.name} (${host.vars.role}) on port ${host.port}"
```

### Accessing Embedded Host Variables

```yaml
hosts:
  - name: app1
    address: 10.0.1.10
    user: deploy
    vars:
      app_port: 8080
      app_env: production
      app_version: "2.1.0"

tasks:
  - name: Deploy application
    command: /opt/deploy.sh ${host.vars.app_version}

  - name: Configure app
    file: /etc/app/config.json
    content: |
      {
        "port": ${host.vars.app_port},
        "environment": "${host.vars.app_env}"
      }
```

### Mixed Pattern Matching

You can still use host patterns with embedded hosts:

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
```

### Use Cases

- Playbook-specific infrastructure
- Self-contained deployment scripts
- Portable playbooks (no external dependencies)
- Documentation-as-code (hosts visible in playbook)

**Example: Portable deployment**

```yaml
# deploy-to-staging.yml - completely self-contained
hosts:
  - name: staging-web
    address: staging.example.com
    user: deploy
    vars:
      service: web
  - name: staging-api
    address: api-staging.example.com
    user: deploy
    vars:
      service: api

vars:
  version: "1.5.0"
  environment: staging

tasks:
  - name: Deploy ${host.vars.service}
    command: /opt/deploy.sh ${vars.version} ${vars.environment}
```

## Method 4: Implicit Localhost

When a playbook specifies `hosts: localhost`, Nexus automatically targets the local machine.

### Basic Localhost Execution

```yaml
hosts: localhost

tasks:
  - name: Local task
    command: echo "Running on local machine"

  - name: Create local file
    file: /tmp/test.txt
    content: "Hello from Nexus"
```

**Run without inventory:**

```bash
nexus run local-playbook.yml
# No -i flag needed - runs on localhost automatically
```

### Localhost with Connection Type

```yaml
hosts: localhost
connection: local    # Skip SSH, execute directly

tasks:
  - name: Fast local execution
    command: date
```

### Use Cases

- Local system configuration
- Workstation setup scripts
- Build/deployment orchestration from control machine
- Pre-flight checks before remote execution

**Example: Workstation setup**

```yaml
# setup-workstation.yml
hosts: localhost
connection: local

tasks:
  - name: Install development tools
    package:
      name:
        - git
        - vim
        - tmux
        - docker
      state: present
    sudo: true

  - name: Configure git
    command: git config --global user.name "Developer"

  - name: Clone repositories
    git:
      repo: https://github.com/example/project.git
      dest: ~/projects/project
```

**Run it:**

```bash
nexus run setup-workstation.yml --sudo -K
```

## Combining Methods

### Discovery with Host Filtering

```bash
# Discover hosts, then limit to specific ones
nexus run playbook.yml --discover 192.168.1.0/24 --limit 192.168.1.10,192.168.1.11
```

### Embedded Hosts with CLI Override

```yaml
# playbook.yml with embedded hosts
hosts:
  - name: default1
    address: 192.168.1.10
```

```bash
# Override with CLI hosts (priority 2 > priority 4)
nexus run playbook.yml --hosts 192.168.1.20,192.168.1.21
# Result: Runs against 192.168.1.20 and 192.168.1.21, not embedded hosts
```

### Discovery with Inventory Fallback

```bash
# Try discovery, fall back to inventory if discovery fails
nexus run playbook.yml --discover 192.168.1.0/24 -i fallback-inventory.yaml
```

## Dynamic Execution Patterns

### Pattern 1: Cloud Auto-Discovery

```bash
# Discover and deploy in ephemeral cloud environments
#!/bin/bash
SUBNET=$(curl -s http://169.254.169.254/latest/meta-data/subnet-cidr-block)
nexus run deploy.yml --discover $SUBNET --probe ssh --timeout 3s
```

### Pattern 2: Ansible Migration

Existing Ansible users can gradually migrate:

```bash
# Stage 1: Use Ansible inventory
nexus run playbook.yml -i ansible-inventory.yaml

# Stage 2: Migrate to embedded hosts
nexus run playbook-with-hosts.yml

# Stage 3: Use discovery for dynamic environments
nexus run playbook.yml --discover 192.168.1.0/24
```

### Pattern 3: Multi-Environment Playbooks

```yaml
# deploy.yml - environment selected at runtime
hosts:
  - name: prod-web
    address: prod.example.com
    user: deploy
    vars:
      env: production
  - name: staging-web
    address: staging.example.com
    user: deploy
    vars:
      env: staging
  - name: dev-web
    address: dev.example.com
    user: deploy
    vars:
      env: development

tasks:
  - name: Deploy to ${host.vars.env}
    command: /opt/deploy.sh ${host.vars.env}
```

```bash
# Deploy to specific environment
nexus run deploy.yml --limit prod-web     # Production only
nexus run deploy.yml --limit staging-web  # Staging only
nexus run deploy.yml                      # All environments
```

### Pattern 4: Hybrid Approach

```yaml
# production.yml - uses inventory
hosts: production_servers
tasks:
  - name: Production deployment
    command: /opt/deploy.sh production
```

```yaml
# testing.yml - uses embedded hosts
hosts:
  - name: test-vm
    address: 192.168.100.10
    user: tester
tasks:
  - name: Test deployment
    command: /opt/deploy.sh testing
```

```bash
# Production: requires inventory
nexus run production.yml -i production-inventory.yaml

# Testing: self-contained
nexus run testing.yml
```

## Best Practices

### When to Use Discovery

**Good for:**
- Cloud auto-scaling environments
- Container orchestration
- Dynamic lab environments
- Network maintenance tasks

**Avoid when:**
- Hosts rarely change
- Need precise host targeting
- Complex host grouping required
- Regulatory compliance requires static inventory

### When to Use Explicit Hosts

**Good for:**
- One-off tasks
- Emergency fixes
- Small host counts (< 10 hosts)
- Ad-hoc testing

**Avoid when:**
- Managing many hosts regularly
- Need host grouping
- Sharing playbooks with team
- Hosts change frequently

### When to Use Embedded Hosts

**Good for:**
- Playbook portability
- Self-documenting infrastructure
- Small, stable host sets
- Environment-specific deployments

**Avoid when:**
- Managing large infrastructure
- Multiple playbooks share hosts
- Hosts change frequently
- Need central inventory management

### When to Use Traditional Inventory

**Good for:**
- Large infrastructure (> 50 hosts)
- Complex host grouping
- Shared infrastructure across teams
- Static, long-lived servers
- Role-based access control

**Avoid when:**
- Hosts are highly dynamic
- Simple, one-off tasks
- Rapid prototyping
- Self-contained playbooks needed

## Migration Guide

### From Ansible with Static Inventory

**Before (Ansible):**

```bash
ansible-playbook site.yml -i inventory.ini
```

**After (Nexus - Option 1: Keep inventory):**

```bash
nexus run site.yml -i inventory.yaml
```

**After (Nexus - Option 2: Use discovery):**

```bash
nexus run site.yml --discover 192.168.1.0/24
```

**After (Nexus - Option 3: Embed hosts):**

```yaml
# site.yml
hosts:
  - name: web1
    address: 192.168.1.10
  - name: web2
    address: 192.168.1.11

tasks:
  # ... existing tasks
```

```bash
nexus run site.yml  # No inventory needed
```

### From Terraform to Nexus

Use discovery to configure infrastructure provisioned by Terraform:

```bash
# After terraform apply
SUBNET=$(terraform output -raw vpc_cidr)
nexus run configure.yml --discover $SUBNET
```

### From Docker Compose to Nexus

Configure containers after compose startup:

```bash
# Start containers
docker-compose up -d

# Discover container IPs
SUBNET=$(docker network inspect myapp_default -f '{{range .IPAM.Config}}{{.Subnet}}{{end}}')

# Configure containers
nexus run configure-containers.yml --discover $SUBNET --probe tcp:22
```

## Troubleshooting

### Priority Confusion

**Problem:** Playbook runs against unexpected hosts

**Solution:** Check priority order - CLI flags override everything

```bash
# Verify which hosts will be targeted
nexus run playbook.yml --discover 192.168.1.0/24 -v
# Output shows: "Using discovery (priority 1)"

nexus run playbook.yml --hosts server1,server2 -i inventory.yaml -v
# Output shows: "Using explicit hosts (priority 2)"
```

### Embedded Hosts Not Working

**Problem:** Playbook still requires inventory

**Solution:** Ensure playbook hosts format is correct

```yaml
# Wrong (string pattern - requires inventory)
hosts: webservers

# Correct (list format - embedded hosts)
hosts:
  - name: web1
    address: 192.168.1.10
```

### Discovery Finds No Hosts

**Problem:** `--discover` returns empty host list

**Solution:** See [Network Discovery Troubleshooting](network-discovery.md#troubleshooting)

## See Also

- [Network Discovery](network-discovery.md) - Complete discovery documentation
- [Inventory Guide](inventory.md) - Traditional inventory management
- [CLI Reference](cli.md) - Command-line options
- [Playbook Syntax](playbook-syntax.md) - Playbook format reference
