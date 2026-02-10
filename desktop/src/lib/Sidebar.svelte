<script lang="ts">
  import { tick } from "svelte";
  import { getGroups, createGroup, type Group } from "./api";

  interface Props {
    groups: Group[];
    activeGroup: string;
    backendReady: boolean;
    onSelectGroup: (id: string) => void;
    onGroupsChanged: (groups: Group[]) => void;
  }

  let { groups, activeGroup, backendReady, onSelectGroup, onGroupsChanged }: Props = $props();
  let creating = $state(false);
  let newName = $state("");
  let newGroupInput = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (!creating) return;
    tick().then(() => {
      newGroupInput?.focus();
      newGroupInput?.select();
    });
  });

  export async function refresh() {
    if (!backendReady) return;
    try {
      const fetched = await getGroups();
      onGroupsChanged(fetched);
    } catch {
      // backend may not be ready yet
    }
  }

  async function handleCreate() {
    const name = newName.trim();
    if (!name) return;
    try {
      await createGroup(name);
      newName = "";
      creating = false;
      await refresh();
    } catch (e: unknown) {
      alert(e instanceof Error ? e.message : "Failed to create group");
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") handleCreate();
    if (e.key === "Escape") {
      creating = false;
      newName = "";
    }
  }
</script>

<aside class="sidebar">
  <div class="header">
    <span class="title">Groups</span>
    <button
      class="add-btn"
      onclick={() => (creating = true)}
      disabled={!backendReady}
      title="New group"
    >+</button>
  </div>

  {#if creating}
    <div class="create-form">
      <input
        bind:this={newGroupInput}
        type="text"
        bind:value={newName}
        onkeydown={handleKeydown}
        placeholder="Group name..."
      />
    </div>
  {/if}

  <div class="group-list">
    {#each groups as group (group.id)}
      <button
        class="group-item"
        class:active={group.id === activeGroup}
        onclick={() => onSelectGroup(group.id)}
      >
        {group.name}
      </button>
    {/each}

    {#if groups.length === 0 && backendReady}
      <div class="empty">No groups yet</div>
    {/if}
    {#if !backendReady}
      <div class="empty">Waiting for backend...</div>
    {/if}
  </div>
</aside>

<style>
  .sidebar {
    width: 200px;
    min-width: 200px;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
  }

  .title {
    font-weight: 600;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
  }

  .add-btn {
    width: 24px;
    height: 24px;
    border-radius: 4px;
    background: var(--bg-input);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 16px;
  }

  .add-btn:hover:not(:disabled) {
    background: var(--accent);
  }

  .create-form {
    padding: 8px;
  }

  .create-form input {
    width: 100%;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg-input);
    font-size: 13px;
  }

  .create-form input:focus {
    outline: none;
    border-color: var(--accent);
  }

  .group-list {
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
  }

  .group-item {
    width: 100%;
    text-align: left;
    padding: 8px 14px;
    font-size: 13px;
    border-radius: 0;
    color: var(--text);
  }

  .group-item:hover {
    background: rgba(255, 255, 255, 0.05);
  }

  .group-item.active {
    background: rgba(59, 130, 246, 0.15);
    color: var(--accent);
  }

  .empty {
    padding: 16px 14px;
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
  }
</style>
