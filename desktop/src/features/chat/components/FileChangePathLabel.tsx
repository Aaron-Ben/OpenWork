interface FileChangePathLabelProps {
  path: string
  workspaceRoot?: string
  additions: number
  deletions: number
}

export function workspaceDisplayName(workspaceRoot?: string): string | null {
  if (!workspaceRoot) return null
  const normalized = workspaceRoot.replace(/[\\/]+$/, '')
  const segments = normalized.split(/[\\/]/).filter(Boolean)
  return segments[segments.length - 1] ?? null
}

export function projectFilePath(path: string, workspaceRoot?: string): string {
  const projectName = workspaceDisplayName(workspaceRoot)
  if (!workspaceRoot || !projectName) return path

  const normalizedRoot = workspaceRoot.replace(/\\/g, '/').replace(/\/+$/, '')
  const normalizedPath = path.replace(/\\/g, '/')
  if (normalizedPath === normalizedRoot) return projectName
  if (normalizedPath.startsWith(`${normalizedRoot}/`)) {
    return `${projectName}/${normalizedPath.slice(normalizedRoot.length + 1)}`
  }
  if (!normalizedPath.startsWith('/')) {
    const relativePath = normalizedPath.replace(/^\.\//, '')
    return relativePath === projectName || relativePath.startsWith(`${projectName}/`)
      ? relativePath
      : `${projectName}/${relativePath}`
  }
  return path
}

export function FileChangePathLabel({
  path,
  workspaceRoot,
  additions,
  deletions,
}: FileChangePathLabelProps) {
  const displayPath = projectFilePath(path, workspaceRoot)
  const separator = displayPath.lastIndexOf('/')
  const directory = separator >= 0 ? displayPath.slice(0, separator + 1) : ''
  const fileName = separator >= 0 ? displayPath.slice(separator + 1) : displayPath
  const tone = additions > 0 ? 'addition' : deletions > 0 ? 'deletion' : 'neutral'

  return (
    <span className="flex min-w-0 flex-1 items-center gap-2">
      <span
        aria-hidden="true"
        data-file-change-tone={tone}
        className={`size-1.5 shrink-0 rounded-full ${
          tone === 'addition'
            ? 'bg-status-success'
            : tone === 'deletion'
              ? 'bg-clay'
              : 'bg-ink-faint'
        }`}
      />
      <span
        data-file-change-path={displayPath}
        title={path}
        className="min-w-0 truncate font-mono text-xs"
      >
        {directory ? <span className="text-ink-faint">{directory}</span> : null}
        <span className="font-semibold text-ink">{fileName}</span>
      </span>
    </span>
  )
}
