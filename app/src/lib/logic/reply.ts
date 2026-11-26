/*
  An agent's last reply, read for what's left for me.

  Agents end a turn with the steps that are now mine: "Next on your side:", "To bring to the
  meeting:", "After the meeting, tell me…". `nextSteps` finds that part, from its lead line
  to the end of the reply (it is nearly always the end). With no lead line it falls back to
  the closing paragraph, which is where an agent asks for the next thing.

  `blocks` reads the little Markdown agents write (headings, lists, bold, code, links) into
  plain data the panel renders as text, never as HTML.
*/

export type Inline = { kind: 'text' | 'bold' | 'code'; text: string } | { kind: 'link'; text: string; url: string };

export type Block =
  | { kind: 'para'; parts: Inline[] }
  | { kind: 'heading'; parts: Inline[] }
  | { kind: 'list'; ordered: boolean; items: Inline[][] };

export type NextSteps = {
  /** The lead line without its Markdown ("Next on your side"), or null for the fallback. */
  lead: string | null;
  /** The section after the lead line, or the closing paragraph. */
  blocks: Block[];
  /** How many list items the section has, for "3 next steps". */
  count: number;
};

/** A lead line names what's left: next steps, my side, to do, follow-ups, open questions. */
const LEAD =
  /\b(next|to[- ]?do|todo|your (side|turn|call)|for you|you('ll)? need|action items?|follow[- ]?ups?|pending|left to|remaining|still (open|to)|open questions?|to bring|before (you|we)|decide|decisions?|waiting on|blocked on|what's left|needs? from you|can start|start (now|next)|still there|not yet done|options|choose|approve|confirm)\b/i;

const LIST = /^\s*(?:[-*+]|(\d+)[.)])\s+(.*)$/;
const HEADING = /^\s*#{1,6}\s+(.*?)\s*#*\s*$/;
const FENCE = /^\s*(```|~~~)/;

/** A line that introduces a section: a heading, a bold line, or a short line ending in ":". */
function leadText(line: string): string | null {
  const t = line.trim();
  const h = HEADING.exec(t);
  if (h) return strip(h[1] ?? '');
  const bold = /^\*\*(.+?)\*\*:?\s*$/.exec(t) ?? /^__(.+?)__:?\s*$/.exec(t);
  if (bold) return strip(bold[1] ?? '');
  if (t.endsWith(':') && t.length <= 80 && !LIST.test(t)) return strip(t);
  return null;
}

function strip(text: string): string {
  return text
    .replace(/\*\*|__|`/g, '')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/:\s*$/, '')
    .trim();
}

/** The reply's lines with fenced code removed: steps aren't in code, and fences confuse the scan. */
function prose(reply: string): string[] {
  const out: string[] = [];
  let inFence = false;
  for (const line of reply.replace(/\r\n?/g, '\n').split('\n')) {
    if (FENCE.test(line)) {
      inFence = !inFence;
      continue;
    }
    if (!inFence) out.push(line);
  }
  return out;
}

export function nextSteps(reply: string | null | undefined): NextSteps | null {
  if (!reply?.trim()) return null;
  const lines = prose(reply);
  // The last lead line wins: a reply that recaps "Next steps" from earlier and then lists
  // new ones means the new ones.
  for (let i = lines.length - 1; i >= 0; i--) {
    const lead = leadText(lines[i] ?? '');
    if (lead === null || !LEAD.test(lead)) continue;
    const rest = blocks(lines.slice(i + 1).join('\n'));
    if (!rest.length) continue;
    return { lead, blocks: rest, count: count(rest) };
  }
  const all = blocks(lines.join('\n'));
  const last = all.at(-1);
  if (!last) return null;
  // A closing list keeps the line that introduces it.
  const before = all.at(-2);
  const tail = last.kind === 'list' && before?.kind === 'para' ? [before, last] : [last];
  return { lead: null, blocks: tail, count: count(tail) };
}

function count(bs: Block[]): number {
  return bs.reduce((n, b) => n + (b.kind === 'list' ? b.items.length : 0), 0);
}

/** Paragraphs, headings and lists. Indented lines carry on the item above them. */
export function blocks(text: string): Block[] {
  const out: Block[] = [];
  let para: string[] = [];
  let list: { ordered: boolean; items: string[] } | null = null;
  const flush = () => {
    if (para.length) out.push({ kind: 'para', parts: inline(para.join(' ')) });
    if (list) out.push({ kind: 'list', ordered: list.ordered, items: list.items.map(inline) });
    para = [];
    list = null;
  };
  for (const raw of prose(text)) {
    const line = raw.trimEnd();
    if (!line.trim()) {
      flush();
      continue;
    }
    const h = HEADING.exec(line);
    if (h) {
      flush();
      out.push({ kind: 'heading', parts: inline(h[1] ?? '') });
      continue;
    }
    const li = LIST.exec(line);
    if (li) {
      if (para.length) {
        out.push({ kind: 'para', parts: inline(para.join(' ')) });
        para = [];
      }
      const ordered = li[1] !== undefined;
      if (list && list.ordered !== ordered && !/^\s{2,}/.test(line)) flush();
      list ??= { ordered, items: [] };
      list.items.push((li[2] ?? '').trim());
      continue;
    }
    if (list && /^\s+\S/.test(raw)) {
      const n = list.items.length - 1;
      list.items[n] = `${list.items[n] ?? ''} ${line.trim()}`;
      continue;
    }
    if (list) flush();
    para.push(line.trim());
  }
  flush();
  return out;
}

/** Bold, inline code and links; everything else is text. Only https links are kept as links. */
export function inline(text: string): Inline[] {
  const out: Inline[] = [];
  const re = /\*\*(.+?)\*\*|__(.+?)__|`([^`]+)`|\[([^\]]+)\]\(([^)\s]+)\)|(https:\/\/[^\s)<>]+[^\s)<>.,;:!?'"])/g;
  let at = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > at) out.push({ kind: 'text', text: text.slice(at, m.index) });
    const bold = m[1] ?? m[2];
    const [, , , code, label, href = ''] = m;
    if (bold) out.push({ kind: 'bold', text: bold });
    else if (code) out.push({ kind: 'code', text: code });
    else if (label) out.push(href.startsWith('https://') ? { kind: 'link', text: label, url: href } : { kind: 'text', text: label });
    else if (m[6]) out.push({ kind: 'link', text: m[6], url: m[6] });
    at = m.index + m[0].length;
  }
  if (at < text.length) out.push({ kind: 'text', text: text.slice(at) });
  return out;
}
