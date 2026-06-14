import { type ReactNode, useState } from "react";
import { Check, Loader2, Pencil, Plus, Trash2, Zap } from "lucide-react";

import { useProviderStore } from "../stores/providerStore";
import type { ProviderConfig, TestResult } from "../type/providers";
import { ProviderFormModal } from "./ProviderFormModal";

type TestState = Record<string, TestResult | "loading">;

export function ProviderSettings() {
  const providers = useProviderStore((state) => state.providers);
  const activeId = useProviderStore((state) => state.activeId);
  const activate = useProviderStore((state) => state.activate);
  const remove = useProviderStore((state) => state.remove);
  const test = useProviderStore((state) => state.test);
  const error = useProviderStore((state) => state.error);

  const [modalOpen, setModalOpen] = useState(false);
  const [modalMode, setModalMode] = useState<"create" | "edit">("create");
  const [editing, setEditing] = useState<ProviderConfig | undefined>(undefined);
  const [tests, setTests] = useState<TestState>({});

  function openCreate() {
    setModalMode("create");
    setEditing(undefined);
    setModalOpen(true);
  }

  function openEdit(provider: ProviderConfig) {
    setModalMode("edit");
    setEditing(provider);
    setModalOpen(true);
  }

  async function handleDelete(provider: ProviderConfig) {
    if (!window.confirm(`Delete provider "${provider.name}"?`)) return;
    try {
      await remove(provider.id);
    } catch (deleteError) {
      window.alert(resolveMessage(deleteError));
    }
  }

  async function handleTest(provider: ProviderConfig) {
    const model = provider.models[0];
    if (!model) {
      setTests((prev) => ({ ...prev, [provider.id]: { success: false, message: "No model configured" } }));
      return;
    }
    setTests((prev) => ({ ...prev, [provider.id]: "loading" }));
    try {
      const result = await test(provider, model);
      setTests((prev) => ({ ...prev, [provider.id]: result }));
    } catch (testError) {
      setTests((prev) => ({ ...prev, [provider.id]: { success: false, message: resolveMessage(testError) } }));
    }
  }

  return (
    <div className="grid content-start gap-5 p-6 max-[560px]:px-4">
      <section className="rounded-lg border border-slate-200 bg-white p-5">
        <div className="flex items-start justify-between gap-4 border-b border-slate-200 pb-4">
          <div>
            <h2 className="m-0 text-lg font-semibold text-slate-900">Providers</h2>
            <p className="m-0 text-xs text-slate-500">Add cloud API-key providers, then activate one to use in chat.</p>
          </div>
          <button
            onClick={openCreate}
            type="button"
            className="flex items-center gap-2 rounded-lg bg-teal-700 px-3 py-2 text-sm font-medium text-white hover:bg-teal-800"
          >
            <Plus size={16} /> Add
          </button>
        </div>

        {error ? <p className="mt-4 text-xs text-rose-600">{error}</p> : null}

        {providers.length === 0 ? (
          <p className="mt-6 text-sm text-slate-500">No providers yet. Click "Add" to configure your first cloud provider.</p>
        ) : (
          <ul className="mt-4 grid gap-3">
            {providers.map((provider) => {
              const isActive = provider.id === activeId;
              const testState = tests[provider.id];
              return (
                <li key={provider.id} className="rounded-lg border border-slate-200 bg-white p-4">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <strong className="text-sm text-slate-900">{provider.name}</strong>
                        <span className="rounded-full bg-slate-100 px-2 py-0.5 text-xs text-slate-500">{provider.kind}</span>
                        {isActive ? <span className="rounded-full bg-emerald-50 px-2 py-0.5 text-xs text-emerald-700">Active</span> : null}
                      </div>
                      <p className="m-0 mt-1 break-words text-xs text-slate-500">{provider.baseUrl}</p>
                      <p className="m-0 mt-0.5 text-xs text-slate-400">{provider.models.join(", ") || "no models"}</p>
                    </div>
                    <div className="flex shrink-0 items-center gap-1">
                      {isActive ? null : (
                        <button
                          onClick={() => activate(provider.id)}
                          type="button"
                          className="rounded-lg border border-slate-300 px-2 py-1 text-xs text-slate-600 hover:bg-slate-50"
                        >
                          Set active
                        </button>
                      )}
                      <IconButton title="Test" onClick={() => handleTest(provider)}>
                        {testState === "loading" ? <Loader2 size={15} className="animate-spin" /> : <Zap size={15} />}
                      </IconButton>
                      <IconButton title="Edit" onClick={() => openEdit(provider)}>
                        <Pencil size={15} />
                      </IconButton>
                      <IconButton title="Delete" onClick={() => handleDelete(provider)} disabled={isActive}>
                        <Trash2 size={15} />
                      </IconButton>
                    </div>
                  </div>
                  {testState && testState !== "loading" ? (
                    <div className={`mt-3 rounded-md px-3 py-2 text-xs ${testState.success ? "bg-emerald-50 text-emerald-700" : "bg-rose-50 text-rose-700"}`}>
                      {testState.success ? (
                        <span className="flex items-center gap-1">
                          <Check size={13} /> {testState.message}
                        </span>
                      ) : (
                        `Failed: ${testState.message}`
                      )}
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <ProviderFormModal open={modalOpen} mode={modalMode} initial={editing} onClose={() => setModalOpen(false)} />
    </div>
  );
}

function IconButton({ title, onClick, disabled, children }: { title: string; onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      disabled={disabled}
      className="grid size-8 place-items-center rounded-lg border border-transparent text-slate-500 hover:border-slate-200 hover:bg-slate-50 disabled:opacity-40 disabled:hover:border-transparent disabled:hover:bg-transparent"
    >
      {children}
    </button>
  );
}

function resolveMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Unexpected error";
}
