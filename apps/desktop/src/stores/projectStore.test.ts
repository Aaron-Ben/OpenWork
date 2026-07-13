import { describe, expect, it } from 'vitest'

import {
  addOpenedProject,
  projectFromDirectory,
  removeOpenedProject,
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
})
