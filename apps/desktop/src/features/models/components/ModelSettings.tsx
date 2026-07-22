import { type ReactNode, useState } from "react";
import { Bot, Check, Loader2, Pencil, Plus, Trash2, X, Zap } from "lucide-react";
import { useTranslation } from "react-i18next";

import { resolveErrorMessage as resolveMessage } from "../../../utils/commandError";
import type { ProviderConfig, TestResult } from "../contracts";
import { useModelStore } from "../modelStore";
import { providerTestResultStyle, ProviderFormModal } from "./ProviderFormModal";

type TestState = Record<string, TestResult | "loading">;

export function ModelSettings() {
  const { t } = useTranslation();
  const providers = useModelStore((state) => state.providers);
  const remove = useModelStore((state) => state.remove);
  const test = useModelStore((state) => state.test);
  const error = useModelStore((state) => state.error);

  const [modalOpen, setModalOpen] = useState(false);
  const [modalMode, setModalMode] = useState<"create" | "edit">("create");
  const [editing, setEditing] = useState<ProviderConfig | undefined>(undefined);
  const [tests, setTests] = useState<TestState>({});
  const [confirmingDeleteId, setConfirmingDeleteId] = useState<string | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);

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
    setConfirmingDeleteId(null);
    setDeleteError(null);
    try {
      await remove(provider.id);
    } catch (deleteError) {
      setDeleteError(resolveMessage(deleteError));
    }
  }

  async function handleTest(provider: ProviderConfig) {
    const model = provider.models.find((item) => item.enabled)?.modelId;
    if (!model) {
      setTests((prev) => ({ ...prev, [provider.id]: { success: false, message: t("settings.models.noModels") } }));
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
    <div className="mx-auto w-full max-w-4xl p-8 max-[640px]:p-5">
      <div className="mb-7 flex flex-wrap items-start justify-between gap-4">
        <div>
          <h2 className="font-sans text-2xl font-semibold text-ink">{t("settings.models.providers")}</h2>
          <p className="mt-2 max-w-xl font-sans text-sm text-ink-faint">{t("settings.models.description")}</p>
        </div>
        <button
          onClick={openCreate}
          type="button"
          className="flex h-10 shrink-0 items-center gap-2 rounded-xl bg-clay px-3.5 text-sm font-medium text-white transition hover:bg-clay/90"
        >
          <Plus size={16} /> {t("settings.models.addProvider")}
        </button>
      </div>

      {error ? (
        <p className="mb-4 rounded-xl bg-status-danger-soft px-3 py-2 text-xs text-status-danger-ink">{error}</p>
      ) : null}
      {deleteError ? (
        <p className="mb-4 rounded-xl bg-status-danger-soft px-3 py-2 text-xs text-status-danger-ink" role="alert">{deleteError}</p>
      ) : null}

      {providers.length === 0 ? (
        <div className="rounded-xl border border-dashed border-line py-12 text-center">
          <Bot size={22} className="mx-auto text-ink-faint" />
          <p className="mt-2.5 text-sm text-ink-faint">{t("settings.models.empty")}</p>
        </div>
      ) : (
        <ul className="divide-y divide-line overflow-hidden rounded-xl border border-line bg-paper">
          {providers.map((provider) => {
            const testState = tests[provider.id];
            return (
              <li key={provider.id} className="px-4 py-3.5">
                <div className="flex items-start gap-3">
                  <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-paper-hover font-sans text-sm font-semibold text-ink-soft">
                    {provider.name.trim().charAt(0).toUpperCase() || "?"}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="truncate text-sm font-medium text-ink">{provider.name}</span>
                      <span className="rounded-full bg-paper-hover px-2 py-0.5 text-[11px] text-ink-faint">{provider.kind}</span>
                    </div>
                    <p className="mt-0.5 truncate font-mono text-[11px] text-ink-faint">{provider.baseUrl}</p>
                    <div className="mt-2 flex flex-wrap gap-1.5">
                      {provider.models.length > 0 ? (
                        provider.models.map((model) => (
                          <span
                            key={model.modelId}
                            className="inline-flex items-baseline gap-1 rounded-md bg-paper-hover px-1.5 py-0.5 font-mono text-[11px] text-ink-soft"
                          >
                            {model.modelId}
                            <span className="font-sans text-[10px] text-ink-faint">{model.modelTier}</span>
                          </span>
                        ))
                      ) : (
                        <span className="text-xs text-ink-faint">{t("settings.models.noModels")}</span>
                      )}
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    <IconButton title={t("settings.models.test")} onClick={() => void handleTest(provider)}>
                      {testState === "loading" ? <Loader2 size={15} className="animate-spin" /> : <Zap size={15} />}
                    </IconButton>
                    <IconButton title={t("settings.models.edit")} onClick={() => openEdit(provider)}>
                      <Pencil size={15} />
                    </IconButton>
                    <IconButton title={t("settings.models.delete")} onClick={() => setConfirmingDeleteId(provider.id)}>
                      <Trash2 size={15} />
                    </IconButton>
                  </div>
                </div>

                {testState && testState !== "loading" ? (
                  <div className={`mt-3 rounded-lg border px-3 py-2 text-xs ${providerTestResultStyle(testState.success)}`}>
                    {testState.success ? (
                      <span className="flex items-center gap-1">
                        <Check size={13} /> {testState.message}
                      </span>
                    ) : (
                      t("settings.models.testFailed", { message: testState.message })
                    )}
                  </div>
                ) : null}

                {confirmingDeleteId === provider.id ? (
                  <div className="mt-3 flex items-center gap-1.5 rounded-lg bg-status-danger-soft px-3 py-2">
                    <span className="min-w-0 flex-1 truncate text-xs text-status-danger-ink">
                      {t("settings.models.deleteConfirm", { name: provider.name })}
                    </span>
                    <button
                      type="button"
                      aria-label={t("common.confirm")}
                      onClick={() => void handleDelete(provider)}
                      className="grid size-7 shrink-0 place-items-center rounded-md text-status-danger-ink hover:bg-status-danger-border"
                    >
                      <Check size={14} />
                    </button>
                    <button
                      type="button"
                      aria-label={t("common.cancel")}
                      onClick={() => setConfirmingDeleteId(null)}
                      className="grid size-7 shrink-0 place-items-center rounded-md text-ink-soft hover:bg-paper-hover"
                    >
                      <X size={14} />
                    </button>
                  </div>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}

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
      className="grid size-8 place-items-center rounded-lg text-ink-faint transition hover:bg-paper-hover hover:text-ink-soft disabled:opacity-40 disabled:hover:bg-transparent"
    >
      {children}
    </button>
  );
}
