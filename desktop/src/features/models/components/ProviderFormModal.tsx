import { type ReactNode, useEffect, useRef, useState } from "react";
import { Check, Eye, EyeOff, Loader2, X, Zap } from "lucide-react";
import { useTranslation } from "react-i18next";

import { providersApi } from "@/bridge/providers";
import { resolveErrorMessage as resolveMessage } from "@/lib/commandError";
import type { ModelCapabilities, ModelTier, ProviderConfig, ProviderInput, ProviderKind, ProviderModel, ProviderPreset } from "../contracts";
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
  focusModelId?: string;
  onClose: () => void;
}

export interface ModelCapabilitiesDraft {
  contextWindowTokens: string;
  maxOutputTokens: string;
  maxReasoningTokens: string;
  acceptsDataBlocks: boolean;
}

export interface ProviderFormDraft {
  name: string;
  baseUrl: string;
  apiKey: string;
  kind: ProviderKind;
  liteModelsText: string;
  plusModelsText: string;
  proModelsText: string;
  extraBodyText: string;
  capabilitiesByModel: Record<string, ModelCapabilitiesDraft>;
}

type ProviderFormErrorKey =
  | "nameRequired"
  | "baseUrlRequired"
  | "apiKeyRequired"
  | "modelRequired"
  | "duplicateModel"
  | "contextWindowTokensInvalid"
  | "maxOutputTokensInvalid"
  | "maxReasoningTokensInvalid"
  | "generationReservationExhaustsWindow"
  | "extraBodyObject"
  | "extraBodyInvalid";

export type ProviderInputBuildResult =
  | { ok: true; input: ProviderInput }
  | { ok: false; errorKey: ProviderFormErrorKey; model?: string };

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

export function buildProviderInput(
  draft: ProviderFormDraft,
  mode: "create" | "edit",
): ProviderInputBuildResult {
  const trimmedName = draft.name.trim();
  const trimmedBaseUrl = draft.baseUrl.trim();
  const trimmedApiKey = draft.apiKey.trim();
  if (!trimmedName) return { ok: false, errorKey: "nameRequired" };
  if (!trimmedBaseUrl) return { ok: false, errorKey: "baseUrlRequired" };
  if (mode === "create" && !trimmedApiKey) {
    return { ok: false, errorKey: "apiKeyRequired" };
  }

  const models = [
    ...parseModels(draft.liteModelsText, "lite"),
    ...parseModels(draft.plusModelsText, "plus"),
    ...parseModels(draft.proModelsText, "pro"),
  ];
  if (models.length === 0) return { ok: false, errorKey: "modelRequired" };
  const modelIds = new Set<string>();
  const duplicate = models.find((model) => {
    if (modelIds.has(model.modelId)) return true;
    modelIds.add(model.modelId);
    return false;
  });
  if (duplicate) {
    return { ok: false, errorKey: "duplicateModel", model: duplicate.modelId };
  }

  for (const model of models) {
    const capabilityDraft = draft.capabilitiesByModel[model.modelId];
    const contextWindowTokens = parseUnsignedInteger(
      capabilityDraft?.contextWindowTokens ?? "",
      true,
      Number.MAX_SAFE_INTEGER,
    );
    if (contextWindowTokens === null) {
      return { ok: false, errorKey: "contextWindowTokensInvalid", model: model.modelId };
    }
    const maxOutputTokens = parseUnsignedInteger(
      capabilityDraft?.maxOutputTokens ?? "",
      true,
      0xffff_ffff,
    );
    if (maxOutputTokens === null) {
      return { ok: false, errorKey: "maxOutputTokensInvalid", model: model.modelId };
    }
    const reasoningText = capabilityDraft?.maxReasoningTokens.trim() ?? "";
    const maxReasoningTokens = reasoningText === ""
      ? null
      : parseUnsignedInteger(reasoningText, false, 0xffff_ffff);
    if (reasoningText !== "" && maxReasoningTokens === null) {
      return { ok: false, errorKey: "maxReasoningTokensInvalid", model: model.modelId };
    }
    if (maxOutputTokens + (maxReasoningTokens ?? 0) >= contextWindowTokens) {
      return {
        ok: false,
        errorKey: "generationReservationExhaustsWindow",
        model: model.modelId,
      };
    }
    model.capabilities = {
      contextWindowTokens,
      maxOutputTokens,
      maxReasoningTokens,
      acceptsDataBlocks: capabilityDraft?.acceptsDataBlocks ?? false,
    };
  }

  let extraBody: Record<string, unknown> | undefined;
  const trimmedExtra = draft.extraBodyText.trim();
  if (trimmedExtra) {
    try {
      const parsed = JSON.parse(trimmedExtra) as unknown;
      if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
        return { ok: false, errorKey: "extraBodyObject" };
      }
      extraBody = parsed as Record<string, unknown>;
    } catch {
      return { ok: false, errorKey: "extraBodyInvalid" };
    }
  }

  return {
    ok: true,
    input: {
      name: trimmedName,
      baseUrl: trimmedBaseUrl,
      apiKey: trimmedApiKey,
      kind: draft.kind,
      models,
      enabled: true,
      extraBody,
    },
  };
}

export function ProviderFormModal({ open, mode, initial, focusModelId, onClose }: ProviderFormModalProps) {
  const { t } = useTranslation();
  const presets = useModelStore((state) => state.presets);
  const create = useModelStore((state) => state.create);
  const update = useModelStore((state) => state.update);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);

  const [selectedPresetId, setSelectedPresetId] = useState("");
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [kind, setKind] = useState<ProviderKind>("openai");
  const [liteModelsText, setLiteModelsText] = useState("");
  const [plusModelsText, setPlusModelsText] = useState("");
  const [proModelsText, setProModelsText] = useState("");
  const [extraBodyText, setExtraBodyText] = useState("");
  const [capabilitiesByModel, setCapabilitiesByModel] = useState<Record<string, ModelCapabilitiesDraft>>({});
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
      // Provider profiles never contain credentials; a blank edit keeps the encrypted key in Core.
      setApiKey("");
      setKind(initial.kind);
      setLiteModelsText(modelsTextForTier(initial.models, "lite"));
      setPlusModelsText(modelsTextForTier(initial.models, "plus"));
      setProModelsText(modelsTextForTier(initial.models, "pro"));
      setExtraBodyText("");
      setCapabilitiesByModel(capabilityDraftsFor(initial.models));
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
      setCapabilitiesByModel(capabilityDraftsFor(defaultPreset?.models ?? []));
    }
  }, [open, mode, initial, presets]);

  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement as HTMLElement | null;
    dialogRef.current?.focus();
    return () => previousFocus.current?.focus();
  }, [open]);

  const configuredModels = [
    ...parseModels(liteModelsText, "lite"),
    ...parseModels(plusModelsText, "plus"),
    ...parseModels(proModelsText, "pro"),
  ].filter((model, index, models) => (
    models.findIndex((candidate) => candidate.modelId === model.modelId) === index
  ));
  const configuredModelIds = configuredModels.map((model) => model.modelId).join("\n");

  useEffect(() => {
    if (!open || !focusModelId || !configuredModelIds.split("\n").includes(focusModelId)) return;
    const editor = document.getElementById(capabilitiesEditorId(focusModelId));
    editor?.scrollIntoView?.({ block: "center" });
    editor?.querySelector<HTMLInputElement>("input")?.focus();
  }, [configuredModelIds, focusModelId, open]);

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
    setCapabilitiesByModel(capabilityDraftsFor(preset.models));
  }

  function updateCapability(
    modelId: string,
    field: keyof ModelCapabilitiesDraft,
    value: string | boolean,
  ) {
    setCapabilitiesByModel((current) => ({
      ...current,
      [modelId]: {
        ...emptyCapabilitiesDraft(),
        ...current[modelId],
        [field]: value,
      },
    }));
  }

  function buildInput(): ProviderInput | string {
    const result = buildProviderInput({
      name,
      baseUrl,
      apiKey,
      kind,
      liteModelsText,
      plusModelsText,
      proModelsText,
      extraBodyText,
      capabilitiesByModel,
    }, mode);
    if (result.ok) return result.input;
    return t(`settings.models.form.${result.errorKey}`, { model: result.model });
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
      const target = resolveProviderTestTarget(mode, initial?.id, inputOrError.apiKey);
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
        className="max-h-[90vh] w-full max-w-3xl overflow-auto rounded-xl border border-line bg-paper shadow-xl outline-none"
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

          <div data-provider-form-grid="true" className="grid gap-4 sm:grid-cols-2">
            <Field label={t("settings.models.form.name")}>
              <input className={inputClass} value={name} onChange={(e) => setName(e.target.value)} placeholder="My DeepSeek" />
            </Field>

            <Field label={t("settings.models.form.baseUrl")}>
              <input className={inputClass} value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.deepseek.com" />
            </Field>

            <Field label={t("settings.models.form.apiKey")} className="sm:col-span-2">
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
              {mode === "edit" ? (
                <span className="text-xs text-ink-faint">{t("settings.models.form.apiKeyKeep")}</span>
              ) : null}
            </Field>

            <Field label={t("settings.models.form.liteModels")}>
              <input className={inputClass} value={liteModelsText} onChange={(e) => setLiteModelsText(e.target.value)} placeholder="deepseek-chat" />
            </Field>

            <Field label={t("settings.models.form.plusModels")}>
              <input className={inputClass} value={plusModelsText} onChange={(e) => setPlusModelsText(e.target.value)} placeholder="qwen-plus" />
            </Field>

            <Field label={t("settings.models.form.proModels")}>
              <input className={inputClass} value={proModelsText} onChange={(e) => setProModelsText(e.target.value)} placeholder="deepseek-reasoner" />
            </Field>

            <Field label={t("settings.models.form.extraBody")}>
              <textarea
                className={`${inputClass} min-h-10 resize-y py-2 font-mono text-xs`}
                value={extraBodyText}
                onChange={(e) => setExtraBodyText(e.target.value)}
                placeholder='{"reasoning_effort": "high"}'
              />
            </Field>

            <div className="grid gap-3 sm:col-span-2">
              <span className="text-sm font-medium text-ink-soft">
                {t("settings.models.form.capabilities")}
              </span>
              {configuredModels.map((model) => {
                const capability = capabilitiesByModel[model.modelId] ?? emptyCapabilitiesDraft();
                return (
                  <fieldset
                    id={capabilitiesEditorId(model.modelId)}
                    key={model.modelId}
                    className="grid gap-3 rounded-lg border border-line p-3"
                  >
                    <legend className="px-1 font-mono text-xs text-ink-soft">{model.modelId}</legend>
                    <div className="grid gap-3 sm:grid-cols-3">
                      <Field label={t("settings.models.form.contextWindowTokens")}>
                        <input
                          className={inputClass}
                          type="number"
                          min={1}
                          step={1}
                          value={capability.contextWindowTokens}
                          onChange={(event) => updateCapability(model.modelId, "contextWindowTokens", event.target.value)}
                        />
                      </Field>
                      <Field label={t("settings.models.form.maxOutputTokens")}>
                        <input
                          className={inputClass}
                          type="number"
                          min={1}
                          step={1}
                          value={capability.maxOutputTokens}
                          onChange={(event) => updateCapability(model.modelId, "maxOutputTokens", event.target.value)}
                        />
                      </Field>
                      <Field label={t("settings.models.form.maxReasoningTokens")}>
                        <input
                          className={inputClass}
                          type="number"
                          min={0}
                          step={1}
                          value={capability.maxReasoningTokens}
                          placeholder={t("settings.models.form.optional")}
                          onChange={(event) => updateCapability(model.modelId, "maxReasoningTokens", event.target.value)}
                        />
                      </Field>
                    </div>
                    <label className="flex items-center gap-2 text-xs text-ink-soft">
                      <input
                        type="checkbox"
                        checked={capability.acceptsDataBlocks}
                        onChange={(event) => updateCapability(model.modelId, "acceptsDataBlocks", event.target.checked)}
                      />
                      {t("settings.models.form.acceptsDataBlocks")}
                    </label>
                  </fieldset>
                );
              })}
            </div>
          </div>

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

function capabilityDraftsFor(
  models: Array<Pick<ProviderModel, "modelId" | "capabilities">>,
): Record<string, ModelCapabilitiesDraft> {
  return Object.fromEntries(models.map((model) => [
    model.modelId,
    model.capabilities ? capabilitiesToDraft(model.capabilities) : emptyCapabilitiesDraft(),
  ]));
}

function capabilitiesToDraft(capabilities: ModelCapabilities): ModelCapabilitiesDraft {
  return {
    contextWindowTokens: String(capabilities.contextWindowTokens),
    maxOutputTokens: String(capabilities.maxOutputTokens),
    maxReasoningTokens: capabilities.maxReasoningTokens === null
      ? ""
      : String(capabilities.maxReasoningTokens),
    acceptsDataBlocks: capabilities.acceptsDataBlocks,
  };
}

function emptyCapabilitiesDraft(): ModelCapabilitiesDraft {
  return {
    contextWindowTokens: "",
    maxOutputTokens: "",
    maxReasoningTokens: "",
    acceptsDataBlocks: false,
  };
}

function parseUnsignedInteger(value: string, positive: boolean, max: number): number | null {
  const parsed = Number(value);
  if (!value.trim() || !Number.isSafeInteger(parsed) || parsed > max) return null;
  if (positive ? parsed <= 0 : parsed < 0) return null;
  return parsed;
}

function capabilitiesEditorId(modelId: string): string {
  return `model-capabilities-${modelId}`;
}
