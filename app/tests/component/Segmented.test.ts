import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { userEvent } from 'vitest/browser';
import SegHost from './SegHost.svelte';

const radio = (name: string) => [...document.querySelectorAll<HTMLButtonElement>('[role="radio"]')].find((b) => b.textContent === name)!;

describe('Segmented (hands-on QA #18)', () => {
  it('moves focus with the checked option on ←/→, so VoiceOver reads the new value', async () => {
    await render(SegHost);
    radio('System').focus();
    await userEvent.keyboard('{ArrowRight}');
    expect(radio('Dark').getAttribute('aria-checked')).toBe('true');
    expect(document.activeElement).toBe(radio('Dark'));
    await userEvent.keyboard('{ArrowRight}');
    expect(document.activeElement).toBe(radio('Light'));
    // Wraps around, both ways.
    await userEvent.keyboard('{ArrowRight}');
    expect(document.activeElement).toBe(radio('System'));
    await userEvent.keyboard('{ArrowLeft}');
    expect(document.activeElement).toBe(radio('Light'));
    expect(radio('Light').getAttribute('aria-checked')).toBe('true');
    expect(radio('Light').tabIndex).toBe(0);
  });
});
