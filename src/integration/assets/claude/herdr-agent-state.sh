#!/bin/sh
# installed by herdr
# managed by herdr; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# HERDR_INTEGRATION_ID=claude
# HERDR_INTEGRATION_VERSION=10

set -eu

action="${1:-}"
hook_input_file="$(mktemp "${TMPDIR:-/tmp}/herdr-claude-hook.XXXXXX")" || exit 0
trap 'rm -f "$hook_input_file"' EXIT HUP INT TERM
cat >"$hook_input_file" 2>/dev/null || true

case "$action" in
  session|workdir) ;;
  *) exit 0 ;;
esac

[ "${HERDR_ENV:-}" = "1" ] || exit 0
[ -n "${HERDR_SOCKET_PATH:-}" ] || exit 0
[ -n "${HERDR_PANE_ID:-}" ] || exit 0
command -v python3 >/dev/null 2>&1 || exit 0

HERDR_ACTION="$action" HERDR_HOOK_INPUT_FILE="$hook_input_file" python3 - <<'PY'
import json
import os
import random
import socket
import time

source = "herdr:claude"
action = os.environ.get("HERDR_ACTION", "")
pane_id = os.environ.get("HERDR_PANE_ID")
socket_path = os.environ.get("HERDR_SOCKET_PATH")
hook_input_file = os.environ.get("HERDR_HOOK_INPUT_FILE")

if not pane_id or not socket_path:
    raise SystemExit(0)

hook_input = {}
if hook_input_file:
    try:
        with open(hook_input_file, encoding="utf-8") as handle:
            content = handle.read()
        if content.strip():
            hook_input = json.loads(content)
    except Exception:
        hook_input = {}

# Never speak for Cursor-hosted Claude or subagents; the pane's agent is the
# top-level session.
if "CURSOR_VERSION" in os.environ or "cursor_version" in hook_input:
    raise SystemExit(0)
if hook_input.get("agent_id"):
    raise SystemExit(0)

hook_event_name = str(hook_input.get("hook_event_name") or "")
request_id = f"{source}:{int(time.time() * 1000)}:{random.randrange(1_000_000):06d}"
report_seq = time.time_ns()


def send(method, params):
    request = {"id": request_id, "method": method, "params": params}
    try:
        client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        client.settimeout(0.5)
        client.connect(socket_path)
        client.sendall((json.dumps(request) + "\n").encode())
        try:
            client.recv(4096)
        except Exception:
            pass
        client.close()
    except Exception:
        pass


if action == "session":
    if hook_event_name != "SessionStart":
        raise SystemExit(0)
    session_id = hook_input.get("session_id")
    agent_session_id = session_id if isinstance(session_id, str) and session_id else None
    if not agent_session_id:
        raise SystemExit(0)
    transcript_path = hook_input.get("transcript_path")
    agent_session_path = transcript_path if isinstance(transcript_path, str) and transcript_path else None
    session_start_source = hook_input.get("source")
    if not isinstance(session_start_source, str) or not session_start_source:
        session_start_source = None
    params = {
        "pane_id": pane_id,
        "source": source,
        "agent": "claude",
        "seq": report_seq,
        "agent_session_id": agent_session_id,
    }
    if agent_session_path:
        params["agent_session_path"] = agent_session_path
    if session_start_source:
        params["session_start_source"] = session_start_source
    send("pane.report_agent_session", params)
elif action == "workdir":
    # Report the directory the agent is operating in so Herdr can tell whether
    # it is working in the main checkout or a linked worktree. Claude keeps its
    # process cwd at the launch dir and edits via file paths, so prefer the
    # directory of the file it just edited; fall back to Claude's logical cwd.
    cwd = hook_input.get("cwd")
    cwd = cwd if isinstance(cwd, str) and cwd else None
    tool_input = hook_input.get("tool_input")
    file_path = None
    if isinstance(tool_input, dict):
        for key in ("file_path", "notebook_path"):
            value = tool_input.get(key)
            if isinstance(value, str) and value:
                file_path = value
                break
    working_dir = None
    if file_path:
        if not os.path.isabs(file_path) and cwd:
            file_path = os.path.join(cwd, file_path)
        working_dir = os.path.dirname(file_path) or file_path
    elif cwd:
        working_dir = cwd
    if not working_dir:
        raise SystemExit(0)
    send(
        "pane.report_metadata",
        {
            "pane_id": pane_id,
            "source": source,
            "agent": "claude",
            "seq": report_seq,
            "working_dir": working_dir,
        },
    )
PY
