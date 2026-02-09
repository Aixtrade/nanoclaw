#!/bin/bash
set -e
[ -f /workspace/env-dir/env ] && export $(cat /workspace/env-dir/env | xargs)
cat > /tmp/input.json
cd /app && PYTHONPATH=/app uv run python -m src.main < /tmp/input.json
