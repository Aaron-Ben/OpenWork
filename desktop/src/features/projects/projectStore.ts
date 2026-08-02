import { create } from 'zustand'

const PROJECTS_STORAGE_KEY = 'openwork-opened-projects-v1'

export interface OpenedProject {
  name: string
  path: string
}

interface StoredProjects {
  projects: OpenedProject[]
  activeProjectPath: string | null
  projectsExpanded: boolean
  collapsedProjectPaths: string[]
}

interface ProjectStoreState extends StoredProjects {
  openDirectory: (path: string) => OpenedProject | null
  selectProject: (path: string) => void
  removeProject: (path: string) => void
  toggleProjects: () => void
  toggleProject: (path: string) => void
  expandProject: (path: string) => void
}

export function projectFromDirectory(path: string): OpenedProject | null {
  const normalized = normalizeDirectoryPath(path)
  if (!normalized) return null
  const segments = normalized.split(/[\\/]/).filter(Boolean)
  return {
    name: segments[segments.length - 1] ?? normalized,
    path: normalized,
  }
}

export function normalizeDirectoryPath(path: string): string {
  const trimmed = path.trim()
  if (!trimmed) return ''
  if (trimmed === '/' || /^[A-Za-z]:[\\/]?$/.test(trimmed)) return trimmed
  return trimmed.replace(/[\\/]+$/, '')
}

export function addOpenedProject(
  projects: OpenedProject[],
  project: OpenedProject,
): OpenedProject[] {
  return projects.some((item) => item.path === project.path) ? projects : [...projects, project]
}

export function removeOpenedProject(projects: OpenedProject[], path: string): OpenedProject[] {
  return projects.filter((project) => project.path !== path)
}

export function toggleCollapsedProject(paths: string[], path: string): string[] {
  return paths.includes(path) ? paths.filter((item) => item !== path) : [...paths, path]
}

function readStoredProjects(): StoredProjects {
  const fallback: StoredProjects = {
    projects: [],
    activeProjectPath: null,
    projectsExpanded: true,
    collapsedProjectPaths: [],
  }
  if (typeof localStorage === 'undefined') return fallback
  try {
    const value = JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY) ?? 'null') as Partial<StoredProjects> | null
    if (!value || !Array.isArray(value.projects)) return fallback
    const projects = value.projects.filter(
      (project): project is OpenedProject =>
        typeof project?.name === 'string' && typeof project?.path === 'string',
    )
    const activeProjectPath = projects.some(
      (project) => project.path === value.activeProjectPath,
    )
      ? (value.activeProjectPath ?? null)
      : (projects[0]?.path ?? null)
    return {
      projects,
      activeProjectPath,
      projectsExpanded: value.projectsExpanded !== false,
      collapsedProjectPaths: Array.isArray(value.collapsedProjectPaths)
        ? value.collapsedProjectPaths.filter(
            (path): path is string =>
              typeof path === 'string' && projects.some((project) => project.path === path),
          )
        : [],
    }
  } catch {
    return fallback
  }
}

function saveStoredProjects(state: StoredProjects): void {
  if (typeof localStorage === 'undefined') return
  localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(state))
}

const initialState = readStoredProjects()

export const useProjectStore = create<ProjectStoreState>((set, get) => ({
  ...initialState,
  openDirectory: (path) => {
    const project = projectFromDirectory(path)
    if (!project) return null
    const next: StoredProjects = {
      projects: addOpenedProject(get().projects, project),
      activeProjectPath: project.path,
      projectsExpanded: true,
      collapsedProjectPaths: get().collapsedProjectPaths.filter(
        (path) => path !== project.path,
      ),
    }
    saveStoredProjects(next)
    set(next)
    return project
  },
  selectProject: (path) => {
    if (!get().projects.some((project) => project.path === path)) return
    const next: StoredProjects = {
      projects: get().projects,
      activeProjectPath: path,
      projectsExpanded: get().projectsExpanded,
      collapsedProjectPaths: get().collapsedProjectPaths,
    }
    saveStoredProjects(next)
    set(next)
  },
  removeProject: (path) => {
    const projects = removeOpenedProject(get().projects, path)
    const activeProjectPath =
      get().activeProjectPath === path ? (projects[0]?.path ?? null) : get().activeProjectPath
    const next: StoredProjects = {
      projects,
      activeProjectPath,
      projectsExpanded: get().projectsExpanded,
      collapsedProjectPaths: get().collapsedProjectPaths.filter((item) => item !== path),
    }
    saveStoredProjects(next)
    set(next)
  },
  toggleProjects: () => {
    const next: StoredProjects = {
      projects: get().projects,
      activeProjectPath: get().activeProjectPath,
      projectsExpanded: !get().projectsExpanded,
      collapsedProjectPaths: get().collapsedProjectPaths,
    }
    saveStoredProjects(next)
    set(next)
  },
  toggleProject: (path) => {
    if (!get().projects.some((project) => project.path === path)) return
    const next: StoredProjects = {
      projects: get().projects,
      activeProjectPath: get().activeProjectPath,
      projectsExpanded: get().projectsExpanded,
      collapsedProjectPaths: toggleCollapsedProject(get().collapsedProjectPaths, path),
    }
    saveStoredProjects(next)
    set(next)
  },
  expandProject: (path) => {
    if (!get().projects.some((project) => project.path === path)) return
    const next: StoredProjects = {
      projects: get().projects,
      activeProjectPath: get().activeProjectPath,
      projectsExpanded: true,
      collapsedProjectPaths: get().collapsedProjectPaths.filter((item) => item !== path),
    }
    saveStoredProjects(next)
    set(next)
  },
}))
