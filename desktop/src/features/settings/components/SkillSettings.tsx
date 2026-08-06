import { useState } from 'react'

import type { RuntimeSkillSummary } from '@/bridge/compat'

import { SkillDetail } from './SkillDetail'
import { SkillList } from './SkillList'

export function SkillSettings() {
  const [selectedSkill, setSelectedSkill] = useState<RuntimeSkillSummary | null>(null)

  return (
    <div className="mx-auto w-full max-w-4xl p-8 max-[640px]:p-5">
      {selectedSkill ? (
        <div className="pb-8">
          <SkillDetail skill={selectedSkill} onBack={() => setSelectedSkill(null)} />
        </div>
      ) : (
        <div className="pb-8">
          <SkillList onSelectSkill={setSelectedSkill} />
        </div>
      )}
    </div>
  )
}
