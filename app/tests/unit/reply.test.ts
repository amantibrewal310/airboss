import { describe, expect, it } from 'vitest';
import { blocks, inline, nextSteps } from '../../src/lib/logic/reply';

const MEETING = [
  'I sent the invite for tomorrow, Friday 27 Nov, 3:00–3:45 PM IST.',
  '',
  '- **Calendar event:** [open in Google Calendar](https://www.google.com/calendar/event?eid=x)',
  '- **Agenda in the invite:** voice streaming or SIP trunk.',
  '',
  '**To bring to the meeting:**',
  "1. Whether you accept Knowlarity's pricing.",
  '2. Who prepares the KYC documents.',
  '3. Ask Goodmeetings whether they can provide a WSS address.',
  '',
  'After the meeting, tell me which approach was chosen.',
].join('\n');

describe('nextSteps', () => {
  it('takes the section from a lead line to the end', () => {
    const n = nextSteps(MEETING)!;
    expect(n.lead).toBe('To bring to the meeting');
    expect(n.count).toBe(3);
    expect(n.blocks.map((b) => b.kind)).toEqual(['list', 'para']);
  });

  it('reads a heading or a plain line ending in a colon as a lead', () => {
    expect(nextSteps('Done.\n\n## Next steps\n- Merge it')!.lead).toBe('Next steps');
    expect(nextSteps('Done.\n\nWhat you need to do:\n- Merge it')!.lead).toBe('What you need to do');
  });

  it('reads what can start as a lead, keeping the closing question', () => {
    const n = nextSteps('Merged.\n\n**What can start now:**\n- **11:** the privacy rule.\n- **12:** empathy.\n\nWhich should I take next?')!;
    expect(n.lead).toBe('What can start now');
    expect(n.blocks.map((b) => b.kind)).toEqual(['list', 'para']);
  });

  it('prefers the last lead line', () => {
    const n = nextSteps('**Next steps:**\n- old\n\nDone that.\n\n**Next on your side:**\n- new')!;
    expect(n.lead).toBe('Next on your side');
    expect(n.count).toBe(1);
  });

  it('ignores labels that are not about what is left', () => {
    const n = nextSteps('**Guests:**\n- Sanjeev\n\nNext, send the invite once Sanjeev confirms.')!;
    expect(n.lead).toBeNull();
    expect(n.blocks).toEqual([{ kind: 'para', parts: [{ kind: 'text', text: 'Next, send the invite once Sanjeev confirms.' }] }]);
  });

  it('falls back to a closing list with the line that introduces it', () => {
    const n = nextSteps('All done.\n\nI could also:\n- add tests\n- bump the version')!;
    expect(n.lead).toBeNull();
    expect(n.blocks.map((b) => b.kind)).toEqual(['para', 'list']);
  });

  it('skips code fences and empty replies', () => {
    expect(nextSteps('Run this:\n```\n## Next steps:\n- not real\n```\nThat is all.')!.lead).toBeNull();
    expect(nextSteps('')).toBeNull();
    expect(nextSteps(null)).toBeNull();
  });
});

describe('blocks and inline', () => {
  it('joins wrapped list items and keeps ordered and plain lists apart', () => {
    expect(blocks('1. one\n   more\n2. two\n\n- a')).toEqual([
      { kind: 'list', ordered: true, items: [[{ kind: 'text', text: 'one more' }], [{ kind: 'text', text: 'two' }]] },
      { kind: 'list', ordered: false, items: [[{ kind: 'text', text: 'a' }]] },
    ]);
  });

  it('keeps only https links as links', () => {
    expect(inline('see [PR](https://x.dev/1), [file](file:///etc/passwd) and https://a.dev/b.')).toEqual([
      { kind: 'text', text: 'see ' },
      { kind: 'link', text: 'PR', url: 'https://x.dev/1' },
      { kind: 'text', text: ', ' },
      { kind: 'text', text: 'file' },
      { kind: 'text', text: ' and ' },
      { kind: 'link', text: 'https://a.dev/b', url: 'https://a.dev/b' },
      { kind: 'text', text: '.' },
    ]);
  });

  it('reads bold and code', () => {
    expect(inline('**Next:** run `pnpm test`')).toEqual([
      { kind: 'bold', text: 'Next:' },
      { kind: 'text', text: ' run ' },
      { kind: 'code', text: 'pnpm test' },
    ]);
  });
});
