import { type ReactNode, useEffect, useRef, useState } from "react";
import { Check, Eye, EyeOff, Loader2, X, Zap } from "lucide-react";
import { useTranslation } from "react-i18next";

import { providersApi } from "@/bridge/providers";
import { resolveErrorMessage as resolveMessage } from "@/lib/commandError";
import type { ProviderConfig, ProviderInput, ProviderPreset } from "@/bridge/providerContracts";
import { useModelStore } from "../modelStore";

const inputClass =
  "h-10 w-full rounded-lg border border-line-strong bg-paper px-3 text-sm text-ink outline-none focus:ring-3 focus:ring-clay/20";

export function providerTestResultStyle(success: boolean): string {
  return success
    ? "border-status-success-border bg-status-success-soft text-status-success-ink"
    : "border-status-danger-border bg-status-danger-soft text-status-danger-ink";
}

interface ProviderFormModalProps {
  open: boolean;
  mode: "create" | "edit";
  initial?: ProviderConfig;
  onClose: () => void;
}

export type ProviderInputBuildResult =
  | { ok: true; input: ProviderInput }
  | { ok: false; errorKey: "presetRequired" | "apiKeyRequired" };

export type ProviderTestTarget =
  | { kind: "stored"; providerId: string }
  | { kind: "draft" };

export function resolveProviderTestTarget(
  mode: "create" | "edit",
  providerId: string | undefined,
  apiKey: string,
): ProviderTestTarget {
  return mode === "edit" && providerId && !apiKey.trim()
    ? { kind: "stored", providerId }
    : { kind: "draft" };
}

/** Builds a provider from a preset. Model capabilities stay empty, so Core reads them from the model catalog. */
export function presetProviderInput(
  preset: ProviderPreset | undefined,
  apiKey: string,
): ProviderInputBuildResult {
  if (!preset) return { ok: false, errorKey: "presetRequired" };
  const trimmedApiKey = apiKey.trim();
  if (!trimmedApiKey) return { ok: false, errorKey: "apiKeyRequired" };
  return {
    ok: true,
    input: {
      name: preset.name,
      baseUrl: preset.baseUrl,
      apiKey: trimmedApiKey,
      models: preset.models.map((model) => ({ modelId: model.modelId, enabled: true })),
      enabled: true,
    },
  };
}

/** Keeps every stored setting. A blank key keeps the stored key. */
export function apiKeyUpdateInput(provider: ProviderConfig, apiKey: string): ProviderInput {
  const { id: _id, hasApiKey: _hasApiKey, resolvedModels: _resolvedModels, ...settings } = provider;
  return { ...settings, apiKey: apiKey.trim() || undefined };
}

export function ProviderFormModal({ open, mode, initial, onClose }: ProviderFormModalProps) {
  const { t } = useTranslation();
  const presets = useModelStore((state) => state.presets);
  const create = useModelStore((state) => state.create);
  const providers = useModelStore((state) => state.providers);
  const update = useModelStore((state) => state.update);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);

  const [selectedPresetId, setSelectedPresetId] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [isTesting, setIsTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; message: string } | null>(null);

  const isConfigured = (presetId: string) => providers.some((provider) => provider.id === presetId);

  useEffect(() => {
    if (!open) return;
    setError(null);
    setTestResult(null);
    setApiKey("");
    if (mode === "create") {
      const firstAvailable = presets.find((preset) => !providers.some((provider) => provider.id === preset.id));
      setSelectedPresetId(firstAvailable?.id ?? "");
    }
    // Reset only when the dialog opens; a provider list refresh must not clear the typed key.
  }, [open, mode, initial, presets]);

  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement as HTMLElement | null;
    dialogRef.current?.focus();
    return () => previousFocus.current?.focus();
  }, [open]);

  if (!open) return null;

  function buildInput(): ProviderInput | string {
    if (mode === "edit" && initial) return apiKeyUpdateInput(initial, apiKey);
    const result = presetProviderInput(
      presets.find((preset) => preset.id === selectedPresetId),
      apiKey,
    );
    return result.ok ? result.input : t(`settings.models.form.${result.errorKey}`);
  }

  async function handleSubmit() {
    const inputOrError = buildInput();
    if (typeof inputOrError === "string") {
      setError(inputOrError);
      return;
    }
    setError(null);
    setIsSubmitting(true);
    try {
      if (mode === "edit" && initial) {
        await update(initial.id, inputOrError);
      } else {
        await create(selectedPresetId, inputOrError);
      }
      onClose();
    } catch (submitError) {
      setError(resolveMessage(submitError));
    } finally {
      setIsSubmitting(false);
    }
  }

  async function handleTest() {
    const inputOrError = buildInput();
    if (typeof inputOrError === "string") {
      setError(inputOrError);
      return;
    }
    const firstModel = inputOrError.models.find((model) => model.enabled);
    if (!firstModel) return;
    setError(null);
    setIsTesting(true);
    setTestResult(null);
    try {
      const target = resolveProviderTestTarget(mode, initial?.id, inputOrError.apiKey ?? "");
      const result = target.kind === "stored"
        ? await providersApi.test(target.providerId, firstModel.modelId)
        : await providersApi.testDraft(inputOrError, firstModel.modelId);
      setTestResult({ success: result.success, message: result.message });
    } catch (testError) {
      setTestResult({ success: false, message: resolveMessage(testError) });
    } finally {
      setIsTesting(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="provider-form-title"
        tabIndex={-1}
        onKeyDown={(event) => {
          if (event.key === "Escape") onClose();
        }}
        className="max-h-[90vh] w-full max-w-lg overflow-auto rounded-xl border border-line bg-paper shadow-xl outline-none"
      >
        <div className="flex items-center justify-between border-b border-line px-5 py-4">
          <h2 id="provider-form-title" className="m-0 text-base font-semibold text-ink">
            {mode === "edit" ? t("settings.models.form.editTitle") : t("settings.models.form.addTitle")}
          </h2>
          <button
            className="grid size-8 place-items-center rounded-lg text-ink-faint hover:bg-paper-hover"
            type="button"
            onClick={onClose}
            aria-label={t("settings.models.form.close")}
          >
            <X size={16} />
          </button>
        </div>

        <div className="grid gap-4 px-5 py-4">
          {mode === "create" ? (
            <div className="grid gap-1.5">
              <span className="text-sm font-medium text-ink-soft">{t("settings.models.form.preset")}</span>
              <div className="flex flex-wrap gap-2">
                {presets.map((preset) => (
                  <button
                    key={preset.id}
                    type="button"
                    disabled={isConfigured(preset.id)}
                    onClick={() => setSelectedPresetId(preset.id)}
                    className={`rounded-full border px-3 py-1 text-xs disabled:cursor-not-allowed disabled:opacity-40 ${
                      selectedPresetId === preset.id
                        ? "border-clay bg-clay-soft text-clay"
                        : "border-line-strong text-ink-soft hover:bg-paper-hover"
                    }`}
                  >
                    {preset.name}
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <p className="m-0 text-sm text-ink">{initial?.name}</p>
          )}

          <Field label={t("settings.models.form.apiKey")}>
            <div className="grid grid-cols-[minmax(0,1fr)_40px] gap-2">
              <input
                className={inputClass}
                type={showApiKey ? "text" : "password"}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder={mode === "edit" ? t("settings.models.form.apiKeyKeep") : "sk-..."}
                autoComplete="off"
                spellCheck={false}
              />
              <button
                type="button"
                className="grid size-10 place-items-center rounded-lg border border-line-strong bg-paper text-ink-soft hover:bg-paper-hover"
                onClick={() => setShowApiKey((value) => !value)}
                aria-label={showApiKey ? t("settings.models.form.hideApiKey") : t("settings.models.form.showApiKey")}
              >
                {showApiKey ? <EyeOff size={16} /> : <Eye size={16} />}
              </button>
            </div>
          </Field>

          {testResult ? (
            <div className={`rounded-lg border px-3 py-2 text-xs ${providerTestResultStyle(testResult.success)}`}>
              {testResult.success ? t("settings.models.form.connectivityOk") : t("settings.models.form.failed", { message: testResult.message })}
            </div>
          ) : null}

          {error ? <p className="m-0 text-xs text-status-danger-ink">{error}</p> : null}
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-line px-5 py-4">
          <button
            type="button"
            onClick={handleTest}
            disabled={isTesting}
            className="flex items-center gap-2 rounded-lg border border-line-strong bg-paper px-3 py-2 text-sm text-ink-soft hover:bg-paper-hover disabled:opacity-60"
          >
            {isTesting ? <Loader2 size={16} className="animate-spin" /> : <Zap size={16} />}
            {t("settings.models.form.test")}
          </button>
          <div className="flex items-center gap-2">
            <button type="button" onClick={onClose} className="rounded-lg border border-line-strong bg-paper px-4 py-2 text-sm text-ink-soft hover:bg-paper-hover">
              {t("common.cancel")}
            </button>
            <button
              type="button"
              onClick={handleSubmit}
              disabled={isSubmitting}
              className="flex items-center gap-2 rounded-lg bg-clay px-4 py-2 text-sm font-medium text-white transition hover:bg-clay/90 disabled:opacity-60"
            >
              {isSubmitting ? <Loader2 size={16} className="animate-spin" /> : <Check size={16} />}
              {t("settings.models.form.save")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function Field({ label, className, children }: { label: string; className?: string; children: ReactNode }) {
  return (
    <label className={`grid gap-1.5 ${className ?? ""}`}>
      <span className="text-sm font-medium text-ink-soft">{label}</span>
      {children}
    </label>
  );
}
