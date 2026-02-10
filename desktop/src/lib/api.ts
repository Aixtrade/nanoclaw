const API_BASE = "http://127.0.0.1:3000";

export interface Group {
  id: string;
  name: string;
  folder: string;
  added_at: string;
}

export interface ChatEvent {
  type: "message" | "error" | "done";
  data: { text?: string; error?: string; sessionId?: string | null };
}

export async function getGroups(): Promise<Group[]> {
  const res = await fetch(`${API_BASE}/api/groups`);
  if (!res.ok) throw new Error(`Failed to fetch groups: ${res.status}`);
  return res.json();
}

export async function createGroup(name: string): Promise<Group> {
  const res = await fetch(`${API_BASE}/api/groups`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name, folder: name }),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err.error || `Failed to create group: ${res.status}`);
  }
  return res.json();
}

export async function* streamChat(
  prompt: string,
  groupId: string,
): AsyncGenerator<ChatEvent> {
  const res = await fetch(`${API_BASE}/api/chat`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ prompt, groupId }),
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    yield {
      type: "error",
      data: { error: err.error || `HTTP ${res.status}` },
    };
    return;
  }

  const reader = res.body!.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });

    // Parse SSE format
    const parts = buffer.split("\n\n");
    buffer = parts.pop() || "";

    for (const part of parts) {
      let eventType = "message";
      let data = "";

      for (const line of part.split("\n")) {
        if (line.startsWith("event: ")) {
          eventType = line.slice(7).trim();
        } else if (line.startsWith("data: ")) {
          data = line.slice(6);
        }
      }

      if (!data) continue;

      try {
        const parsed = JSON.parse(data);
        yield { type: eventType as ChatEvent["type"], data: parsed };
      } catch {
        // skip malformed data
      }
    }
  }
}

export async function checkHealth(): Promise<boolean> {
  try {
    const res = await fetch(`${API_BASE}/api/health`, {
      signal: AbortSignal.timeout(2000),
    });
    return res.ok;
  } catch {
    return false;
  }
}
