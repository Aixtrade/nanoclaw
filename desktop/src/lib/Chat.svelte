<script lang="ts">
  import { tick } from "svelte";

  interface Message {
    role: "user" | "agent";
    text: string;
  }

  interface Props {
    messages: Message[];
    streaming: boolean;
    streamText: string;
  }

  let { messages, streaming, streamText }: Props = $props();

  let container: HTMLDivElement;

  async function scrollToBottom() {
    await tick();
    if (container) {
      container.scrollTop = container.scrollHeight;
    }
  }

  $effect(() => {
    // Re-run whenever messages change or stream text updates
    messages;
    streamText;
    scrollToBottom();
  });
</script>

<div class="chat" bind:this={container}>
  {#if messages.length === 0 && !streaming}
    <div class="empty">
      <div class="empty-icon">&#x1f680;</div>
      <p>Send a message to start chatting</p>
    </div>
  {/if}

  {#each messages as msg, i (i)}
    <div class="message" class:user={msg.role === "user"} class:agent={msg.role === "agent"}>
      <div class="bubble">
        <pre>{msg.text}</pre>
      </div>
    </div>
  {/each}

  {#if streaming && streamText}
    <div class="message agent">
      <div class="bubble">
        <pre>{streamText}</pre>
        <span class="typing">|</span>
      </div>
    </div>
  {:else if streaming}
    <div class="message agent">
      <div class="bubble thinking">
        <span class="dots">Thinking</span>
      </div>
    </div>
  {/if}
</div>

<style>
  .chat {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    gap: 8px;
  }

  .empty-icon {
    font-size: 32px;
  }

  .message {
    display: flex;
  }

  .message.user {
    justify-content: flex-end;
  }

  .message.agent {
    justify-content: flex-start;
  }

  .bubble {
    max-width: 80%;
    padding: 8px 12px;
    border-radius: 12px;
    word-break: break-word;
  }

  .message.user .bubble {
    background: var(--bg-message-user);
    border-bottom-right-radius: 4px;
  }

  .message.agent .bubble {
    background: var(--bg-message-agent);
    border-bottom-left-radius: 4px;
  }

  pre {
    white-space: pre-wrap;
    font-family: inherit;
    margin: 0;
    font-size: 14px;
    line-height: 1.5;
  }

  .typing {
    animation: blink 0.7s infinite;
  }

  @keyframes blink {
    0%, 100% { opacity: 1; }
    50% { opacity: 0; }
  }

  .thinking .dots::after {
    content: "...";
    animation: dots 1.5s infinite;
  }

  @keyframes dots {
    0% { content: "."; }
    33% { content: ".."; }
    66% { content: "..."; }
  }
</style>
