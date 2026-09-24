import { describe, expect, it } from 'vitest'

import { cardIdsOutsideCode, textSegments } from './messageText'

const CARD = `card-${'3f9a1c2e'.repeat(4)}`
const OTHER = `card-${'0'.repeat(32)}`

describe('textSegments', () => {
  it('splits known mentions, @all and card ids out of plain text', () => {
    expect(textSegments(`@bo and @all see ${CARD}, not @zed or me@bo.`, new Set(['bo']))).toEqual([
      { kind: 'mention', id: 'bo' },
      { kind: 'text', text: ' and ' },
      { kind: 'mention', id: 'all' },
      { kind: 'text', text: ' see ' },
      { kind: 'card', id: CARD },
      { kind: 'text', text: ', not @zed or me@bo.' },
    ])
  })

  it('does not treat a longer id or a longer hex run as a match', () => {
    expect(textSegments(`@bob ${CARD}ff`, new Set(['bo']))).toEqual([
      { kind: 'text', text: `@bob ${CARD}ff` },
    ])
  })
})

describe('cardIdsOutsideCode', () => {
  it('lists card ids once in order and skips code', () => {
    const body = [
      `First ${CARD} then \`${OTHER}\`.`,
      '```',
      OTHER,
      '```',
      `Again ${CARD} and ${OTHER.replace('0', '1')}`,
    ].join('\n')
    expect(cardIdsOutsideCode(body)).toEqual([CARD, OTHER.replace('0', '1')])
  })
})
