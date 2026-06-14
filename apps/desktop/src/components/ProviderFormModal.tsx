import { type ReactNode, useEffect, useState } from "react";
import { Check, Eye, EyeOff, Loader2, X, Zap } from "lucide-react";

import { useProviderStore } from "../stores/providerStore";
import type { ProviderConfig, ProviderInput, ProviderKind, ProviderPreset } from "../type/providers";

const KIND_OPTIONS: { value: ProviderKind; label: string }[] = [
  { value: "openai", label: "OpenAI · Responses API" },
  { value: "glm", label: "GLM · OpenAI-compatible" },
  { value: "kimi", label: "Kimi · OpenAI-compatible" },
  { value: "deepseek", label: "DeepSeek · OpenAI-compatible" },
  { value: "qwen", label: "Qwen · OpenAI-compatible" },
  { value: "anthropic", label: "Anthropic · /v1/messages" },
  { value: "openai_compatible", label: "Custom · OpenAI-compatible" },
];

const inputClass =
  "h-10 w-full rounded-lg border border-slate-300 bg-white px-3 text-sm text-slate-900 outline-none focus:ring-3 focus:ring-teal-700/15";

interface ProviderFormModalProps {
  open: boolean;
  mode: "create" | "edit";
  initial?: ProviderConfig;
  onClose: () => void;
}

export function ProviderFormModal({ open, mode, initial, onClose }: ProviderFormModalProps) {
  const presets = useProviderStore((state) => state.presets);
  const create = useProviderStore((state) => state.create);
  const update = useProviderStore((state) => state.update);
  const test = useProviderStore((state) => state.test);

  const [selectedPresetId, setSelectedPresetId] = useState("custom");
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [kind, setKind] = useState<ProviderKind>("openai_compatible");
  const [modelsText, setModelsText] = useState("");
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
      setSelectedPresetId("custom");
      setName(initial.name);
      setBaseUrl(initial.baseUrl);
      setApiKey(initial.apiKey);
      setKind(initial.kind);
      setModelsText(initial.models.join(", "));
      setExtraBodyText(initial.extraBody ? JSON.stringify(initial.extraBody, null, 2) : "");
    } else {
      setSelectedPresetId("custom");
      setName("");
      setBaseUrl("");
      setApiKey("");
      setKind("openai_compatible");
      setModelsText("");
      setExtraBodyText("");
    }
  }, [open, mode, initial]);

  if (!open) return null;

  function applyPreset(preset: ProviderPreset) {
    setSelectedPresetId(preset.id);
    if (mode !== "create") return;
    setName(preset.name);
    setBaseUrl(preset.baseUrl);
    setKind(preset.kind);
    setModelsText(preset.models.join(", "));
  }

  function buildInput(): ProviderInput | string {
    const trimmedName = name.trim();
    const trimmedBaseUrl = baseUrl.trim();
    const trimmedApiKey = apiKey.trim();
    if (!trimmedName) return "Name is required";
    if (!trimmedBaseUrl) return "Base URL is required";
    if (!trimmedApiKey) return "API key is required";

    const models = modelsText
      .split(",")
      .map((value) => value.trim())
      .filter(Boolean);

    let extraBody: Record<string, unknown> | undefined;
    const trimmedExtra = extraBodyText.trim();
    if (trimmedExtra) {
      try {
        const parsed = JSON.parse(trimmedExtra) as unknown;
        if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
          return "Extra body must be a JSON object";
        }
        extraBody = parsed as Record<string, unknown>;
      } catch {
        return "Extra body is not valid JSON";
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
    const firstModel = inputOrError.models[0];
    if (!firstModel) {
      setError("Add at least one model to test");
      return;
    }
    setError(null);
    setIsTesting(true);
    setTestResult(null);
    try {
      const draftConfig: ProviderConfig = { id: "draft", ...inputOrError };
      const result = await test(draftConfig, firstModel);
      setTestResult({ success: result.success, message: result.message });
    } catch (testError) {
      setTestResult({ success: false, message: resolveMessage(testError) });
    } finally {
      setIsTesting(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-slate-900/40 p-4">
      <div className="max-h-[90vh] w-full max-w-lg overflow-auto rounded-xl border border-slate-200 bg-white shadow-xl">
        <div className="flex items-center justify-between border-b border-slate-200 px-5 py-4">
          <h2 className="m-0 text-base font-semibold text-slate-900">
            {mode === "edit" ? "Edit provider" : "Add provider"}
          </h2>
          <button
            className="grid size-8 place-items-center rounded-lg text-slate-500 hover:bg-slate-100"
            type="button"
            onClick={onClose}
            aria-label="Close"
          >
            <X size={16} />
          </button>
        </div>

        <div className="grid gap-4 px-5 py-4">
          {mode === "create" ? (
            <div className="grid gap-1.5">
              <span className="text-sm font-medium text-slate-700">Preset</span>
              <div className="flex flex-wrap gap-2">
                {presets.map((preset) => (
                  <button
                    key={preset.id}
                    type="button"
                    onClick={() => applyPreset(preset)}
                    className={`rounded-full border px-3 py-1 text-xs ${
                      selectedPresetId === preset.id
                        ? "border-teal-700 bg-teal-50 text-teal-700"
                        : "border-slate-300 text-slate-600 hover:bg-slate-50"
                    }`}
                  >
                    {preset.name}
                  </button>
                ))}
              </div>
            </div>
          ) : null}

          <Field label="Name">
            <input className={inputClass} value={name} onChange={(e) => setName(e.target.value)} placeholder="My DeepSeek" />
          </Field>

          <Field label="Base URL">
            <input className={inputClass} value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.deepseek.com" />
          </Field>

          <Field label="API key">
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
                className="grid size-10 place-items-center rounded-lg border border-slate-300 bg-white text-slate-600 hover:bg-slate-100"
                onClick={() => setShowApiKey((value) => !value)}
                aria-label={showApiKey ? "Hide API key" : "Show API key"}
              >
                {showApiKey ? <EyeOff size={16} /> : <Eye size={16} />}
              </button>
            </div>
          </Field>

          <Field label="Protocol">
            <select className={inputClass} value={kind} onChange={(e) => setKind(e.target.value as ProviderKind)}>
              {KIND_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </Field>

          <Field label="Models" hint="Comma-separated model IDs">
            <input className={inputClass} value={modelsText} onChange={(e) => setModelsText(e.target.value)} placeholder="deepseek-chat, deepseek-reasoner" />
          </Field>

          <Field label="Extra body" hint="Optional JSON object merged into the request body">
            <textarea
              className={`${inputClass} min-h-20 resize-y font-mono text-xs`}
              value={extraBodyText}
              onChange={(e) => setExtraBodyText(e.target.value)}
              placeholder='{"reasoning_effort": "high"}'
            />
          </Field>

          {testResult ? (
            <div className={`rounded-lg border px-3 py-2 text-xs ${testResult.success ? "border-emerald-200 bg-emerald-50 text-emerald-700" : "border-rose-200 bg-rose-50 text-rose-700"}`}>
              {testResult.success ? "Connectivity OK" : `Failed: ${testResult.message}`}
            </div>
          ) : null}

          {error ? <p className="m-0 text-xs text-rose-600">{error}</p> : null}
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-slate-200 px-5 py-4">
          <button
            type="button"
            onClick={handleTest}
            disabled={isTesting}
            className="flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-700 hover:bg-slate-50 disabled:opacity-60"
          >
            {isTesting ? <Loader2 size={16} className="animate-spin" /> : <Zap size={16} />}
            Test
          </button>
          <div className="flex items-center gap-2">
            <button type="button" onClick={onClose} className="rounded-lg border border-slate-300 bg-white px-4 py-2 text-sm text-slate-700 hover:bg-slate-50">
              Cancel
            </button>
            <button
              type="button"
              onClick={handleSubmit}
              disabled={isSubmitting}
              className="flex items-center gap-2 rounded-lg bg-teal-700 px-4 py-2 text-sm font-medium text-white hover:bg-teal-800 disabled:opacity-60"
            >
              {isSubmitting ? <Loader2 size={16} className="animate-spin" /> : <Check size={16} />}
              Save
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
      <span className="text-sm font-medium text-slate-700">{label}</span>
      {children}
      {hint ? <span className="text-xs text-slate-400">{hint}</span> : null}
    </label>
  );
}

function resolveMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Unexpected error";
}
