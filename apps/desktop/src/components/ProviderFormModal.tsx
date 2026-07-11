import { type ReactNode, useEffect, useState } from "react";
import { Check, Eye, EyeOff, Loader2, X, Zap } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useProviderStore } from "../stores/providerStore";
import { providersApi } from "../api/providers";
import type { ModelTier, ProviderConfig, ProviderInput, ProviderKind, ProviderModel, ProviderPreset } from "../type/providers";
import { resolveErrorMessage as resolveMessage } from "../utils/commandError";

const KIND_OPTIONS: { value: ProviderKind; label: string }[] = [
  { value: "openai", label: "OpenAI · Responses API" },
  { value: "glm", label: "GLM · OpenAI-compatible" },
  { value: "kimi", label: "Kimi · OpenAI-compatible" },
  { value: "deepseek", label: "DeepSeek · OpenAI-compatible" },
  { value: "qwen", label: "Qwen · OpenAI-compatible" },
  { value: "anthropic", label: "Anthropic · /v1/messages" },
];

const inputClass =
  "h-10 w-full rounded-lg border border-line-strong bg-paper px-3 text-sm text-ink outline-none focus:ring-3 focus:ring-clay/20";

interface ProviderFormModalProps {
  open: boolean;
  mode: "create" | "edit";
  initial?: ProviderConfig;
  onClose: () => void;
}

export function ProviderFormModal({ open, mode, initial, onClose }: ProviderFormModalProps) {
  const { t } = useTranslation();
  const presets = useProviderStore((state) => state.presets);
  const create = useProviderStore((state) => state.create);
  const update = useProviderStore((state) => state.update);

  const [selectedPresetId, setSelectedPresetId] = useState("");
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [kind, setKind] = useState<ProviderKind>("openai");
  const [liteModelsText, setLiteModelsText] = useState("");
  const [plusModelsText, setPlusModelsText] = useState("");
  const [proModelsText, setProModelsText] = useState("");
  const [extraBodyText, setExtraBodyText] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [isTesting, setIsTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; message: string } | null>(null);

  useEffect(() => {
    if (!open) return;
    setError(null);
    setTestResult(null);
    if (mode === "edit" && initial) {
      setSelectedPresetId("");
      setName(initial.name);
      setBaseUrl(initial.baseUrl);
      // Provider profiles never contain credentials; editing requires an explicit replacement key.
      setApiKey("");
      setKind(initial.kind);
      setLiteModelsText(modelsTextForTier(initial.models, "lite"));
      setPlusModelsText(modelsTextForTier(initial.models, "plus"));
      setProModelsText(modelsTextForTier(initial.models, "pro"));
      setExtraBodyText("");
    } else {
      const defaultPreset = presets[0];
      setSelectedPresetId(defaultPreset?.id ?? "");
      setName(defaultPreset?.name ?? "");
      setBaseUrl(defaultPreset?.baseUrl ?? "");
      setApiKey("");
      setKind(defaultPreset?.kind ?? "openai");
      setLiteModelsText(modelsTextForTier(defaultPreset?.models ?? [], "lite"));
      setPlusModelsText(modelsTextForTier(defaultPreset?.models ?? [], "plus"));
      setProModelsText(modelsTextForTier(defaultPreset?.models ?? [], "pro"));
      setExtraBodyText("");
    }
  }, [open, mode, initial, presets]);

  if (!open) return null;

  function applyPreset(preset: ProviderPreset) {
    setSelectedPresetId(preset.id);
    if (mode !== "create") return;
    setName(preset.name);
    setBaseUrl(preset.baseUrl);
    setKind(preset.kind);
    setLiteModelsText(modelsTextForTier(preset.models, "lite"));
    setPlusModelsText(modelsTextForTier(preset.models, "plus"));
    setProModelsText(modelsTextForTier(preset.models, "pro"));
  }

  function buildInput(): ProviderInput | string {
    const trimmedName = name.trim();
    const trimmedBaseUrl = baseUrl.trim();
    const trimmedApiKey = apiKey.trim();
    if (!trimmedName) return t("settings.models.form.nameRequired");
    if (!trimmedBaseUrl) return t("settings.models.form.baseUrlRequired");
    if (!trimmedApiKey) return t("settings.models.form.apiKeyRequired");

    const models = [
      ...parseModels(liteModelsText, "lite"),
      ...parseModels(plusModelsText, "plus"),
      ...parseModels(proModelsText, "pro"),
    ];
    const modelIds = new Set<string>();
    const duplicate = models.find((model) => {
      if (modelIds.has(model.modelId)) return true;
      modelIds.add(model.modelId);
      return false;
    });
    if (duplicate) return t("settings.models.form.duplicateModel", { model: duplicate.modelId });

    let extraBody: Record<string, unknown> | undefined;
    const trimmedExtra = extraBodyText.trim();
    if (trimmedExtra) {
      try {
        const parsed = JSON.parse(trimmedExtra) as unknown;
        if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
          return t("settings.models.form.extraBodyObject");
        }
        extraBody = parsed as Record<string, unknown>;
      } catch {
        return t("settings.models.form.extraBodyInvalid");
      }
    }

    return {
      name: trimmedName,
      baseUrl: trimmedBaseUrl,
      apiKey: trimmedApiKey,
      kind,
      models,
      enabled: true,
      extraBody,
    };
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
        await create(inputOrError);
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
    if (!firstModel) {
      setError(t("settings.models.form.modelRequiredForTest"));
      return;
    }
    setError(null);
    setIsTesting(true);
    setTestResult(null);
    try {
      const result = await providersApi.testDraft(inputOrError, firstModel.modelId);
      setTestResult({ success: result.success, message: result.message });
    } catch (testError) {
      setTestResult({ success: false, message: resolveMessage(testError) });
    } finally {
      setIsTesting(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4">
      <div className="max-h-[90vh] w-full max-w-lg overflow-auto rounded-xl border border-line bg-paper shadow-xl">
        <div className="flex items-center justify-between border-b border-line px-5 py-4">
          <h2 className="m-0 text-base font-semibold text-ink">
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
                    onClick={() => applyPreset(preset)}
                    className={`rounded-full border px-3 py-1 text-xs ${
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
          ) : null}

          <Field label={t("settings.models.form.name")}>
            <input className={inputClass} value={name} onChange={(e) => setName(e.target.value)} placeholder="My DeepSeek" />
          </Field>

          <Field label={t("settings.models.form.baseUrl")}>
            <input className={inputClass} value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.deepseek.com" />
          </Field>

          <Field label={t("settings.models.form.apiKey")}>
            <div className="grid grid-cols-[minmax(0,1fr)_40px] gap-2">
              <input
                className={inputClass}
                type={showApiKey ? "text" : "password"}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-..."
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

          <Field label={t("settings.models.form.protocol")}>
            <select className={inputClass} value={kind} onChange={(e) => setKind(e.target.value as ProviderKind)}>
              {KIND_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </Field>

          <Field label={t("settings.models.form.liteModels")} hint={t("settings.models.form.liteHint")}>
            <input className={inputClass} value={liteModelsText} onChange={(e) => setLiteModelsText(e.target.value)} placeholder="deepseek-chat" />
          </Field>

          <Field label={t("settings.models.form.plusModels")} hint={t("settings.models.form.plusHint")}>
            <input className={inputClass} value={plusModelsText} onChange={(e) => setPlusModelsText(e.target.value)} placeholder="qwen-plus" />
          </Field>

          <Field label={t("settings.models.form.proModels")} hint={t("settings.models.form.proHint")}>
            <input className={inputClass} value={proModelsText} onChange={(e) => setProModelsText(e.target.value)} placeholder="deepseek-reasoner" />
          </Field>

          <Field label={t("settings.models.form.extraBody")} hint={t("settings.models.form.extraBodyHint")}>
            <textarea
              className={`${inputClass} min-h-20 resize-y font-mono text-xs`}
              value={extraBodyText}
              onChange={(e) => setExtraBodyText(e.target.value)}
              placeholder='{"reasoning_effort": "high"}'
            />
          </Field>

          {testResult ? (
            <div className={`rounded-lg border px-3 py-2 text-xs ${testResult.success ? "border-emerald-200 bg-emerald-50 text-emerald-700" : "border-rose-200 bg-rose-50 text-rose-700"}`}>
              {testResult.success ? t("settings.models.form.connectivityOk") : t("settings.models.form.failed", { message: testResult.message })}
            </div>
          ) : null}

          {error ? <p className="m-0 text-xs text-rose-600">{error}</p> : null}
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

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="grid gap-1.5">
      <span className="text-sm font-medium text-ink-soft">{label}</span>
      {children}
      {hint ? <span className="text-xs text-ink-faint">{hint}</span> : null}
    </label>
  );
}

function modelsTextForTier(
  models: Array<Pick<ProviderModel, "modelId" | "modelTier">>,
  tier: ModelTier,
): string {
  return models
    .filter((model) => model.modelTier === tier)
    .map((model) => model.modelId)
    .join(", ");
}

function parseModels(value: string, modelTier: ModelTier): ProviderModel[] {
  return value
    .split(",")
    .map((modelId) => modelId.trim())
    .filter(Boolean)
    .map((modelId) => ({ modelId, modelTier, enabled: true }));
}
