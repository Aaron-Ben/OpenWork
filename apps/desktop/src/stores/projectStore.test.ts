import { describe, expect, it } from 'vitest'

import {
  addOpenedProject,
  projectFromDirectory,
  removeOpenedProject,
  toggleCollapsedProject,
} from './projectStore'

describe('projectStore helpers', () => {
  it('derives a stable project bookmark from a selected directory', () => {
    expect(projectFromDirectory('/Volumes/Code/OpenWork/')).toEqual({
      name: 'OpenWork',
      path: '/Volumes/Code/OpenWork',
    })
  })

  it('deduplicates a directory that is opened more than once', () => {
    const project = projectFromDirectory('/Volumes/Code/OpenWork')
    expect(project).not.toBeNull()

    const once = addOpenedProject([], project!)
    const twice = addOpenedProject(once, project!)

    expect(twice).toEqual(once)
  })

  it('removes only the local bookmark', () => {
    const projects = [
      { name: 'OpenWork', path: '/Volumes/Code/OpenWork' },
      { name: 'mcp-client', path: '/Users/me/Code/mcp-client' },
    ]

    expect(removeOpenedProject(projects, '/Volumes/Code/OpenWork')).toEqual([
      { name: 'mcp-client', path: '/Users/me/Code/mcp-client' },
    ])
  })

  it('toggles one project without changing the collapsed state of another project', () => {
    expect(toggleCollapsedProject([], '/Volumes/Code/OpenWork')).toEqual([
      '/Volumes/Code/OpenWork',
    ])
    expect(
      toggleCollapsedProject(
        ['/Volumes/Code/OpenWork', '/Users/me/Code/mcp-client'],
        '/Volumes/Code/OpenWork',
      ),
    ).toEqual(['/Users/me/Code/mcp-client'])
  })
})
