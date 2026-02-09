"""NanoClaw Agno Agent Runner — core entry point.

Reads ContainerInput JSON from stdin, creates an Agno Agent, and runs
a query loop with IPC polling. Outputs results wrapped in sentinel markers
for the host to parse.

Protocol:
  stdin:  ContainerInput JSON (read until EOF)
  stdout: ---NANOCLAW_OUTPUT_START--- / ---NANOCLAW_OUTPUT_END--- marker pairs
  IPC:    Follow-up messages via /workspace/ipc/input/*.json
          Close sentinel: /workspace/ipc/input/_close
"""

import asyncio
import json
import os
import sys
import uuid

from agno.agent import Agent
from agno.db.sqlite import SqliteDb
from agno.models.openai.like import OpenAILike
from agno.tools.duckduckgo import DuckDuckGoTools
from pathlib import Path

from agno.tools.file import FileTools
from agno.tools.shell import ShellTools

from .config import (
    GROUP_DIR,
    SESSION_DB_PATH,
    load_model_config,
    load_nanoclaw_config,
)
from .ipc import drain_ipc_input, log, should_close, wait_for_ipc_message
from .prompts import build_system_prompt
from .tools import get_ipc_tools, set_context


OUTPUT_START_MARKER = "---NANOCLAW_OUTPUT_START---"
OUTPUT_END_MARKER = "---NANOCLAW_OUTPUT_END---"


def write_output(status: str, result: str | None, new_session_id: str | None = None, error: str | None = None) -> None:
    """Write a result wrapped in sentinel markers to stdout."""
    output = {"status": status, "result": result}
    if new_session_id:
        output["newSessionId"] = new_session_id
    if error:
        output["error"] = error
    print(OUTPUT_START_MARKER, flush=True)
    print(json.dumps(output), flush=True)
    print(OUTPUT_END_MARKER, flush=True)


def read_stdin() -> dict:
    """Read full JSON from stdin (blocks until EOF)."""
    data = sys.stdin.read()
    return json.loads(data)


def create_agent(
    model_config,
    system_prompt: str,
    ipc_tools: list,
    session_id: str,
) -> Agent:
    """Create and configure the Agno Agent."""
    model = OpenAILike(
        id=model_config.model_id,
        api_key=model_config.api_key,
        base_url=model_config.base_url,
        temperature=model_config.temperature,
        max_tokens=model_config.max_tokens,
    )

    agent = Agent(
        model=model,
        tools=[
            FileTools(base_dir=Path(GROUP_DIR)),
            ShellTools(base_dir=GROUP_DIR),
            DuckDuckGoTools(),
            *ipc_tools,
        ],
        instructions=system_prompt,
        db=SqliteDb(
            session_table="agent_sessions",
            db_file=SESSION_DB_PATH,
        ),
        session_id=session_id,
        add_history_to_context=True,
        num_history_runs=10,
        markdown=False,
        stream=False,
    )

    return agent


async def run_agent_loop(agent: Agent, initial_prompt: str, session_id: str) -> None:
    """Main query loop: run agent → wait for IPC → repeat."""
    prompt = initial_prompt

    while True:
        log(f"Starting query (session: {session_id})...")

        try:
            response = await agent.arun(prompt, session_id=session_id)
            result_text = response.get_content_as_string() if response else None
            if result_text == "":
                result_text = None
            log(f"Query done. Result: {result_text[:200] if result_text else 'None'}")
            if result_text is not None:
                write_output("success", result_text, session_id)
            # End-of-turn marker for host SSE lifecycle.
            # The host closes one-shot SSE requests when it receives a
            # success frame with a null result.
            write_output("success", None, session_id)
        except Exception as e:
            error_msg = str(e)
            log(f"Agent error: {error_msg}")
            write_output("error", None, session_id, error_msg)
            return

        # Check if closed during the run
        if should_close():
            log("Close sentinel detected after query, exiting")
            break

        log("Query ended, waiting for next IPC message...")
        next_message = await wait_for_ipc_message()
        if next_message is None:
            log("Close sentinel received, exiting")
            break

        log(f"Got new message ({len(next_message)} chars), starting new query")
        prompt = next_message


async def main() -> None:
    """Entry point: parse input, create agent, run query loop."""
    try:
        stdin_data = read_stdin()
    except (json.JSONDecodeError, KeyError) as e:
        write_output("error", None, error=f"Failed to parse input: {e}")
        sys.exit(1)

    nc_config = load_nanoclaw_config(stdin_data)
    log(f"Received input for group: {nc_config.group_folder}")

    try:
        model_config = load_model_config()
    except ValueError as e:
        write_output("error", None, error=str(e))
        sys.exit(1)

    # Set IPC tool context
    set_context(nc_config.chat_jid, nc_config.group_folder, nc_config.is_main)

    # Clean up stale _close sentinel
    try:
        os.unlink("/workspace/ipc/input/_close")
    except OSError:
        pass

    # Build initial prompt
    prompt = nc_config.prompt
    if nc_config.is_scheduled_task:
        prompt = (
            "[SCHEDULED TASK - The following message was sent automatically "
            "and is not coming directly from the user or group.]\n\n" + prompt
        )

    # Drain any pending IPC messages into initial prompt
    pending = drain_ipc_input()
    if pending:
        log(f"Draining {len(pending)} pending IPC messages into initial prompt")
        prompt += "\n" + "\n".join(pending)

    # Session ID: reuse or generate new
    session_id = nc_config.session_id or str(uuid.uuid4())

    # Build system prompt and create agent
    system_prompt = build_system_prompt(nc_config)
    ipc_tools = get_ipc_tools(nc_config.is_main)
    agent = create_agent(model_config, system_prompt, ipc_tools, session_id)

    await run_agent_loop(agent, prompt, session_id)


if __name__ == "__main__":
    asyncio.run(main())
