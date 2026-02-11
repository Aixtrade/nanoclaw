<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";

  import Chat from "./lib/Chat.svelte";
  import Input from "./lib/Input.svelte";
  import Setup from "./lib/Setup.svelte";
  import { streamChat, checkHealth, configureApi } from "./lib/api";
  import logo from "./assets/logo.png";

  interface SetupStatus {
    nodeInstalled: boolean;
    nodeVersion: string;
    dockerRunning: boolean;
    containerImageBuilt: boolean;
    containerResourcesReady: boolean;
    apiKeyConfigured: boolean;
    userDataDir: string;
  }

  const groupId = "main";

  let setupComplete = $state(false);
  let checkingSetup = $state(true);

  let backendReady = $state(false);
  let backendStarting = $state(true);
  let streaming = $state(false);
  let streamText = $state("");
  let userText = $state("");
  let agentText = $state("");
  let inputRef = $state<ReturnType<typeof Input> | undefined>(undefined);
  let failedHealthChecks = 0;

  let status = $derived<"running" | "starting" | "stopped">(
    backendReady ? "running" : backendStarting ? "starting" : "stopped"
  );

  let disposed = false;
  let healthCheck: ReturnType<typeof setInterval> | null = null;
  let unlistenReady: (() => void) | null = null;
  let unlistenStopped: (() => void) | null = null;

  async function probeHealth() {
    const healthy = await checkHealth();
    if (healthy) {
      backendReady = true;
      backendStarting = false;
      failedHealthChecks = 0;
      return;
    }

    backendReady = false;
    failedHealthChecks += 1;
    if (backendStarting && failedHealthChecks >= 3) {
      backendStarting = false;
    }
  }

  function allChecksPass(s: SetupStatus): boolean {
    return (
      s.nodeInstalled &&
      s.dockerRunning &&
      s.containerResourcesReady &&
      s.containerImageBuilt &&
      s.apiKeyConfigured
    );
  }

  async function init() {
    backendStarting = true;
    failedHealthChecks = 0;

    try {
      const config = await invoke<{ baseUrl: string; authToken: string | null }>(
        "get_backend_config"
      );
      configureApi(config);
    } catch {
      // fallback to default API base in api.ts
    }

    try {
      unlistenReady = await listen("backend-ready", () => {
        backendReady = true;
        backendStarting = false;
        failedHealthChecks = 0;
      });

      unlistenStopped = await listen("backend-stopped", () => {
        backendReady = false;
        backendStarting = false;
      });

      if (disposed) {
        unlistenReady();
        unlistenStopped();
        return;
      }
    } catch {
      // event bridge unavailable; health polling still covers readiness
    }

    try {
      backendReady = await invoke<boolean>("get_backend_status");
      if (backendReady) {
        backendStarting = false;
        failedHealthChecks = 0;
      }
    } catch {
      backendReady = false;
    }

    if (!backendReady) {
      await probeHealth();
    }

    healthCheck = setInterval(async () => {
      await probeHealth();
    }, 2000);
  }

  function handleSetupComplete() {
    setupComplete = true;
    init();
  }

  onMount(() => {
    invoke<SetupStatus>("check_setup").then((s) => {
      setupComplete = allChecksPass(s);
      checkingSetup = false;
      if (setupComplete) {
        init();
      }
    }).catch(() => {
      setupComplete = true;
      checkingSetup = false;
      init();
    });

    return () => {
      disposed = true;
      if (healthCheck) {
        clearInterval(healthCheck);
      }
      if (unlistenReady) {
        unlistenReady();
      }
      if (unlistenStopped) {
        unlistenStopped();
      }
    };
  });

  async function handleSend(text: string) {
    if (streaming || !backendReady) return;

    // Clear previous round, start new one
    userText = text;
    agentText = "";
    streaming = true;
    streamText = "";

    try {
      for await (const event of streamChat(text, groupId)) {
        if (event.type === "message" && event.data.text) {
          streamText += event.data.text;
        } else if (event.type === "error") {
          const errorText = event.data.error || "An error occurred";
          if (streamText) {
            agentText = streamText + `\n\nError: ${errorText}`;
          } else {
            agentText = `Error: ${errorText}`;
          }
          streamText = "";
          break;
        } else if (event.type === "done") {
          break;
        }
      }
    } catch (e: unknown) {
      const errorText = e instanceof Error ? e.message : "Connection failed";
      agentText = `Error: ${errorText}`;
      const healthy = await checkHealth();
      backendReady = healthy;
      backendStarting = false;
    }

    // Commit streamed text as complete
    if (streamText) {
      agentText = streamText;
    }

    streaming = false;
    streamText = "";
    await tick();
    inputRef?.focus();
    requestAnimationFrame(() => {
      inputRef?.focus();
    });
  }

  async function restartBackend() {
    try {
      backendReady = false;
      backendStarting = true;
      failedHealthChecks = 0;
      await invoke("restart_backend");
    } catch (e: unknown) {
      console.error("Failed to restart:", e);
      backendStarting = false;
    }
  }

  function handleDrag(e: MouseEvent) {
    if (e.button === 0 && e.detail === 1) {
      getCurrentWindow().startDragging();
    }
  }

  function handleDragDblClick() {
    getCurrentWindow().toggleMaximize();
  }
</script>

{#if checkingSetup}
  <div class="loading">
    <p>Loading...</p>
  </div>
{:else if !setupComplete}
  <Setup onComplete={handleSetupComplete} />
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="app" onclick={() => inputRef?.focus()}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="drag-region" onmousedown={handleDrag} ondblclick={handleDragDblClick} onclick={() => inputRef?.focus()}></div>

    <div class="header">
      <button class="logo-btn" onclick={restartBackend} title="Restart backend">
        <img src={logo} alt="NanoClaw" class="logo" />
        <span
          class="status-dot"
          class:green={status === "running"}
          class:yellow={status === "starting"}
          class:red={status === "stopped"}
        ></span>
      </button>
    </div>

    <div class="content">
      <Chat {userText} {agentText} {streaming} {streamText}>
        <Input bind:this={inputRef} disabled={!backendReady || streaming} onSend={handleSend} />
      </Chat>
    </div>
  </div>
{/if}

<style>
  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
  }

  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .drag-region {
    height: 40px;
    min-height: 40px;
    -webkit-app-region: drag;
  }

  .header {
    padding: 8px 24px 12px;
    display: flex;
    align-items: center;
  }

  .logo-btn {
    position: relative;
    width: 48px;
    height: 48px;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
  }

  .logo-btn:hover {
    background: rgba(255, 255, 255, 0.06);
  }

  .logo {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    display: block;
  }

  .status-dot {
    position: absolute;
    bottom: 0;
    right: 0;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    border: 2px solid var(--bg);
  }

  .status-dot.green {
    background: var(--green);
    box-shadow: 0 0 4px var(--green);
  }

  .status-dot.yellow {
    background: var(--yellow);
    animation: pulse 1.5s ease-in-out infinite;
  }

  .status-dot.red {
    background: var(--red);
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }

  .content {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
</style>
