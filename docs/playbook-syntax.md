# Playbook Syntax Reference

Nexus playbooks are YAML files that define automation tasks. This guide covers the complete syntax.

## Playbook Structure

```yaml
# Target hosts (required)
hosts: all | group_name | pattern

# Playbook-level variables (optional)
vars:
  key: value

# Gather system facts before execution (optional, default: false)
gather_facts: true

# Execution strategy (optional, default: linear)
strategy: linear | free

# Rolling deployment batch size (optional)
serial: 2 | "25%" | [1, 5, 10]

# Tasks run before roles (optional)
pre_tasks:
  - name: Pre-task
    command: echo "Before roles"

# Roles to execute (optional)
roles:
  - common
  - role: webserver
    vars:
      port: 8080

# Main tasks (required unless using roles only)
tasks:
  - name: Task name
    module: args

# Tasks run after main tasks (optional)
post_tasks:
  - name: Post-task
    command: echo "After tasks"

# Handlers triggered by notify (optional)
handlers:
  - name: handler_name
    service: nginx
    state: restarted

# Custom functions (optional)
functions: |
  def my_function():
    return "result"
```

## Task Definition

```yaml
tasks:
  - name: Descriptive task name          # Required
    module: args                         # Module to execute

    # Conditional execution
    when: ${condition}                   # Run only if true

    # Output capture
    register: result_var                 # Store output in variable

    # Custom conditions
    fail_when: ${expression}             # Fail if true
    changed_when: ${expression}          # Mark changed if true

    # Looping
    loop: ${list_variable}               # Iterate over list

    # Handler notification
    notify: handler_name                 # Trigger handler on change
    notify:                              # Or multiple handlers
      - handler1
      - handler2

    # Tags for filtering
    tags:
      - deploy
      - config

    # Privilege escalation
    sudo: true                           # Run as root

    # Retry configuration
    retry:
      attempts: 3
      delay: 5s

    # Async execution
    async: 300                           # Timeout in seconds
    poll: 10                             # Check interval

    # Throttle concurrent execution
    throttle: 2                          # Max parallel hosts

    # Delegate to different host
    delegate_to: localhost
```

## Variables and Expressions

### Variable Syntax

```yaml
vars:
  simple: "value"
  number: 42
  boolean: true
  list:
    - item1
    - item2
  dict:
    key1: value1
    key2: value2
```

### Expression Syntax

Expressions are wrapped in `${}`:

```yaml
tasks:
  - name: Using variables
    command: echo "${vars.simple}"

  - name: Host variables
    command: echo "Host: ${host.name}, IP: ${host.address}"

  - name: Registered variables
    command: echo "Previous output: ${result.stdout}"

  - name: Loop item
    command: echo "Item: ${item}"
    loop: ${vars.list}

  - name: Nested access
    command: echo "${vars.dict.key1}"

  - name: Index access
    command: echo "First: ${vars.list[0]}, Last: ${vars.list[-1]}"
```

### Built-in Functions

```yaml
# String functions
${len(string)}                    # Length
${upper(string)}                  # Uppercase
${lower(string)}                  # Lowercase
${trim(string)}                   # Remove whitespace
${split(string, ",")}             # Split to list
${join(list, "-")}                # Join list to string
${replace(string, "old", "new")}  # Replace substring

# List functions
${len(list)}                      # Count items
${first(list)}                    # First item
${last(list)}                     # Last item
${reverse(list)}                  # Reverse order
${sort(list)}                     # Sort items
${unique(list)}                   # Remove duplicates

# Type functions
${int(string)}                    # Convert to integer
${float(string)}                  # Convert to float
${string(value)}                  # Convert to string
${bool(value)}                    # Convert to boolean

# Default value
${value | default("fallback")}    # Use fallback if null
```

### Operators

```yaml
# Arithmetic
${a + b}    ${a - b}    ${a * b}    ${a / b}    ${a % b}

# Comparison
${a == b}   ${a != b}   ${a < b}    ${a > b}    ${a <= b}   ${a >= b}

# Logical
${a and b}  ${a or b}   ${not a}

# Ternary
${value if condition else default}

# Membership
${item in list}
${key in dict}
```

### Filters

```yaml
# Filter list items
${items | filter(x => x.active)}
${items | select(status="ready")}

# Map transformation
${items | map(x => x.name)}

# Default values
${value | default("fallback")}
```

## Conditionals

```yaml
tasks:
  - name: Run only on Debian
    command: apt-get update
    when: ${host.vars.os_family == "Debian"}

  - name: Run if variable is defined
    command: echo "${vars.optional}"
    when: ${vars.optional is defined}

  - name: Complex condition
    command: echo "Ready"
    when: ${host.vars.ready and vars.deploy_enabled}
```

## Loops

```yaml
tasks:
  # Simple list loop
  - name: Install packages
    package: ${item}
    state: installed
    loop:
      - nginx
      - curl
      - vim

  # Loop over variable
  - name: Create users
    user: ${item.name}
    state: present
    loop: ${vars.users}

  # Loop with index
  - name: Show index
    command: echo "Index ${loop.index}: ${item}"
    loop: ${vars.items}
    # Available: loop.index, loop.index0, loop.first, loop.last, loop.length
```

## Blocks (Error Handling)

```yaml
tasks:
  - block:
      - name: Try this
        command: risky_command
      - name: Then this
        command: another_command
    rescue:
      - name: On failure
        command: echo "Block failed, recovering..."
    always:
      - name: Always run
        command: echo "Cleanup"
```

## Handlers

```yaml
tasks:
  - name: Update config
    file: /etc/app/config.ini
    content: "new config"
    notify: restart_app

handlers:
  - name: restart_app
    service: myapp
    state: restarted
```

## Include and Import

```yaml
tasks:
  # Static import (parsed at load time)
  - import_tasks: common-setup.yml

  # Dynamic include (parsed at runtime)
  - include_tasks: user-setup.yml
    vars:
      username: deploy

  # Conditional include
  - include_tasks: debian-specific.yml
    when: ${host.vars.os_family == "Debian"}
```

## Roles

```yaml
roles:
  # Simple role
  - common

  # Role with variables
  - role: webserver
    vars:
      nginx_port: 8080
    tags:
      - nginx

  # Role with condition
  - role: monitoring
    when: ${vars.enable_monitoring}
```

## Serial Execution

```yaml
# Fixed batch size
serial: 2

# Percentage
serial: "25%"

# Progressive batches
serial: [1, 5, 10, "100%"]
```

## Async Tasks

```yaml
tasks:
  - name: Long running task
    command: /path/to/slow-script.sh
    async: 3600           # Max runtime (seconds)
    poll: 0               # Don't wait (fire-and-forget)
    register: job

  - name: Check job status
    async_status:
      job_id: ${job.ansible_job_id}
    register: job_result
    until: ${job_result.finished}
    retry:
      attempts: 30
      delay: 10s
```
