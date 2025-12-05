#!/bin/bash
# Dynamic Inventory Example for Nexus
#
# This script demonstrates the Ansible-compatible dynamic inventory interface.
# It can be used as a template for building custom dynamic inventory scripts.
#
# Usage:
#   ./dynamic-inventory.sh --list
#   ./dynamic-inventory.sh --host <hostname>

set -e

if [ "$1" = "--list" ]; then
    # Return full inventory as JSON
    # This includes all groups, hosts, and the special _meta section
    cat <<'EOF'
{
  "webservers": {
    "hosts": ["web1", "web2"],
    "vars": {
      "http_port": 80,
      "app_env": "production"
    }
  },
  "dbservers": {
    "hosts": ["db1", "db2"],
    "vars": {
      "db_port": 5432,
      "db_name": "production"
    }
  },
  "loadbalancers": {
    "hosts": ["lb1"]
  },
  "production": {
    "children": ["webservers", "dbservers", "loadbalancers"],
    "vars": {
      "environment": "production",
      "datacenter": "us-east-1"
    }
  },
  "_meta": {
    "hostvars": {
      "web1": {
        "ansible_host": "192.168.1.10",
        "ansible_user": "ubuntu",
        "ansible_port": 22,
        "role": "frontend"
      },
      "web2": {
        "ansible_host": "192.168.1.11",
        "ansible_user": "ubuntu",
        "ansible_port": 22,
        "role": "frontend"
      },
      "db1": {
        "ansible_host": "192.168.1.20",
        "ansible_user": "postgres",
        "ansible_port": 22,
        "role": "primary",
        "db_replica": false
      },
      "db2": {
        "ansible_host": "192.168.1.21",
        "ansible_user": "postgres",
        "ansible_port": 22,
        "role": "replica",
        "db_replica": true
      },
      "lb1": {
        "ansible_host": "192.168.1.5",
        "ansible_user": "ubuntu",
        "ansible_port": 22,
        "backend_servers": ["web1", "web2"]
      }
    }
  }
}
EOF

elif [ "$1" = "--host" ]; then
    # Return host-specific variables (optional, for backwards compatibility)
    # Modern dynamic inventory should use _meta.hostvars in --list output
    HOSTNAME="$2"

    case "$HOSTNAME" in
        web1)
            cat <<'EOF'
{
  "ansible_host": "192.168.1.10",
  "ansible_user": "ubuntu",
  "ansible_port": 22,
  "role": "frontend"
}
EOF
            ;;
        web2)
            cat <<'EOF'
{
  "ansible_host": "192.168.1.11",
  "ansible_user": "ubuntu",
  "ansible_port": 22,
  "role": "frontend"
}
EOF
            ;;
        db1)
            cat <<'EOF'
{
  "ansible_host": "192.168.1.20",
  "ansible_user": "postgres",
  "ansible_port": 22,
  "role": "primary",
  "db_replica": false
}
EOF
            ;;
        db2)
            cat <<'EOF'
{
  "ansible_host": "192.168.1.21",
  "ansible_user": "postgres",
  "ansible_port": 22,
  "role": "replica",
  "db_replica": true
}
EOF
            ;;
        lb1)
            cat <<'EOF'
{
  "ansible_host": "192.168.1.5",
  "ansible_user": "ubuntu",
  "ansible_port": 22,
  "backend_servers": ["web1", "web2"]
}
EOF
            ;;
        *)
            echo "{}"
            ;;
    esac
else
    echo "Usage: $0 --list | --host <hostname>" >&2
    exit 1
fi
