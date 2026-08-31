import { ChevronDown } from 'lucide-react'

export function ScrollToLatestButton({ visible, label, onClick }: { visible: boolean; label: string; onClick: () => void }) {
  if (!visible) return null
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className="absolute bottom-3 right-4 z-20 grid size-9 place-items-center rounded-full border border-line bg-paper/95 text-ink-muted shadow-[0_10px_28px_rgba(31,30,29,0.14)] backdrop-blur transition hover:border-line-strong hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35"
      onClick={onClick}
    >
      <ChevronDown size={17} />
    </button>
  )
}
