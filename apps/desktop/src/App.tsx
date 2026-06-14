import { type ReactNode, useEffect, useRef, useState } from "react";
import {
  MessageSquare,
  PanelLeft,
  Settings as SettingsIcon,
  Sparkles,
} from "lucide-react";
import { providersApi } from "./api/providers";
import { AssistantMessage } from "./components/chat/AssistantMessage";
import { ChatInput } from "./components/chat/ChatInput";
import { UserMessage } from "./components/chat/UserMessage";
import { ProviderSettings } from "./components/ProviderSettings";
import { useActiveProvider, useProviderStore } from "./stores/providerStore";

type View = "chat" | "settings";

interface ChatItem {
  id: string;
  role: "user" | "assistant";
  content: string;
  reasoningText?: string | null;
  model?: string;
  isStreaming?: boolean;
}

const WELCOME: ChatItem = {
  id: "welcome",
  role: "assistant",
  content: "Add a cloud provider in Providers, activate it, then start a conversation.",
};

function App() {
  const [view, setView] = useState<View>("chat");
  const fetchAll = useProviderStore((state) => state.fetchAll);
  const fetchPresets = useProviderStore((state) => state.fetchPresets);
  const active = useActiveProvider();

  useEffect(() => {
    void fetchAll();
    void fetchPresets();
  }, [fetchAll, fetchPresets]);

  return (
    <main className="grid min-h-screen grid-cols-[260px_minmax(0,1fr)] bg-slate-100 text-slate-800 max-[820px]:grid-cols-1">
      <Sidebar view={view} onNavigate={setView} activeName={active?.name ?? null} />
      <section className="grid min-h-screen min-w-0 grid-rows-[auto_minmax(0,1fr)] max-[820px]:min-h-[calc(100vh-126px)]">
        <header className="flex min-h-[68px] items-center justify-between gap-4 border-b border-slate-200 bg-white px-6 py-3 max-[560px]:px-4">
          <div>
            <h1 className="m-0 text-xl font-semibold text-slate-900">{view === "chat" ? "Chat" : "Providers"}</h1>
            <p className="m-0 text-xs text-slate-500">
              {active ? `${active.name} · ${active.models[0] ?? "no model"}` : "No active provider"}
            </p>
          </div>
          <button
            className="grid size-10 place-items-center rounded-lg border border-slate-300 bg-white text-slate-700 hover:bg-slate-50"
            type="button"
            onClick={() => setView(view === "chat" ? "settings" : "chat")}
            aria-label="Toggle view"
          >
            {view === "chat" ? <SettingsIcon size={18} /> : <PanelLeft size={18} />}
          </button>
        </header>
        {view === "chat" ? <ChatView activeId={active?.id ?? null} /> : <ProviderSettings />}
      </section>
    </main>
  );
}

function Sidebar({ view, onNavigate, activeName }: { view: View; onNavigate: (next: View) => void; activeName: string | null }) {
  return (
    <aside className="flex min-w-0 flex-col gap-4 border-r border-slate-200 bg-white/80 p-4 max-[820px]:grid max-[820px]:grid-cols-[1fr_auto] max-[820px]:border-r-0 max-[820px]:border-b">
      <div className="flex min-h-11 items-center gap-3">
        <div className="grid size-9 place-items-center rounded-lg border border-teal-100 bg-teal-50 text-teal-700">
          <Sparkles size={18} />
        </div>
        <div>
          <strong className="block text-sm leading-5">Anvil</strong>
          <span className="block text-xs text-slate-500">Desktop</span>
        </div>
      </div>
      <nav className="grid gap-1 max-[820px]:col-span-2 max-[820px]:grid-cols-2">
        <NavButton active={view === "chat"} icon={<MessageSquare size={17} />} label="Chat" onClick={() => onNavigate("chat")} />
        <NavButton active={view === "settings"} icon={<SettingsIcon size={17} />} label="Providers" onClick={() => onNavigate("settings")} />
      </nav>
      <div className="mt-auto rounded-lg border border-slate-200 bg-white p-3 max-[820px]:hidden">
        <span className="text-xs text-slate-500">Active provider</span>
        <strong className="mt-1 block break-words text-sm">{activeName ?? "None"}</strong>
        <div className="mt-3 flex items-center gap-2 text-xs">
          <span className={`size-2 rounded-full ${activeName ? "bg-emerald-500" : "bg-amber-500"}`} />
          <span className="text-slate-500">{activeName ? "Ready" : "Not configured"}</span>
        </div>
      </div>
    </aside>
  );
}

function NavButton({ active, icon, label, onClick }: { active: boolean; icon: ReactNode; label: string; onClick: () => void }) {
  return (
    <button
      className={`flex min-h-10 items-center gap-2 rounded-lg border px-3 text-left text-sm ${
        active ? "border-slate-300 bg-slate-100 text-slate-900" : "border-transparent text-slate-700 hover:border-slate-200 hover:bg-slate-50"
      }`}
      type="button"
      onClick={onClick}
    >
      {icon}
      <span>{label}</span>
    </button>
  );
}

function ChatView({ activeId }: { activeId: string | null }) {
  const active = useActiveProvider();
  const [draft, setDraft] = useState("");
  const [model, setModel] = useState("");
  const [messages, setMessages] = useState<ChatItem[]>([WELCOME]);
  const [isSending, setIsSending] = useState(false);
  const currentRequestIdRef = useRef<string | null>(null);

  useEffect(() => {
    setModel(active?.models[0] ?? "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeId]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void providersApi.listenToChatStream((payload) => {
      if (disposed || payload.requestId !== currentRequestIdRef.current) return;

      if (payload.event === "text_delta" && payload.delta) {
        setMessages((current) =>
          current.map((message) =>
            message.id === payload.requestId
              ? { ...message, content: `${message.content}${payload.delta}` }
              : message,
          ),
        );
        return;
      }

      if (payload.event === "reasoning_delta" && payload.delta) {
        setMessages((current) =>
          current.map((message) =>
            message.id === payload.requestId
              ? { ...message, reasoningText: `${message.reasoningText ?? ""}${payload.delta}` }
              : message,
          ),
        );
        return;
      }

      if (payload.event === "error") {
        setMessages((current) =>
          current.map((message) =>
            message.id === payload.requestId
              ? {
                  ...message,
                  content: `Request failed: ${payload.message ?? "Unexpected error"}`,
                  isStreaming: false,
                }
              : message,
          ),
        );
        setIsSending(false);
        currentRequestIdRef.current = null;
      }
    }).then((dispose) => {
      if (disposed) {
        dispose();
      } else {
        unlisten = dispose;
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const modelOptions = active?.models ?? [];

  async function send() {
    const content = draft.trim();
    if (!content || isSending || !active || !model) return;

    const userMessage: ChatItem = { id: crypto.randomUUID(), role: "user", content };
    const conversation = [...messages.filter((item) => item.id !== "welcome"), userMessage];
    const requestId = crypto.randomUUID();
    currentRequestIdRef.current = requestId;
    setMessages([...conversation, { id: requestId, role: "assistant", model, content: "", reasoningText: "", isStreaming: true }]);
    setDraft("");
    setIsSending(true);

    try {
      const response = await providersApi.chatGenerateStream({
        requestId,
        providerId: active.id,
        model,
        messages: conversation.map((item) => ({ role: item.role, content: item.content })),
      });
      setMessages((current) =>
        current.map((message) =>
          message.id === requestId
            ? {
                ...message,
                content: response.text || message.content || "(empty response)",
                reasoningText: response.reasoningText ?? message.reasoningText,
                isStreaming: false,
              }
            : message,
        ),
      );
    } catch (sendError) {
      setMessages((current) =>
        current.map((message) =>
          message.id === requestId
            ? { ...message, content: `Request failed: ${resolveMessage(sendError)}`, isStreaming: false }
            : message,
        ),
      );
    } finally {
      setIsSending(false);
      currentRequestIdRef.current = null;
    }
  }

  if (!active) {
    return (
      <div className="grid place-items-center p-10 text-center">
        <div className="max-w-sm">
          <h3 className="m-0 text-base font-semibold text-slate-900">No active provider</h3>
          <p className="mt-2 text-sm text-slate-500">
            Go to Providers, add a cloud API-key provider, and set it active to start chatting.
          </p>
          <p className="mt-2 text-xs text-slate-400">Local models and login are not supported.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="grid min-h-0 grid-rows-[minmax(0,1fr)_auto]">
      <div className="flex min-h-0 flex-col gap-4 overflow-auto p-6 max-[560px]:px-4" aria-live="polite">
        {messages.map((message) => (
          message.role === "user" ? (
            <UserMessage key={message.id} content={message.content} />
          ) : (
            <AssistantMessage
              key={message.id}
              content={message.content}
              reasoningText={message.reasoningText}
              model={message.model}
              isStreaming={message.isStreaming}
            />
          )
        ))}
      </div>

      <ChatInput
        activeProviderName={active.name}
        model={model}
        modelOptions={modelOptions}
        value={draft}
        isSending={isSending}
        disabled={!active}
        onValueChange={setDraft}
        onModelChange={setModel}
        onSubmit={() => void send()}
      />
    </div>
  );
}

function resolveMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Unexpected error";
}

export default App;
