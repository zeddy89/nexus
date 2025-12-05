# Migrating from Ansible to Nexus

This guide covers how to migrate your existing Ansible infrastructure to Nexus using the built-in `nexus convert` command.

## Philosophy

**Meet users where they are.**

Nobody wants to hear "rewrite your entire infrastructure codebase to try our new tool." That's a non-starter for any team with real work to do.

`nexus convert` lets you:
- Try Nexus with your existing playbooks in minutes
- Migrate incrementally (one playbook at a time)
- Keep Ansible running in parallel during transition
- See exactly what changed and why

## Quick Start

```bash
# Convert a single playbook
nexus convert playbook.yml -o playbook.nx.yml

# Preview without writing files
nexus convert playbook.yml --dry-run

# Convert an entire role
nexus convert roles/webserver/ -o nexus-roles/webserver/

# Convert a full Ansible project
nexus convert ansible-project/ -o nexus-project/ --all
```

## What Gets Converted

### Fully Supported (~95% automatic)
- Playbooks (.yml)
- Roles
- Static inventory
- Group vars / Host vars
- Jinja2 templates
- Handlers
- Tags
- `when:` conditionals
- `loop:` / `with_items:`
- `register:`
- `include_tasks:` / `import_tasks:`

### Partially Supported (may need review)
- Complex Jinja2 filter chains
- Custom modules (flagged for manual conversion)
- Lookup plugins
- Dynamic inventory scripts

### Not Converted (manual migration)
- Custom filter plugins → Rewrite as Nexus functions
- Custom action plugins → Rewrite as Nexus modules
- ansible.cfg settings → Create nexus.yml config

## CLI Reference

```bash
nexus convert <source> [options]
```

### Options

| Option | Description |
|--------|-------------|
| `-o, --output <path>` | Output file or directory |
| `--dry-run` | Show what would be converted without writing |
| `--interactive` | Approve each file conversion |
| `--all` | Convert entire project |
| `--include-inventory` | Also convert inventory files |
| `--include-templates` | Convert Jinja2 templates to Nexus syntax |
| `--keep-jinja2` | Keep Jinja2 syntax in templates |
| `--report <file>` | Write conversion report to file |
| `--strict` | Fail on any conversion warning |
| `--assess` | Assessment mode - analyze without converting |
| `-q, --quiet` | Minimal output |
| `-v, --verbose` | Detailed conversion log |

## Conversion Mappings

### Variable Syntax
| Ansible | Nexus |
|---------|-------|
| `{{ variable }}` | `${variable}` |
| `{{ var \| default('x') }}` | `${var ?? 'x'}` |
| `{{ var \| upper }}` | `${var.upper()}` |
| `{{ var \| join(',') }}` | `${var.join(',')}` |
| `{{ ansible_hostname }}` | `${host.hostname}` |
| `{{ inventory_hostname }}` | `${host.name}` |

### Conditional Expressions
| Ansible | Nexus |
|---------|-------|
| `when: var is defined` | `when: ${var != null}` |
| `when: result is changed` | `when: ${result.changed}` |
| `when: result is failed` | `when: ${result.failed}` |

### Module Mapping
| Ansible | Nexus |
|---------|-------|
| `yum/dnf/apt/package` | `package:` |
| `service/systemd` | `service:` |
| `copy` | `file: copy` |
| `template` | `file: template` |
| `file (state: directory)` | `file: mkdir` |
| `lineinfile` | `file: line` |
| `command/shell` | `command:/shell:` |
| `debug` | `log:` |
| `set_fact` | `set:` |

## Incremental Migration Strategy

### Phase 1: Test the Waters

```bash
# Convert one simple playbook
nexus convert utilities/cleanup.yml -o nexus/cleanup.nx.yml
nexus validate nexus/cleanup.nx.yml
nexus plan nexus/cleanup.nx.yml --limit test-hosts
```

### Phase 2: Convert Supporting Playbooks

```bash
# Lower risk playbooks first
nexus convert playbooks/maintenance/ -o nexus/maintenance/
```

### Phase 3: Convert Roles

```bash
nexus convert roles/common/ -o nexus-roles/common/
nexus convert roles/webserver/ -o nexus-roles/webserver/
```

### Phase 4: Convert Core Playbooks

```bash
nexus convert site.yml -o site.nx.yml
nexus convert deploy.yml -o deploy.nx.yml
```

### Phase 5: Full Cutover

```bash
nexus convert ansible-project/ -o nexus-project/ --all
```

## Running Side-by-Side

During migration, you can run both tools:

```
project/
├── ansible/           # Original Ansible playbooks
├── nexus/             # Converted Nexus playbooks
└── Makefile
```

```makefile
# Makefile
ansible-deploy:
    cd ansible && ansible-playbook -i inventory site.yml

nexus-deploy:
    cd nexus && nexus run -i inventory site.nx.yml
```

## Conversion Report

Every conversion generates a detailed report:

```
╔══════════════════════════════════════════════════════════════════╗
║                    Nexus Conversion Report                       ║
╠══════════════════════════════════════════════════════════════════╣
║  Source: ~/ansible-project/site.yml                              ║
║  Output: ~/nexus-project/site.nx.yml                             ║
╚══════════════════════════════════════════════════════════════════╝

SUMMARY
  Total tasks:        47
  ✓ Converted:        42 (89%)
  ~ Modified:          3 (6%)
  ⚠ Needs review:      2 (4%)

DETAILS
  ✓ Package module 'yum' → 'package:'
  ✓ Service module 'systemd' → 'service:'
  ~ Complex Jinja2 filter chain simplified
  ⚠ Custom module 'my_module' needs manual review
  ⚠ Lookup plugin 'custom_lookup' not supported

FILES WRITTEN
  - site.nx.yml
  - group_vars/all.yml (unchanged)
  - host_vars/web1.yml (unchanged)

NEXT STEPS
  1. Review flagged items in site.nx.yml
  2. Run: nexus validate site.nx.yml
  3. Run: nexus plan site.nx.yml -i inventory.yaml
  4. Test in non-production environment
```

## Example Conversions

### Before: Ansible Playbook

```yaml
---
- name: Configure webserver
  hosts: webservers
  become: yes
  vars:
    nginx_port: 80

  tasks:
    - name: Install nginx
      yum:
        name: nginx
        state: present

    - name: Start nginx
      service:
        name: nginx
        state: started
        enabled: yes

    - name: Deploy config
      template:
        src: nginx.conf.j2
        dest: /etc/nginx/nginx.conf
      notify: reload nginx

    - name: Debug message
      debug:
        msg: "Deployed to {{ ansible_hostname }}"

  handlers:
    - name: reload nginx
      service:
        name: nginx
        state: reloaded
```

### After: Nexus Playbook

```yaml
name: Configure webserver
hosts: webservers

vars:
  nginx_port: 80

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

  - name: Deploy config
    file: /etc/nginx/nginx.conf
    template: nginx.conf.j2
    sudo: true
    notify: reload_nginx

  - name: Debug message
    log: "Deployed to ${host.hostname}"

handlers:
  - name: reload_nginx
    service: nginx
    state: reloaded
    sudo: true
```

## Troubleshooting

### Unknown Module

If you see "Unknown module 'my_module'":

1. Check if there's a Nexus equivalent
2. Use `command:` or `shell:` as a fallback
3. Create a Nexus module plugin

**Example:**

```yaml
# Ansible
- name: Run custom module
  my_module:
    param: value

# Nexus fallback
- name: Run custom module
  shell: /usr/local/bin/my_module --param value
```

### Complex Jinja2 Expression

Move complex expressions to a functions block:

```yaml
functions: |
  def process_data(data):
      # Your logic here
      return result

tasks:
  - name: Process data
    set: processed_data
    value: ${process_data(vars.raw_data)}
```

### Custom Filter

Implement as a Nexus function or plugin:

```yaml
# Ansible
{{ items | my_custom_filter }}

# Nexus
functions: |
  def my_custom_filter(items):
      # Filter logic here
      return filtered_items

tasks:
  - name: Use filter
    set: filtered
    value: ${my_custom_filter(vars.items)}
```

### Dynamic Inventory

Convert dynamic inventory scripts to Nexus inventory plugins or use network discovery:

```bash
# Instead of Ansible dynamic inventory script
ansible-playbook site.yml -i ec2.py

# Use Nexus discovery
nexus run site.nx.yml --discover 10.0.0.0/16 --fingerprint
```

## Assessment Mode

Before converting, assess your Ansible codebase:

```bash
nexus convert ansible-project/ --assess --report assessment.txt
```

Sample assessment report:

```
╔══════════════════════════════════════════════════════════════════╗
║                    Nexus Migration Assessment                    ║
╚══════════════════════════════════════════════════════════════════╝

PROJECT OVERVIEW
  Playbooks:              23
  Roles:                  12
  Tasks:                  487
  Custom modules:         3
  Dynamic inventories:    2

COMPATIBILITY
  ✓ Fully compatible:     415 tasks (85%)
  ~ Needs minor changes:  65 tasks (13%)
  ⚠ Needs manual work:    7 tasks (2%)

CUSTOM COMPONENTS
  ⚠ Custom modules: db_migrate, cache_flush, load_balancer
  ⚠ Dynamic inventory: aws_ec2.py, azure_inventory.py
  ✓ Jinja2 templates: 34 files (compatible)

RECOMMENDATIONS
  1. Convert standard playbooks first (85% automatic)
  2. Rewrite 3 custom modules as Nexus plugins
  3. Replace dynamic inventory with --discover or static files

ESTIMATED EFFORT
  Automatic conversion:   2-4 hours
  Manual migration:       8-12 hours
  Testing:               16-24 hours
  Total:                 26-40 hours
```

## Best Practices

### Start Small

Begin with low-risk playbooks:

```bash
# Good first candidates
nexus convert playbooks/utilities/cleanup.yml
nexus convert playbooks/monitoring/check-disk.yml
nexus convert playbooks/maintenance/log-rotate.yml
```

### Validate Everything

Always validate before running:

```bash
nexus convert site.yml -o site.nx.yml
nexus validate site.nx.yml
nexus plan site.nx.yml -i inventory.yaml --check
```

### Use Version Control

Track conversion changes:

```bash
git checkout -b ansible-to-nexus-migration
nexus convert site.yml -o site.nx.yml
git add site.nx.yml
git commit -m "Convert site.yml to Nexus format"
```

### Test in Parallel

Run both tools against test environments:

```bash
# Ansible
ansible-playbook site.yml -i test-inventory.yaml

# Nexus (should produce identical results)
nexus run site.nx.yml -i test-inventory.yaml
```

### Document Customizations

Keep notes on manual changes:

```yaml
# site.nx.yml
# MIGRATION NOTES:
# - Custom filter 'to_json' replaced with json.dumps()
# - Module 'my_db_module' replaced with direct SQL via command:
# - Changed 'become: yes' to 'sudo: true'

name: Site configuration
hosts: all
# ...
```

## Getting Help

```bash
# View convert command help
nexus convert --help

# Dry run with verbose output
nexus convert playbook.yml --dry-run --verbose

# Generate detailed report
nexus convert playbook.yml -o playbook.nx.yml --report conversion.txt
```

## Additional Resources

- [Playbook Syntax](playbook-syntax.md) - Nexus playbook reference
- [Modules Reference](modules.md) - Module equivalents
- [CLI Reference](cli.md) - Full convert command options
- [Advanced Features](advanced-features.md) - Roles, vault, and more
