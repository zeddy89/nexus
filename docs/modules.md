# Modules Reference

Nexus includes built-in modules for common infrastructure tasks. All modules support `sudo: true` for privilege escalation and work in both check mode (`--check`) and diff mode (`--diff`).

## Command Module

Execute shell commands on remote hosts.

```yaml
- name: Run a command
  command: echo "Hello World"

- name: Command with creates (idempotent)
  command: npm install
  creates: /path/to/node_modules    # Skip if exists

- name: Command with removes
  command: rm -rf /tmp/cache
  removes: /tmp/cache               # Skip if doesn't exist
```

**Parameters:**
| Parameter | Type | Description |
|-----------|------|-------------|
| `command` | string | Command to execute (required) |
| `creates` | string | Skip if this file/directory exists |
| `removes` | string | Skip if this file/directory doesn't exist |

**Returns:**
- `stdout`: Command output
- `stderr`: Error output
- `exit_code`: Exit code (0 = success)

## Package Module

Install, update, or remove packages. Auto-detects package manager (apt, yum, dnf, pacman, apk, zypper).

```yaml
- name: Install nginx
  package: nginx
  state: installed

- name: Ensure latest version
  package: curl
  state: latest

- name: Remove package
  package: apache2
  state: absent
```

**Parameters:**
| Parameter | Type | Description |
|-----------|------|-------------|
| `package` | string | Package name (required) |
| `state` | string | `installed`, `latest`, or `absent` (default: installed) |

## Service Module

Manage systemd services.

```yaml
- name: Start and enable nginx
  service: nginx
  state: running
  enabled: true

- name: Stop service
  service: mysql
  state: stopped

- name: Restart service
  service: apache2
  state: restarted

- name: Reload configuration
  service: nginx
  state: reloaded
```

**Parameters:**
| Parameter | Type | Description |
|-----------|------|-------------|
| `service` | string | Service name (required) |
| `state` | string | `running`, `stopped`, `restarted`, `reloaded` |
| `enabled` | bool | Enable/disable at boot |

## File Module

Manage files, directories, and symlinks.

```yaml
# Create a file with content
- name: Create config file
  file: /etc/app/config.ini
  state: file
  content: |
    [settings]
    debug = true
  owner: app
  group: app
  mode: "0644"

# Create directory
- name: Create directory
  file: /var/log/myapp
  state: directory
  owner: app
  mode: "0755"

# Create symlink
- name: Create symlink
  file: /usr/local/bin/myapp
  state: link
  source: /opt/myapp/bin/myapp

# Remove file or directory
- name: Remove old config
  file: /etc/app/old-config
  state: absent

# Touch file (create empty or update timestamp)
- name: Touch file
  file: /tmp/marker
  state: touch
```

**Parameters:**
| Parameter | Type | Description |
|-----------|------|-------------|
| `file` | string | Path to file/directory (required) |
| `state` | string | `file`, `directory`, `link`, `absent`, `touch` |
| `content` | string | File content (for state: file) |
| `source` | string | Source file to copy or link target |
| `owner` | string | User owner |
| `group` | string | Group owner |
| `mode` | string | Permissions (e.g., "0644") |

## User Module

Manage system users.

```yaml
# Create user
- name: Create app user
  user: appuser
  state: present
  uid: 1000
  groups:
    - docker
    - wheel
  shell: /bin/bash
  home: /home/appuser
  create_home: true

# Remove user
- name: Remove old user
  user: olduser
  state: absent
```

**Parameters:**
| Parameter | Type | Description |
|-----------|------|-------------|
| `user` | string | Username (required) |
| `state` | string | `present` or `absent` |
| `uid` | int | User ID |
| `gid` | int | Primary group ID |
| `groups` | list | Secondary groups |
| `shell` | string | Login shell |
| `home` | string | Home directory path |
| `create_home` | bool | Create home directory (default: true) |

## Template Module

Render Jinja2-style templates.

```yaml
- name: Deploy nginx config
  template: templates/nginx.conf.j2
  dest: /etc/nginx/nginx.conf
  owner: root
  group: root
  mode: "0644"
```

**Template Example (nginx.conf.j2):**
```jinja2
server {
    listen {{ vars.port | default(80) }};
    server_name {{ vars.hostname }};

    {% if vars.ssl_enabled %}
    ssl on;
    ssl_certificate {{ vars.cert_path }};
    {% endif %}

    {% for location in vars.locations %}
    location {{ location.path }} {
        proxy_pass {{ location.backend }};
    }
    {% endfor %}
}
```

**Parameters:**
| Parameter | Type | Description |
|-----------|------|-------------|
| `template` | string | Template file path (required) |
| `dest` | string | Destination path (required) |
| `owner` | string | File owner |
| `group` | string | File group |
| `mode` | string | File permissions |

**Template Features:**
- Variables: `{{ variable }}`
- Filters: `{{ value | upper }}`, `{{ list | join(",") }}`
- Conditionals: `{% if condition %} ... {% endif %}`
- Loops: `{% for item in items %} ... {% endfor %}`
- Includes: `{% include "header.j2" %}`
- Inheritance: `{% extends "base.j2" %}`, `{% block name %} ... {% endblock %}`

**Built-in Filters:**
- String: `upper`, `lower`, `trim`, `title`, `capitalize`, `replace`, `split`, `join`
- Default: `default(value)`
- Length: `length`, `count`
- List: `first`, `last`, `reverse`, `sort`, `unique`
- Type: `int`, `float`, `string`, `bool`
- JSON: `tojson`, `tojson_pretty`
- Path: `basename`, `dirname`

## Async Status Module

Check status of asynchronous tasks.

```yaml
- name: Start long task
  command: /path/to/slow-script.sh
  async: 3600
  poll: 0
  register: job

- name: Wait for completion
  async_status:
    job_id: ${job.ansible_job_id}
  register: result
  until: ${result.finished}
  retry:
    attempts: 60
    delay: 10s
```

**Parameters:**
| Parameter | Type | Description |
|-----------|------|-------------|
| `job_id` | string | Job ID from async task |

**Returns:**
- `finished`: Boolean indicating completion
- `status`: `running`, `finished`, `failed`, `timeout`
- `stdout`/`stderr`: Output (when finished)
- `rc`: Exit code (when finished)

## Facts Module

Gather system information (usually automatic with `gather_facts: true`).

```yaml
- name: Gather all facts
  facts:
    categories:
      - all

- name: Gather specific facts
  facts:
    categories:
      - system
      - network
```

**Categories:**
- `system`: Hostname, OS, kernel
- `hardware`: CPU, memory, architecture
- `network`: Interfaces, IPs
- `all`: Everything

**Available Facts (after gathering):**
- `ansible_hostname`: Short hostname
- `ansible_fqdn`: Fully qualified domain name
- `ansible_os_family`: OS family (Debian, RedHat, etc.)
- `ansible_distribution`: Distribution name
- `ansible_distribution_version`: Version
- `ansible_kernel`: Kernel version
- `ansible_architecture`: CPU architecture
- `ansible_processor_count`: CPU count
- `ansible_memtotal_mb`: Total memory in MB
