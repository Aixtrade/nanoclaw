<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  import Sidebar from "./lib/Sidebar.svelte";
  import Chat from "./lib/Chat.svelte";
  import Input from "./lib/Input.svelte";
  import StatusBar from "./lib/StatusBar.svelte";
  import {
    streamChat,
    getGroups,
    checkHealth,
    configureApi,
    type Group,
  } from "./lib/api";

  interface Message {
    role: "user" | "agent";
    text: string;
  }

  let groups = $state<Group[]>([]);
  let activeGroup = $state("main");
  let backendReady = $state(false);
  let streaming = $state(false);
  let streamText = $state("");

  // Per-group message history (in memory)
  let messageStore = $state<Record<string, Message[]>>({});

  let messages = $derived(messageStore[activeGroup] ?? []);

  let status = $derived<"running" | "starting" | "stopped">(
    backendReady ? "running" : streaming ? "starting" : "stopped"
  );

  let sidebar: Sidebar;

  onMount(() => {
    let disposed = false;
    let healthCheck: ReturnType<typeof setInterval> | null = null;
    let unlistenReady: (() => void) | null = null;
    let unlistenStopped: (() => void) | null = null;

    const init = async () => {
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
          loadGroups();
        });

        unlistenStopped = await listen("backend-stopped", () => {
          backendReady = false;
        });

        if (disposed) {
          unlistenReady();
          unlistenStopped();
          return;
        }
      } catch {
        // event bridge unavailable; health polling still covers readiness
      }

      // Check initial status
      try {
        backendReady = await invoke<boolean>("get_backend_status");
      } catch {
        backendReady = false;
      }

      if (backendReady) {
        loadGroups();
      }

      // Also poll health to catch backend that started before event listeners attached
      healthCheck = setInterval(async () => {
        if (!backendReady) {
          const healthy = await checkHealth();
          if (healthy) {
            backendReady = true;
            loadGroups();
          }
        }
      }, 2000);
    };

    init();

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

  async function loadGroups() {
    try {
      groups = await getGroups();
      // If active group doesn't exist in list, default to first or "main"
      if (groups.length > 0 && !groups.find(g => g.id === activeGroup)) {
        activeGroup = groups[0].id;
      }
    } catch {
      // backend may still be starting
    }
  }

  async function handleSend(text: string) {
    if (streaming || !backendReady) return;
    const groupId = activeGroup;

    // Add user message
    if (!messageStore[groupId]) messageStore[groupId] = [];
    messageStore[groupId] = [...messageStore[groupId], { role: "user", text }];
    // Trigger reactivity
    messageStore = { ...messageStore };

    streaming = true;
    streamText = "";

    try {
      for await (const event of streamChat(text, groupId)) {
        if (event.type === "message" && event.data.text) {
          streamText += event.data.text;
        } else if (event.type === "error") {
          // Show error as agent message
          const errorText = event.data.error || "An error occurred";
          if (streamText) {
            messageStore[groupId] = [
              ...messageStore[groupId],
              { role: "agent", text: streamText },
            ];
          }
          messageStore[groupId] = [
            ...messageStore[groupId],
            { role: "agent", text: `Error: ${errorText}` },
          ];
          messageStore = { ...messageStore };
          streamText = "";
          break;
        } else if (event.type === "done") {
          break;
        }
      }
    } catch (e: unknown) {
      const errorText = e instanceof Error ? e.message : "Connection failed";
      messageStore[groupId] = [
        ...(messageStore[groupId] ?? []),
        { role: "agent", text: `Error: ${errorText}` },
      ];
      messageStore = { ...messageStore };
    }

    // Commit streamed text as a complete message
    if (streamText) {
      messageStore[groupId] = [
        ...messageStore[groupId],
        { role: "agent", text: streamText },
      ];
      messageStore = { ...messageStore };
    }

    streaming = false;
    streamText = "";
  }

  function selectGroup(id: string) {
    activeGroup = id;
  }

  function handleGroupsChanged(newGroups: Group[]) {
    groups = newGroups;
  }
</script>

<div class="app">
  <Sidebar
    bind:this={sidebar}
    {groups}
    {activeGroup}
    {backendReady}
    onSelectGroup={selectGroup}
    onGroupsChanged={handleGroupsChanged}
  />
  <main class="main">
    <div class="chat-area">
      <Chat {messages} {streaming} {streamText} />
      <Input disabled={!backendReady || streaming} onSend={handleSend} />
    </div>
    <StatusBar {status} />
  </main>
</div>

<style>
  .app {
    display: flex;
    height: 100%;
  }

  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .chat-area {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
</style>
