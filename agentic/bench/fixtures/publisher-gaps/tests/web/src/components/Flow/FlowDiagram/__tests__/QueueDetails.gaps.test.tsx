import { describe, expect, it } from 'vitest'
import QueueDetails from '@/components/Flow/FlowDiagram/QueueDetails'
import { render, screen, within } from '@/test/render'
import { makeLane } from '@/test-utils/fixtures'

function rows(pending: { id: string; type: string; publishedAt?: string }[]) {
    render(<QueueDetails lane={makeLane({ pending, pendingTotal: pending.length, messagesWaiting: pending.length })} />)
    return within(screen.getByRole('list', { name: 'Messages waiting in consumer.billing' })).getAllByRole('listitem')
}

describe('QueueDetails message gaps', () => {
    it('shows the gap to the previous message after each time but the first', () => {
        const [first, second, third] = rows([
            { id: 'a-0001', type: 'order.created', publishedAt: '2026-10-12T10:00:01.000Z' },
            { id: 'b-0002', type: 'order.paid', publishedAt: '2026-10-12T10:00:02.500Z' },
            { id: 'c-0003', type: 'order.sent', publishedAt: '2026-10-12T10:00:02.750Z' },
        ])
        expect(first.textContent).not.toMatch(/\+\d/)
        expect(second).toHaveTextContent('+1.5s')
        expect(third).toHaveTextContent('+250ms')
    })

    it('shows no gap for a message without a time or the message after it', () => {
        const [, second, third, fourth] = rows([
            { id: 'a-0001', type: 'order.created', publishedAt: '2026-10-12T10:00:01.000Z' },
            { id: 'b-0002', type: 'order.paid' },
            { id: 'c-0003', type: 'order.sent', publishedAt: '2026-10-12T10:00:03.000Z' },
            { id: 'd-0004', type: 'order.done', publishedAt: '2026-10-12T10:00:03.040Z' },
        ])
        expect(second.textContent).not.toMatch(/\+\d/)
        expect(third.textContent).not.toMatch(/\+\d/)
        expect(fourth).toHaveTextContent('+40ms')
    })
})
