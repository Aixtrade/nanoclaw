<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface Props {
    status: "running" | "starting" | "stopped";
  }

  let { status }: Props = $props();

  async function restart() {
    try {
      await invoke("restart_backend");
    } catch (e: any) {
      console.error("Failed to restart:", e);
    }
  }
</script>

<footer class="status-bar">
  <div class="status">
    <span
      class="dot"
      class:green={status === "running"}
      class:yellow={status === "starting"}
      class:red={status === "stopped"}
    ></span>
    <span class="label">
      {#if status === "running"}Running{:else if status === "starting"}Starting...{:else}Stopped{/if}
    </span>
  </div>
  <button class="restart-btn" onclick={restart} title="Restart backend">
    Restart
  </button>
</footer>

<style>
  .status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 16px;
    border-top: 1px solid var(--border);
    background: var(--bg-sidebar);
    font-size: 12px;
    height: 28px;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .dot.green {
    background: var(--green);
    box-shadow: 0 0 4px var(--green);
  }

  .dot.yellow {
    background: var(--yellow);
    animation: pulse 1.5s ease-in-out infinite;
  }

  .dot.red {
    background: var(--red);
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }

  .label {
    color: var(--text-muted);
  }

  .restart-btn {
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 11px;
    color: var(--text-muted);
  }

  .restart-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text);
  }
</style>
