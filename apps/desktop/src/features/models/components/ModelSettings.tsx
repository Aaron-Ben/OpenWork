import { type ReactNode, useState } from "react";
import { Check, Loader2, Pencil, Plus, Trash2, Zap } from "lucide-react";
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
    if (!window.confirm(t("settings.models.deleteConfirm", { name: provider.name }))) return;
    try {
      await remove(provider.id);
    } catch (deleteError) {
      window.alert(resolveMessage(deleteError));
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
    <div className="grid content-start gap-5 p-6 max-[560px]:px-4">
      <section className="rounded-lg border border-line bg-paper p-5">
        <div className="flex items-start justify-between gap-4 border-b border-line pb-4">
          <div>
            <h2 className="m-0 text-lg font-semibold text-ink">{t("settings.models.providers")}</h2>
            <p className="m-0 text-xs text-ink-faint">{t("settings.models.description")}</p>
          </div>
          <button
            onClick={openCreate}
            type="button"
            className="flex items-center gap-2 rounded-lg bg-clay px-3 py-2 text-sm font-medium text-white transition hover:bg-clay/90"
          >
            <Plus size={16} /> {t("settings.models.addProvider")}
          </button>
        </div>

        {error ? <p className="mt-4 text-xs text-status-danger-ink">{error}</p> : null}

        {providers.length === 0 ? (
          <p className="mt-6 text-sm text-ink-faint">{t("settings.models.empty")}</p>
        ) : (
          <ul className="mt-4 grid gap-3">
            {providers.map((provider) => {
              const testState = tests[provider.id];
              return (
                <li key={provider.id} className="rounded-lg border border-line bg-paper p-4">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <strong className="text-sm text-ink">{provider.name}</strong>
                        <span className="rounded-full bg-paper-hover px-2 py-0.5 text-xs text-ink-faint">{provider.kind}</span>
                      </div>
                      <p className="m-0 mt-1 break-words text-xs text-ink-faint">{provider.baseUrl}</p>
                      <p className="m-0 mt-0.5 text-xs text-ink-faint">
                        {provider.models.map((model) => `${model.modelId} (${model.modelTier})`).join(", ") || t("settings.models.noModels")}
                      </p>
                    </div>
                    <div className="flex shrink-0 items-center gap-1">
                      <IconButton title={t("settings.models.test")} onClick={() => handleTest(provider)}>
                        {testState === "loading" ? <Loader2 size={15} className="animate-spin" /> : <Zap size={15} />}
                      </IconButton>
                      <IconButton title={t("settings.models.edit")} onClick={() => openEdit(provider)}>
                        <Pencil size={15} />
                      </IconButton>
                      <IconButton title={t("settings.models.delete")} onClick={() => handleDelete(provider)}>
                        <Trash2 size={15} />
                      </IconButton>
                    </div>
                  </div>
                  {testState && testState !== "loading" ? (
                    <div className={`mt-3 rounded-md border px-3 py-2 text-xs ${providerTestResultStyle(testState.success)}`}>
                      {testState.success ? (
                        <span className="flex items-center gap-1">
                          <Check size={13} /> {testState.message}
                        </span>
                      ) : (
                        t("settings.models.testFailed", { message: testState.message })
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
      className="grid size-8 place-items-center rounded-lg border border-transparent text-ink-faint hover:border-line hover:bg-paper-hover disabled:opacity-40 disabled:hover:border-transparent disabled:hover:bg-transparent"
    >
      {children}
    </button>
  );
}
