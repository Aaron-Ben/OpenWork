import { useEffect, useState } from "react";
import { type ConnectionStatus, createServerClient, loadStatus } from "./status";

const client = createServerClient(window.crew.serverUrl, window.crew.desktopToken);

function Row({ label, ok, detail }: { label: string; ok: boolean; detail: string }) {
  return (
    <div className="flex items-center justify-between gap-6 py-2">
      <span className="text-neutral-700">{label}</span>
      <span className={ok ? "text-emerald-600" : "text-amber-600"}>{detail}</span>
    </div>
  );
}

export function App() {
  const [status, setStatus] = useState<ConnectionStatus | undefined>();

  useEffect(() => {
    void loadStatus(client).then(setStatus);
  }, []);

  return (
    <main className="flex min-h-screen items-center justify-center bg-neutral-50 font-sans">
      <section className="w-80 rounded-lg border border-neutral-200 bg-white p-6 shadow-sm">
        <h1 className="mb-4 text-lg font-semibold text-neutral-900">Crew</h1>
        {status === undefined && <p className="text-neutral-500">连接中…</p>}
        {status?.kind === "connected" && (
          <div className="divide-y divide-neutral-100">
            <Row label="Server" ok detail="已连接" />
            <Row
              label="Computer"
              ok={status.computerConnected}
              detail={status.computerConnected ? "已连接" : "未连接"}
            />
          </div>
        )}
        {status?.kind === "failed" && <p className="text-red-600">无法连接 Server：{status.reason}</p>}
      </section>
    </main>
  );
}
